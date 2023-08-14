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
    WsConfig,
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

/// This struct serves as a main interface to exch_observer WS service.
/// It sends the data about the prices to the connected WS clients.
pub struct ObserverWsRunner
{
    /// Config for the ws server
    pub config: WsConfig,
    /// Observer that will be used to get the prices
    observer: Arc<RwLock<CombinedObserver<ExchangeSymbol>>>,

    /// Map of exchange kind to mpsc channel receiver
    /// that will be used to send the prices to the subscribed WS clients
    pub exchange_kind_to_rx: HashMap<ExchangeKind, mpsc::Receiver<PriceUpdateEvent>>,

    /// Map of exchange kind to websocket subscribers
    pub subscribers: HashMap<ExchangeKind, Arc<Mutex<Vec<WebSocket<TcpStream>>>>>,

    pub runtime: Arc<Runtime>,
}

unsafe impl Send for ObserverWsRunner {}
unsafe impl Sync for ObserverWsRunner {}

impl ObserverWsRunner
    {
    pub fn new(observer: &Arc<RwLock<CombinedObserver<ExchangeSymbol>>>, runtime: Arc<Runtime>, config: WsConfig) -> Self {

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

        Self {
            config: config,
            observer: observer.clone(),
            exchange_kind_to_rx: exchange_kind_to_rx,
            subscribers: subscribers,
            runtime: runtime,
        }
    }

    pub async fn run(&mut self) {
        // Start rx loops for each exchange kind that was added so far.
        for (kind, rx) in self.exchange_kind_to_rx.drain() {
            let subscribers = self.subscribers.get(&kind).unwrap().clone();

            self.runtime.spawn_blocking(move || {
                Self::rx_block_loop(kind, rx, subscribers);
            });
        }

        info!("Starting WS server on port {}", self.config.port.unwrap());

        // Create the tcp binding for the server
        let server = TcpListener::bind(
            format!(
                "{}:{}",
                self.config.host.clone().unwrap(),
                self.config.port.unwrap())
            ).unwrap();

        // Handle incoming connections
        for stream in server.incoming() {
            let stream = stream.expect("Failed to establish connection with incoming client");
            info!("New ws connection: {:?}", stream);

            self.handle_new_ws_connection(stream);
        }
    }

    fn rx_block_loop(
        kind: ExchangeKind,
        rx: mpsc::Receiver<PriceUpdateEvent>,
        subscribers: Arc<Mutex<Vec<WebSocket<TcpStream>>>>
    ) {
        debug!("Starting rx_block_loop for {}", kind.to_str());
        loop {
            let event = rx.recv().unwrap();
            debug!("New event: {:?}", event);
            let msg_text = serde_json::to_string(&event).unwrap();
            let msg = WsMessage::Text(msg_text);

            for subscriber in subscribers.lock().expect("Failed to acquire mutex").iter_mut() {
                match subscriber.send(msg.clone()) {
                    Ok(_) => {},
                    Err(e) => {
                        debug!("Closing connection to subscriber: {:?}", e);
                        // Remove the subscriber from the list
                    }
                }
            }
        }
    }

    fn handle_new_ws_connection(&mut self, stream: TcpStream) {
        let mut ws_stream = tungstenite::accept(stream).expect("Failed to accept ws connection");

        let prices_dump = self.observer.read().expect("Failed to lock RWLock for reading").dump_price_table(ExchangeKind::Binance);

        let msg_text = serde_json::to_string(&prices_dump).unwrap();
        let msg = WsMessage::Text(msg_text);

        ws_stream.send(msg).unwrap();

        // Add websocket stream to the subscribers list
        if let Some(subscribers) = self.subscribers.get_mut(&ExchangeKind::Binance) {
            subscribers.lock().expect("Failed to acquire lock").push(ws_stream);
        } else {
            self.subscribers.insert(ExchangeKind::Binance, Arc::new(Mutex::new(vec![ws_stream])));
        }
    }
}
