use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock, mpsc},
    net::{TcpListener, TcpStream},
    hash::Hash,
    marker::PhantomData,
    fmt::{Debug, Display},
    io::Write,
};
use exch_observer_config::{
    WsConfig, ObserverConfig
};
use exch_subobservers::{
    CombinedObserver,
};
use exch_observer_types::{
    ExchangeKind, ExchangeSymbol, PriceUpdateEvent, PairedExchangeSymbol
};
use tungstenite::{
    protocol::{Message as WsMessage, WebSocket}
};
use tokio::runtime::Runtime;
use log::{info, debug};
use uuid::Uuid;

#[derive(Debug)]
/// Structure that represents a unique client of a websocket server
pub struct WsClient {
    /// Unique identifier of the client
    pub uuid: Uuid,
    /// Websocket stream to the client
    pub ws: WebSocket<TcpStream>,
}

impl WsClient {
    pub fn new(uuid: Uuid, ws: WebSocket<TcpStream>) -> Self {
        Self {
            uuid: uuid,
            ws: ws,
        }
    }
}

/// Internal structure that holds and updates the state of the websocket server.
/// Manages subscribers and tasks for listening to the price updates.
struct ObserverWsDriver {
    /// Map of exchange kind to websocket subscribers
    pub subscribers: HashMap<ExchangeKind, Arc<Mutex<Vec<WsClient>>>>,

    /// Observer that will be used to get the prices
    observer: Arc<RwLock<CombinedObserver<ExchangeSymbol>>>,

    pub runtime: Arc<Runtime>,
}

impl ObserverWsDriver {

    pub fn new(
        subscribers: HashMap<ExchangeKind, Arc<Mutex<Vec<WsClient>>>>,
        observer: Arc<RwLock<CombinedObserver<ExchangeSymbol>>>,
        runtime: Arc<Runtime>,
    ) -> Self {
        Self {
            subscribers: subscribers,
            observer: observer,
            runtime: runtime,
        }
    }

    /// Spwans internal loop that awaits for new price updates and sends them to all subscribers.
    /// for `exchange`.
    pub fn rx_block_loop(
        exchange: ExchangeKind,
        rx: mpsc::Receiver<PriceUpdateEvent>,
        subscribers: Arc<Mutex<Vec<WsClient>>>
    ) {
        debug!("Starting rx_block_loop for {}", exchange.to_str());
        loop {
            let event = rx.recv().unwrap();
            let msg_text = serde_json::to_string(&event).unwrap();
            let msg = WsMessage::Text(msg_text);

            // Keep track of the subscribers that returned an error to remove them from internal
            // list of subscribers later
            let mut drop_indicies = vec![];
            for (i, subscriber) in subscribers.lock().expect("Failed to acquire mutex").iter_mut().enumerate() {
                match subscriber.ws.send(msg.clone()) {
                    Ok(_) => {},
                    Err(e) => {
                        debug!("Closing connection to subscriber due to error: {:?}", e);
                        drop_indicies.push(i);
                    }
                }
            }

            // Remove the subscribers that returned an error, we can do it this way, since array is
            // sorted
            let mut i = 0;
            for index in drop_indicies {
                subscribers.lock().expect("Failed to acquire mutex").remove(index - i);
                i += 1;
            }
        }
    }

    pub fn handle_new_ws_connection(&mut self, stream: TcpStream, exchange: ExchangeKind) {
        let mut ws_stream = tungstenite::accept(stream).expect("Failed to accept ws connection");

        let prices_dump = self.observer
            .read()
            .expect("Failed to lock RWLock for reading")
            .dump_price_table(exchange.clone());

        let msg_text = serde_json::to_string(&prices_dump).unwrap();
        let msg = WsMessage::Text(msg_text);

        ws_stream.send(msg).unwrap();

        let new_user = WsClient::new(Uuid::new_v4(), ws_stream);

        // Add websocket stream to the subscribers list
        if let Some(subscribers) = self.subscribers.get_mut(&exchange) {
            subscribers.lock().expect("Failed to acquire lock").push(new_user);
        } else {
            self.subscribers.insert(exchange, Arc::new(Mutex::new(vec![new_user])));
        }
    }

