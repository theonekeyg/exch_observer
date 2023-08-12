use std::{
    collections::HashMap,
    sync::{Arc, RwLock, mpsc},
    net::{TcpListener, TcpStream},
    hash::Hash,
    marker::PhantomData,
    fmt::{Debug, Display},
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
use log::info;

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
    pub subscribers: HashMap<ExchangeKind, Vec<WebSocket<TcpListener>>>,
}

unsafe impl Send for ObserverWsRunner {}
unsafe impl Sync for ObserverWsRunner {}

impl ObserverWsRunner
    {
    pub fn new(observer: &Arc<RwLock<CombinedObserver<ExchangeSymbol>>>, config: WsConfig) -> Self {

        let mut exchange_kind_to_rx = HashMap::new();
        {
            let mut mut_observer = observer.write().expect("Failed to lock RWLock for writing");
            let keys = mut_observer.get_supported_observers();
            for key in keys {
                let (tx, rx) = mpsc::channel();

                // Insert the tx channel to the combined observer
                mut_observer.set_tx_fifo(key.clone(), tx.clone());

                // Insert the rx channel to our reader map
                exchange_kind_to_rx.insert(key, rx);
            }
        }

        Self {
            config: config,
            observer: observer.clone(),
            exchange_kind_to_rx: exchange_kind_to_rx,
            subscribers: HashMap::new(),
        }
    }

    pub async fn run(&mut self) {
        info!("Starting WS server on port {}", self.config.port.unwrap());

        let server = TcpListener::bind(
            format!(
                "{}:{}",
                self.config.host.clone().unwrap(),
                self.config.port.unwrap())
            ).unwrap();

        for stream in server.incoming() {
            let stream = stream.expect("Failed to establish connection with incoming client");
            info!("New ws connection: {:?}", stream);

            self.handle_new_ws_connection(stream);
        }
    }

    fn handle_new_ws_connection(&mut self, stream: TcpStream) {
        let ws_stream = tungstenite::accept(stream).expect("Failed to accept ws connection");
    }

    /*

/// A WebSocket Prices server
fn main () {
    let server = TcpListener::bind("127.0.0.1:9001").unwrap();
    for stream in server.incoming() {
        println!("New client!: {:?}", stream);
        spawn (move || {
            let mut prices_table: HashMap<String, u64> = HashMap::new();

            prices_table.insert(String::from("ethusdt"), 2700);

            let mut websocket = accept(stream.unwrap()).unwrap();

            let prices_str = serde_json::to_string(&prices_table).unwrap();
            let msg = Message::Text(String::from(prices_str));
            websocket.send(msg).unwrap();

            loop {
                let msg = websocket.read().unwrap();
                websocket.send(msg).unwrap();
            }
        });
    }
}
    */
}
