use std::{
    cmp::PartialEq,
    net::TcpStream,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex
    },
    fmt::{Display, Debug},
    collections::HashMap,
    time::Duration,
    hash::Hash,
};
use exch_observer_types::{
    ExchangeKind, AskBidValues, PriceUpdateEvent, ExchangeSymbol, OrderedExchangeSymbol,
    USD_STABLES, PairedExchangeSymbol, ExchangeValues, SwapOrder
};
use exch_observer_config::{
    ObserverConfig, WsConfig
};
use dashmap::DashMap;
use tungstenite::{
    connect, handshake::client::Response, protocol::WebSocket, stream::MaybeTlsStream, Message,
};
use ringbuf::{
    HeapRb, SharedRb,
    producer::Producer,
    consumer::Consumer
};
use log::error;
use tokio::{task::JoinHandle, runtime::{Runtime}};
use crate::error::{
    ObserverError, ObserverResult as OResult
};

struct WsObserverClientThreadData {
    /// Producer of the SPSC queue
    pub rx: Producer<PriceUpdateEvent, Arc<HeapRb<PriceUpdateEvent>>>,
    /// URL to the ws server
    pub ws_uri: String,
    /// Exchange kind
    pub exchange: ExchangeKind,
}

impl WsObserverClientThreadData {
    pub fn new(
        rx: Producer<PriceUpdateEvent, Arc<HeapRb<PriceUpdateEvent>>>,
        ws_uri: String,
        exchange: ExchangeKind,
    ) -> Self {
        Self {
            rx: rx,
            ws_uri: ws_uri,
            exchange: exchange,
        }
    }
}

/// This structure represents remote exch_observer server with WS interface enabled.
/// It is used to connect to remove WS server and listen to updates on it.
/// Push the updates into the local SPSC queue.
pub struct WsRemoteObserverClient {
    /// Internal map that stores websocket connections to the remote server.
    ws_sockets: HashMap<ExchangeKind, WebSocket<MaybeTlsStream<TcpStream>>>,

    /// Internal map that stores necessary info for each exchange
    thread_data_vec: Vec<WsObserverClientThreadData>,

    /// Must be used by the used of WsRemoteObserverClient to get consumer of the queues.
    /// It fills from the config in the constructor and must be consumed by the higher
    /// application, for example, by using `HashMap::drain`.
    pub rx_map: HashMap<ExchangeKind, Consumer<PriceUpdateEvent, Arc<HeapRb<PriceUpdateEvent>>>>,

    /// Configs for individual observers
    pub obs_config: ObserverConfig,

    /// Websocket config
    pub ws_config: WsConfig,

    jobs: Vec<JoinHandle<()>>,

    pub runtime: Arc<Runtime>,
}

impl WsRemoteObserverClient {
    pub fn new(runtime: Arc<Runtime>, obs_config: ObserverConfig, ws_config: WsConfig) -> Self {

        let mut thread_data_vec = Vec::new();
        let mut rx_map = HashMap::new();

        // Create SPSC queues for each exchange that is enabled in the config.
        // Add binance
        if let Some(config) = &obs_config.binance {
            if config.enable {
                let (tx, rx) = HeapRb::<PriceUpdateEvent>::new(1000).split();
                let thread_data = WsObserverClientThreadData::new(
                    tx,
                    format!("ws://{}:{}", ws_config.host, config.ws_port),
                    ExchangeKind::Binance,
                );

                thread_data_vec.push(thread_data);

                rx_map.insert(ExchangeKind::Binance, rx);
            }
        }

        // Add huobi
        if let Some(config) = &obs_config.huobi {
            if config.enable {
                let (tx, rx) = HeapRb::<PriceUpdateEvent>::new(1000).split();
                let thread_data = WsObserverClientThreadData::new(
                    tx,
                    format!("ws://{}:{}", ws_config.host, config.ws_port),
                    ExchangeKind::Huobi,
                );

                thread_data_vec.push(thread_data);
                rx_map.insert(ExchangeKind::Huobi, rx);
            }
        }

        // Add kraken
        if let Some(config) = &obs_config.kraken {
            if config.enable {
                let (tx, rx) = HeapRb::<PriceUpdateEvent>::new(1000).split();
                let thread_data = WsObserverClientThreadData::new(
                    tx,
                    format!("ws://{}:{}", ws_config.host, config.ws_port),
                    ExchangeKind::Kraken
                );

                thread_data_vec.push(thread_data);
                rx_map.insert(ExchangeKind::Kraken, rx);
            }
        }

        Self {
            ws_sockets: HashMap::new(),
            thread_data_vec: thread_data_vec,
            rx_map: rx_map,
            obs_config: obs_config,
            ws_config: ws_config,
            jobs: vec![],
            runtime: runtime
        }
    }