    pub fn spawn_new_server(
        &mut self,
        host: String,
        port: u16,
        rx: mpsc::Receiver<PriceUpdateEvent>,
        exchange: ExchangeKind,
    ) -> Result<(), Box<dyn std::error::Error>> {

        // Spawn websocket task to await incoming updates and send them to subscribers.
        let subscribers = self.subscribers.get(&exchange).unwrap().clone();
        let _exchange = exchange.clone();
        self.runtime.spawn_blocking(move || {
            Self::rx_block_loop(_exchange, rx, subscribers);
        });

        info!("Starting {} WS server on port {}", exchange.to_str(), port);
        let server = TcpListener::bind(
            format!("{}:{}", host, port)
        )?;

        // Handle incoming connections
        for stream in server.incoming() {
            let stream = stream.expect("Failed to establish connection with incoming client");
            info!("New ws connection: {:?}", stream);

            self.handle_new_ws_connection(stream, exchange.clone());
        }

        Ok(())
    }
}

/// This struct serves as a main interface to exch_observer WS service.
/// It sends the data about the prices to the connected WS clients.
pub struct ObserverWsRunner
{
    /// Config for the ws server
    pub config: WsConfig,

    /// Configs for individual observers
    pub obs_config: ObserverConfig,

    /// Map of exchange kind to mpsc channel receiver
    /// that will be used to send the prices to the subscribed WS clients
    pub exchange_kind_to_rx: HashMap<ExchangeKind, mpsc::Receiver<PriceUpdateEvent>>,

    /// Driver for the websocket server
    driver: Arc<Mutex<ObserverWsDriver>>,
}

unsafe impl Send for ObserverWsRunner {}
unsafe impl Sync for ObserverWsRunner {}

impl ObserverWsRunner {
    pub fn new(observer: &Arc<RwLock<CombinedObserver<ExchangeSymbol>>>, runtime: Arc<Runtime>, config: WsConfig, obs_config: ObserverConfig) -> Self {

        // Create a map of exchange kind to mpsc channel receiver
        let mut exchange_kind_to_rx = HashMap::new();
        let mut subscribers = HashMap::new();
        {
            let mut mut_observer = observer.write().expect("Failed to lock RWLock for writing");
            let supported_kinds = mut_observer.get_supported_observers();
            for kind in supported_kinds {
                let (tx, rx) = mpsc::channel();

                // Insert the tx channel to the combined observer
                mut_observer.set_tx_fifo(kind.clone(), tx.clone());

                // Insert the rx channel to our reader map
                exchange_kind_to_rx.insert(kind.clone(), rx);

                subscribers.insert(kind, Arc::new(Mutex::new(vec![])));
            }
        }

        // Create ws driver
        let ws_driver = Arc::new(Mutex::new(ObserverWsDriver::new(
            subscribers,
            observer.clone(),
            runtime,
        )));

        Self {
            config: config,
            exchange_kind_to_rx: exchange_kind_to_rx,
            obs_config: obs_config,
            driver: ws_driver,
        }
    }

    pub async fn run(&mut self) {
        // Start rx loops for each exchange kind that was added so far.
        for (kind, rx) in self.exchange_kind_to_rx.drain() {
            match kind {
                ExchangeKind::Binance => {
                    if let Some(config) = &self.obs_config.binance {
                        self.driver.lock().expect("Failed to capture RWLock").spawn_new_server(
                            self.config.host.clone().unwrap(),
                            config.ws_port,
                            rx,
                            kind,
                        ).unwrap();
                    }
                },

                ExchangeKind::Huobi => {
                    if let Some(config) = &self.obs_config.huobi {
                        self.driver.lock().expect("Failed to capture RWLock").spawn_new_server(
                            self.config.host.clone().unwrap(),
                            config.ws_port,
                            rx,
                            kind,
                        ).unwrap();
                    }
                }

                _ => unimplemented!(),
            }
            // self.driver.write().spawn_new_server(
        }
    }
}