    fn spawn_ws_handler(runtime: Arc<Runtime>, mut thread_data: WsObserverClientThreadData) -> JoinHandle<()> {
        runtime.spawn_blocking(move || {
            let (mut ws_stream, _) = connect(&thread_data.ws_uri)
                .expect(
                    format!("Failed to connect to ws server: {}", thread_data.ws_uri).as_str()
                );

            loop {
                let res = ws_stream.read().unwrap();

                match &res {
                    Message::Text(text) => {
                        // Deserialize the message and push it into the queue
                        // let event: PriceUpdateEvent = serde_json::from_str(text);
                        if let Ok(event) = serde_json::from_str::<PriceUpdateEvent>(text) {
                            thread_data
                                .rx
                                .push(event)
                                .expect("Failed to push event into the queue");
                        } else if let Ok(mut event) = serde_json::from_str::<Vec<PriceUpdateEvent>>(text) {
                            for e in event.drain(0..) {
                                thread_data
                                    .rx
                                    .push(e.clone())
                                    .expect("Failed to push event into the queue");
                            }
                        } else {
                            error!("Failed to deserialize the message: {}", text);
                        }
                    },
                    _ => {}
                }

                println!("Received a message from the server! {:?}", res);
            }
        })
    }

    // Start the main WS client application loop.
    pub fn start(&mut self) {
        for thread_data in self.thread_data_vec.drain(0..) {
            self.jobs.push(Self::spawn_ws_handler(self.runtime.clone(), thread_data));
        }
    }

    pub fn stop(&mut self) {
        for job in &mut self.jobs {
            println!("Aborting task");
            job.abort();
        }
    }
}

/// Internal threads that handles each exchange, including websocket connection, price table,
/// and other necessary data.
struct RemoteObserverDriver<Symbol>
where
    Symbol: Eq
        + Hash
        + Clone
        + Display
        + Debug
        + Into<String>
        + Send
        + Sync
        + PairedExchangeSymbol
        + 'static,
{
    /// `price_table` - Represents the main storage for prices, as well as accessing the
    /// prices in the storage. Here key is the pair name (e.g. ethusdt), not the single
    /// token like in `connected_symbols`. It is made this way to be able to index this
    /// map from already concatenated pair names, when turning concatenated string into
    /// ExchangeSymbol is impossible.
    price_table: Arc<DashMap<String, Arc<Mutex<AskBidValues>>>>,

    /// `connected_symbols` - Connected symbols represent all existing pools on certain token,
    /// hence here `key` is single token (e.g. eth, btc), not pair (e.g. ethusdt).
    pub connected_symbols: Arc<DashMap<String, Vec<OrderedExchangeSymbol<Symbol>>>>,

    // /// Internal consumer of events emitted by the WS client.
    // rx: Consumer<PriceUpdateEvent, Arc<HeapRb<PriceUpdateEvent>>>,

    job: Option<JoinHandle<()>>,

    runtime: Arc<Runtime>,
}

impl<Symbol> RemoteObserverDriver<Symbol>
where
    Symbol: Eq
        + Hash
        + Clone
        + Display
        + Debug
        + Into<String>
        + Send
        + Sync
        + PairedExchangeSymbol
        + 'static,
{
    pub fn new(
        rx: Consumer<PriceUpdateEvent, Arc<HeapRb<PriceUpdateEvent>>>,
        runtime: Arc<Runtime>,
    ) -> Self {
        let price_table = Arc::new(DashMap::new());
        let connected_symbols = Arc::new(DashMap::new());

        let job = Self::spawn_listen_task(
            rx,
            price_table.clone(),
            connected_symbols.clone(),
            runtime.clone(),
        );

        Self {
            price_table: price_table,
            connected_symbols: connected_symbols,
            job: Some(job),
            runtime: runtime,
        }
    }

    fn spawn_listen_task(
        mut rx: Consumer<PriceUpdateEvent, Arc<HeapRb<PriceUpdateEvent>>>,
        price_table: Arc<DashMap<String, Arc<Mutex<AskBidValues>>>>,
        connected_symbols: Arc<DashMap<String, Vec<OrderedExchangeSymbol<Symbol>>>>,
        runtime: Arc<Runtime>,
    ) -> JoinHandle<()> {

        runtime.spawn_blocking(move || {
            loop {

                // It expects an empty state of price_table and connection_symbols.
                // If initializes them with required structues on-the-fly.
                if let Some(update) = rx.pop() {
                    let symbol_idx = update.symbol.to_string();
                    let symbol = update.symbol;

                    // If the symbol is already in the price table, simply update the price,
                    // otherwise add it to the price table first.
                    if let Some(price) = price_table.get(&symbol_idx) {
                        *price.lock().expect("Failed to get mutex") = update.price;
                    } else {
                        price_table.insert(symbol_idx.clone(), Arc::new(Mutex::new(update.price)));
                    }


                    // The symbol could be added to the connected_symbolsk
                    if !connected_symbols.contains_key(symbol.base()) {
                        connected_symbols
                            .insert(symbol.base().to_string().clone(), Vec::new());
                    }

                    if !connected_symbols.contains_key(symbol.quote()) {
                        connected_symbols
                            .insert(symbol.quote().to_string().clone(), Vec::new());
                    }


                    // Insert OrderedExchangeSymbol into the connected_symbols map for both
                    // base and quote tokens
                    connected_symbols
                        .get_mut(symbol.base())
                        .expect("INTERNAL ERROR: Base symbol wasn't found")
                        .push(OrderedExchangeSymbol::new(&(symbol as Symbol), SwapOrder::Sell));
                    connected_symbols
                        .get_mut(symbol.quote())
                        .expect("INTERNAL ERROR: Quote symbol wasn't found")
                        .push(OrderedExchangeSymbol::new(&symbol, SwapOrder::Buy));

                } else {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        })
    }

    pub fn start(&mut self) {
    }
}

/// Main structure to view realtime prices on remote observer. It receives Websocket price
/// updates from the remote WS observer server and stores them in the `price_table` map structure.
pub struct WsRemoteObserver {
    /// Price mapping table. It maps the exchange kind to another map that maps the symbol to
    /// the current price value. It stores the result in `Arc<Mutex<...>>` so you can share it
    price_table: Arc<DashMap<ExchangeKind, Arc<DashMap<String, Arc<Mutex<AskBidValues>>>>>>,

    /// `connected_symbols` - Connected symbols represent all existing pools on certain token,
    /// hence here `key` is single token (e.g. eth, btc), not pair (e.g. ethusdt).
    pub connected_symbols: Arc<DashMap<ExchangeKind, Arc<DashMap<String, Vec<OrderedExchangeSymbol<ExchangeSymbol>>>>>>,
    obs_config: ObserverConfig,
    ws_config: WsConfig,

    ws_client: WsRemoteObserverClient,

    runtime: Arc<Runtime>,
}

impl WsRemoteObserver {

    pub fn new(runtime: Arc<Runtime>, obs_config: ObserverConfig, ws_config: WsConfig) -> Self {
        let ws_client = WsRemoteObserverClient::new(runtime.clone(), obs_config.clone(), ws_config.clone());
        let price_table = Arc::new(DashMap::new());

        // Insert empty maps for each exchange that is enabled in the config.
        // Add binance
        if let Some(config) = &obs_config.binance {
            if config.enable {
                price_table.insert(
                    ExchangeKind::Binance,
                    Arc::new(DashMap::new())
                );
            }
        }

        // Add huobi
        if let Some(config) = &obs_config.huobi {
            if config.enable {
                price_table.insert(
                    ExchangeKind::Huobi,
                    Arc::new(DashMap::new())
                );
            }
        }

        // Add kraken
        if let Some(config) = &obs_config.kraken {
            if config.enable {
                price_table.insert(
                    ExchangeKind::Kraken,
                    Arc::new(DashMap::new())
                );
            }
        }

        Self {
            price_table: price_table,
            connected_symbols: Arc::new(DashMap::new()),
            obs_config: obs_config,
            ws_config: ws_config,
            ws_client: ws_client,
            runtime: runtime
        }
    }

    fn start_listener_threads(&mut self) {
    }

    pub fn start(&mut self) {
        // Start listener threads
        self.start_listener_threads();

        // Start inner WS client
        self.ws_client.start();
    }

    /// Get all pools in which this symbol appears, very useful for most strategies
    pub fn get_interchanged_symbols(&self, exchange: ExchangeKind, symbol: &String)
        -> OResult<&'_ Vec<OrderedExchangeSymbol<ExchangeSymbol>>> {
        // &self
        //     .connected_symbols
        //     .get(exchange)
        //     .expect("Symbol wasn't found")
        //
        if let Some(connected_symbols) = self.connected_symbols.get(&exchange) {
            if let Some(connected_symbols) = connected_symbols.get(symbol) {
                return Ok(connected_symbols.value());
            }

            return Err(ObserverError::SymbolNotFound(exchange.clone(), symbol.clone()).into());
        }

        Err(ObserverError::ExchangeNotFound(exchange).into())
    }

    /// Fetches price on certain symbol from the observer
    pub fn get_price_from_table(&self, exchange: ExchangeKind, symbol: &ExchangeSymbol) -> OResult<Arc<Mutex<AskBidValues>>> {
        let symbol = <ExchangeSymbol as Into<String>>::into(symbol.clone());

        if let Some(price_map) = self.price_table.get(&exchange) {
            if let Some(inner) = price_map.get(&symbol) {
                return Ok(inner.value().clone());
            }
        }

        Err(ObserverError::ExchangeNotFound(exchange).into())
    }

    /// Returns value of certain token to usd if available
    pub fn get_usd_value(&self, exchange: ExchangeKind, sym: &String) -> OResult<f64> {
        // TODO: This lock USD wrapped tokens to 1 seems to be unnecessary,
        // considering to remove this later
        if let Some(_) = USD_STABLES.into_iter().find(|v| v == sym) {
            return Ok(1.0);
        };

        let connected = self.get_interchanged_symbols(exchange.clone(), sym)?;

        for ordered_sym in connected.iter() {
            for stable in &USD_STABLES {
                if <&str as Into<String>>::into(stable) == ordered_sym.symbol.base()
                    || <&str as Into<String>>::into(stable) == ordered_sym.symbol.quote()
                {
                    return self.get_price_from_table(exchange, &ordered_sym.symbol).map(|v| {
                        let unlocked = v.lock().expect("Failed to get mutex lock for price");
                        (unlocked.get_ask_price() + unlocked.get_bid_price()) / 2.0
                    });
                }
            }
        }

        Err(ObserverError::ExchangeNotFound(exchange).into())
    }
}
/*
pub trait ExchangeObserver<Symbol: Eq + Hash> {
    /// Get all pools in which this symbol appears, very useful for most strategies
    fn get_interchanged_symbols(&self, symbol: &String) -> &'_ Vec<OrderedExchangeSymbol<Symbol>>;

    /// Adds price to the monitor
    fn add_price_to_monitor(&mut self, symbol: &Symbol, price: Arc<Mutex<Self::Values>>);

    /// Fetches price on certain symbol from the observer
    fn get_price_from_table(&self, symbol: &Symbol) -> Option<Arc<Mutex<Self::Values>>>;

    /// Initialize the runtime, if observer requires one
    fn start(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    /// Starts the threads that will ping existing threads, if some handle appears to be dead, it
    /// will be removed from the observer
    // fn spawn_await_on_handles(

    /// Allegedly remove symbol from watching table, if your observer has one, if not,
    /// this might be an nop
    fn remove_symbol(&mut self, symbol: Symbol);

    /// Returns value of certain token to usd if available
    fn get_usd_value(&self, sym: &String) -> Option<f64>;

    /// Returns the reference to vector of symbols that are being watched
    fn get_watching_symbols(&self) -> &'_ Vec<Symbol>;

    /// Function to set tx sender to send messages on price updates from observer.
    fn set_tx_fifo(&mut self, tx: mpsc::Sender<PriceUpdateEvent>);

    /// Function to dump the existing prices into a newly created HashMap. Pretty expensive
    /// function to call.
    fn dump_price_table(&self) -> HashMap<Symbol, Self::Values>;
}
*/
