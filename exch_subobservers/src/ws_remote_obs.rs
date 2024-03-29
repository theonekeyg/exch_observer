use crate::error::{ObserverError, ObserverResult as OResult};
use dashmap::{mapref::one::Ref, DashMap, DashSet};
use exch_observer_config::{ObserverConfig, WsConfig};
use exch_observer_types::{
    ArbitrageExchangeSymbol, AskBidValues, ExchangeKind, ExchangeValues, OrderedExchangeSymbol,
    PairedExchangeSymbol, PriceUpdateEvent, SwapOrder, KRAKEN_USD_STABLES, USD_STABLES,
};
use log::{error, info};
use ringbuf::{consumer::Consumer, producer::Producer, HeapRb};
use std::{
    collections::{HashMap, HashSet},
    net::TcpStream,
    ops::Deref,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{runtime::Runtime, task::JoinHandle};
use tungstenite::{
    client::IntoClientRequest, connect, error::Result as WsResult, protocol::WebSocket,
    stream::MaybeTlsStream, Message,
};

struct WsObserverClientThreadData {
    /// Producer of the SPSC queue
    pub rx: Producer<
        PriceUpdateEvent<ArbitrageExchangeSymbol>,
        Arc<HeapRb<PriceUpdateEvent<ArbitrageExchangeSymbol>>>,
    >,
    /// URL to the ws server
    pub ws_uri: String,
    #[allow(dead_code)]
    /// Exchange kind
    pub exchange: ExchangeKind,
}

impl WsObserverClientThreadData {
    pub fn new(
        rx: Producer<
            PriceUpdateEvent<ArbitrageExchangeSymbol>,
            Arc<HeapRb<PriceUpdateEvent<ArbitrageExchangeSymbol>>>,
        >,
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
    /// Internal map that stores necessary info for each exchange
    thread_data_vec: Vec<WsObserverClientThreadData>,

    /// Must be used by the used of WsRemoteObserverClient to get consumer of the queues.
    /// It fills from the config in the constructor and must be consumed by the higher
    /// application, for example, by using `HashMap::drain`.
    pub rx_map: HashMap<
        ExchangeKind,
        Consumer<
            PriceUpdateEvent<ArbitrageExchangeSymbol>,
            Arc<HeapRb<PriceUpdateEvent<ArbitrageExchangeSymbol>>>,
        >,
    >,

    /// Configs for individual observers
    pub obs_config: ObserverConfig,

    /// Websocket config
    pub ws_config: WsConfig,

    jobs: Vec<JoinHandle<()>>,

    runtime: Arc<Runtime>,
}

impl WsRemoteObserverClient {
    pub fn new(runtime: Arc<Runtime>, obs_config: ObserverConfig, ws_config: WsConfig) -> Self {
        let mut thread_data_vec = Vec::new();
        let mut rx_map = HashMap::new();

        // Create SPSC queues for each exchange that is enabled in the config.
        // Add binance
        if let Some(config) = &obs_config.binance {
            if config.enable {
                let (tx, rx) =
                    HeapRb::<PriceUpdateEvent<ArbitrageExchangeSymbol>>::new(4096).split();
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
                let (tx, rx) =
                    HeapRb::<PriceUpdateEvent<ArbitrageExchangeSymbol>>::new(4096).split();
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
                let (tx, rx) =
                    HeapRb::<PriceUpdateEvent<ArbitrageExchangeSymbol>>::new(4096).split();
                let thread_data = WsObserverClientThreadData::new(
                    tx,
                    format!("ws://{}:{}", ws_config.host, config.ws_port),
                    ExchangeKind::Kraken,
                );

                thread_data_vec.push(thread_data);
                rx_map.insert(ExchangeKind::Kraken, rx);
            }
        }

        Self {
            thread_data_vec: thread_data_vec,
            rx_map: rx_map,
            obs_config: obs_config,
            ws_config: ws_config,
            jobs: vec![],
            runtime: runtime,
        }
    }

    fn get_working_connection<Req: IntoClientRequest>(
        req: Req,
    ) -> WsResult<WebSocket<MaybeTlsStream<TcpStream>>> {
        let (ws_stream, _) = connect(req)?;
        Ok(ws_stream)
    }

    fn get_connection_with_reconnect_on_failure(
        req: &String,
    ) -> WebSocket<MaybeTlsStream<TcpStream>> {
        loop {
            // Try to establish connection
            if let Ok(ws_stream) = Self::get_working_connection(req) {
                // If connection is established, return it
                info!("Connected to the ws server: {}", req);
                break ws_stream;
            } else {
                // Log error on failure, sleep for 5 seconds and try again
                error!("Failed to connect to the ws server: {}, trying to reconnect with 5 seconds interval", req);
                // Sleep for 5 seconds
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
        }
    }

    fn spawn_ws_handler(
        runtime: Arc<Runtime>,
        mut thread_data: WsObserverClientThreadData,
    ) -> JoinHandle<()> {
        runtime.spawn_blocking(move || {

            // Firstly try to initialize the ws_stream.
            let mut ws_stream = Self::get_connection_with_reconnect_on_failure(&thread_data.ws_uri);

            loop {
                let res = if let Ok(res) = ws_stream.read() {
                    res
                } else {
                    // If we catch error on reading from the ws_stream, try to reconnect
                    error!("Failed to read from the ws stream, trying to reconnect with 5 seconds interval");

                    ws_stream = Self::get_connection_with_reconnect_on_failure(&thread_data.ws_uri);
                    continue;
                };

                match &res {
                    Message::Text(text) => {
                        // Deserialize the message and push it into the queue
                        // let event: PriceUpdateEvent<ArbitrageExchangeSymbol> = serde_json::from_str(text);
                        if let Ok(event) = serde_json::from_str::<PriceUpdateEvent<ArbitrageExchangeSymbol>>(text) {
                            if !thread_data.rx.is_full() {
                                thread_data
                                    .rx
                                    .push(event)
                                    .expect("Failed to push event into the queue");
                            }
                        } else if let Ok(mut event) = serde_json::from_str::<Vec<PriceUpdateEvent<ArbitrageExchangeSymbol>>>(text) {
                            for e in event.drain(0..) {
                                if !thread_data.rx.is_full() {
                                    thread_data
                                        .rx
                                        .push(e.clone())
                                        .expect("Failed to push event into the queue");
                                }
                            }
                        } else {
                            error!("Failed to deserialize the message: {}", text);
                        }
                    },
                    _ => {}
                }
            }
        })
    }

    // Start the main WS client application loop.
    pub fn start(&mut self) {
        for thread_data in self.thread_data_vec.drain(0..) {
            self.jobs
                .push(Self::spawn_ws_handler(self.runtime.clone(), thread_data));
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
struct RemoteObserverDriver {
    /// `price_table` - Represents the main storage for prices, as well as accessing the
    /// prices in the storage. Here key is the pair name (e.g. ethusdt), not the single
    /// token like in `connected_symbols`. It is made this way to be able to index this
    /// map from already concatenated pair names, when turning concatenated string into
    /// ArbitrageExchangeSymbol is impossible.
    price_table: Arc<DashMap<ArbitrageExchangeSymbol, Arc<Mutex<AskBidValues>>>>,

    /// `connected_symbols` - Connected symbols represent all existing pools on certain token,
    /// hence here `key` is single token (e.g. eth, btc), not pair (e.g. ethusdt).
    pub connected_symbols:
        Arc<DashMap<String, HashSet<OrderedExchangeSymbol<ArbitrageExchangeSymbol>>>>,

    watching_symbols: Arc<DashSet<ArbitrageExchangeSymbol>>,

    /// Running job handle
    job: Option<JoinHandle<()>>,

    runtime: Arc<Runtime>,

    /// Exchange kind of this driver.
    exchange: ExchangeKind,

    /// Internal consumer of events emitted by the WS client.
    rx: Option<
        Consumer<
            PriceUpdateEvent<ArbitrageExchangeSymbol>,
            Arc<HeapRb<PriceUpdateEvent<ArbitrageExchangeSymbol>>>,
        >,
    >,
}

impl RemoteObserverDriver {
    #[allow(dead_code)]
    pub fn new(
        rx: Consumer<
            PriceUpdateEvent<ArbitrageExchangeSymbol>,
            Arc<HeapRb<PriceUpdateEvent<ArbitrageExchangeSymbol>>>,
        >,
        exchange: ExchangeKind,
        runtime: Arc<Runtime>,
    ) -> Self {
        Self {
            price_table: Arc::new(DashMap::new()),
            connected_symbols: Arc::new(DashMap::new()),
            job: None,
            runtime: runtime,
            exchange: exchange,
            watching_symbols: Arc::new(DashSet::new()),
            rx: Some(rx), // Save rx for use with `start`
        }
    }

    #[allow(dead_code)]
    pub fn start(&mut self) {
        // Take rx from previously saved in `new` method
        let rx = self
            .rx
            .take()
            .expect("Invalid call of start method, RemoveObserverDriver::rx is None");
        // Spawn price update task
        let job = Self::spawn_listen_task(
            rx,
            self.price_table.clone(),
            self.connected_symbols.clone(),
            self.watching_symbols.clone(),
            self.runtime.clone(),
        );

        self.job = Some(job);
    }

    pub fn new_instant_start(
        rx: Consumer<
            PriceUpdateEvent<ArbitrageExchangeSymbol>,
            Arc<HeapRb<PriceUpdateEvent<ArbitrageExchangeSymbol>>>,
        >,
        exchange: ExchangeKind,
        runtime: Arc<Runtime>,
    ) -> Self {
        let price_table = Arc::new(DashMap::new());
        let connected_symbols = Arc::new(DashMap::new());
        let watching_symbols = Arc::new(DashSet::new());

        // Spawn price update task
        let job = Self::spawn_listen_task(
            rx,
            price_table.clone(),
            connected_symbols.clone(),
            watching_symbols.clone(),
            runtime.clone(),
        );

        Self {
            price_table: price_table,
            connected_symbols: connected_symbols,
            watching_symbols: watching_symbols,
            job: Some(job),
            runtime: runtime,
            exchange: exchange,
            rx: None,
        }
    }

    fn spawn_listen_task(
        mut rx: Consumer<
            PriceUpdateEvent<ArbitrageExchangeSymbol>,
            Arc<HeapRb<PriceUpdateEvent<ArbitrageExchangeSymbol>>>,
        >,
        price_table: Arc<DashMap<ArbitrageExchangeSymbol, Arc<Mutex<AskBidValues>>>>,
        connected_symbols: Arc<
            DashMap<String, HashSet<OrderedExchangeSymbol<ArbitrageExchangeSymbol>>>,
        >,
        watching_symbols: Arc<DashSet<ArbitrageExchangeSymbol>>,
        runtime: Arc<Runtime>,
    ) -> JoinHandle<()> {
        runtime.spawn_blocking(move || {
            loop {
                // It expects an empty state of price_table and connection_symbols.
                // If initializes them with required structues on-the-fly.
                if let Some(update) = rx.pop() {
                    let symbol = update.symbol;

                    // If the symbol is already in the price table, simply update the price,
                    // otherwise add it to the price table first.
                    if let Some(price) = price_table.get(&symbol) {
                        *price.lock().expect("Failed to get mutex") = update.price;
                    } else {
                        price_table.insert(symbol.clone(), Arc::new(Mutex::new(update.price)));
                    }

                    // The symbol could be added to the connected_symbols
                    if !connected_symbols.contains_key(symbol.base()) {
                        connected_symbols.insert(symbol.base().to_string().clone(), HashSet::new());
                    }

                    if !connected_symbols.contains_key(symbol.quote()) {
                        connected_symbols
                            .insert(symbol.quote().to_string().clone(), HashSet::new());
                    }

                    // If symbols hasn't appeared already, add it to the connected_symbols
                    connected_symbols
                        .get_mut(symbol.base())
                        .expect("INTERNAL ERROR: Base symbol wasn't found")
                        .insert(OrderedExchangeSymbol::new(&symbol, SwapOrder::Sell));
                    connected_symbols
                        .get_mut(symbol.quote())
                        .expect("INTERNAL ERROR: Quote symbol wasn't found")
                        .insert(OrderedExchangeSymbol::new(&symbol, SwapOrder::Buy));

                    // Add symbol to the watching_symbols
                    watching_symbols.insert(symbol.clone());
                } else {
                    // Wait for 50ms if there is no new event
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        })
    }

    /// Function that returns the current watching_symbols - symbols that we have the price,
    /// there fore it gets updates from in the observer
    pub fn get_watching_symbols(&self) -> Arc<DashSet<ArbitrageExchangeSymbol>> {
        self.watching_symbols.clone()
    }

    /// Get all pools in which this symbol appears, very useful for most strategies
    pub fn get_interchanged_symbols(
        &self,
        symbol: &String,
    ) -> OResult<Ref<String, HashSet<OrderedExchangeSymbol<ArbitrageExchangeSymbol>>>> {
        if let Some(symbols) = self.connected_symbols.get(symbol) {
            return Ok(symbols);
        }

        Err(ObserverError::SymbolNotFound(self.exchange.clone(), symbol.clone()).into())
    }

    /// Fetches price on certain symbol from the observer
    pub fn get_price_from_table(
        &self,
        symbol: &ArbitrageExchangeSymbol,
    ) -> OResult<Arc<Mutex<AskBidValues>>> {
        if let Some(price) = self.price_table.get(&symbol) {
            return Ok(price.value().clone());
        }

        Err(ObserverError::SymbolNotFound(self.exchange.clone(), symbol.to_string()).into())
    }

    fn get_usd_stables(&self) -> impl Iterator<Item = &str> {
        match self.exchange {
            // For kraken, stables are defined in KRAKEN_USD_STABLES
            ExchangeKind::Kraken => KRAKEN_USD_STABLES.into_iter(),
            // For binance and huobi, stables are defined in USD_STABLED
            ExchangeKind::Binance | ExchangeKind::Huobi => USD_STABLES.into_iter(),
            _ => {
                unimplemented!("INTERNAL ERROR: Exchange not supported")
            }
        }
        // USD_STABLES.into_iter()
    }

    /// Returns value of certain token to usd if available
    pub fn get_usd_value(&self, sym: &String) -> OResult<f64> {
        // TODO: This lock USD wrapped tokens to 1 seems to be unnecessary,
        // considering to remove this later
        if let Some(_) = self.get_usd_stables().find(|v| v == sym) {
            return Ok(1.0);
        };

        let connected = self.get_interchanged_symbols(sym)?;

        for ordered_sym in connected.iter() {
            for stable in self.get_usd_stables() {
                if <&str as Into<String>>::into(stable) == ordered_sym.symbol.base()
                    || <&str as Into<String>>::into(stable) == ordered_sym.symbol.quote()
                {
                    return self.get_price_from_table(&ordered_sym.symbol).map(|v| {
                        let unlocked = v.lock().expect("Failed to get mutex lock for price");
                        (unlocked.get_ask_price() + unlocked.get_bid_price()) / 2.0
                    });
                }
            }
        }

        Err(ObserverError::SymbolNotFound(self.exchange.clone(), sym.clone()).into())
    }

    /// Function to dump the existing prices into a newly created HashMap.
    pub fn dump_price_table(&self) -> HashMap<ArbitrageExchangeSymbol, AskBidValues> {
        let mut price_table: HashMap<ArbitrageExchangeSymbol, AskBidValues> =
            HashMap::with_capacity(self.price_table.len());

        for element in self.price_table.iter() {
            let value = *element
                .value()
                .lock()
                .expect("Failed to receive Mutex")
                .deref();
            let symbol = element.key().clone();

            price_table.insert(symbol, value);
        }

        price_table
    }
}

/// Main structure to view realtime prices on remote observer. It receives Websocket price
/// updates from the remote WS observer server and stores them in the `price_table` map structure.
pub struct WsRemoteObserver {
    /// Local websocket client to the exch_observer WS server.
    ws_client: WsRemoteObserverClient,

    /// Mapping of exchange kind to its driver instance. Each driver connects to single WS port.
    driver_map: HashMap<ExchangeKind, RemoteObserverDriver>,

    runtime: Arc<Runtime>,
}

impl WsRemoteObserver {
    pub fn new(runtime: Arc<Runtime>, obs_config: ObserverConfig, ws_config: WsConfig) -> Self {
        let ws_client =
            WsRemoteObserverClient::new(runtime.clone(), obs_config.clone(), ws_config.clone());

        Self {
            ws_client: ws_client,
            driver_map: HashMap::new(),
            runtime: runtime,
        }
    }

    fn start_listener_threads(&mut self) {
        for (exchange, rx) in self.ws_client.rx_map.drain() {
            info!("Starting rx listener thread for {}", exchange.to_string());
            let driver =
                RemoteObserverDriver::new_instant_start(rx, exchange.clone(), self.runtime.clone());
            self.driver_map.insert(exchange, driver);
        }
    }

    pub fn start(&mut self) {
        // Start listener threads
        self.start_listener_threads();

        // Start inner WS client
        self.ws_client.start();
    }

    /// Returns the array of enabled exchanges
    pub fn get_supported_exchanges(&self) -> Vec<ExchangeKind> {
        let mut exchanges = Vec::new();
        for (exchange, _) in self.driver_map.iter() {
            exchanges.push(exchange.clone());
        }

        exchanges
    }

    /// Get all pools in which this symbol appears, very useful for most strategies
    pub fn get_interchanged_symbols(
        &self,
        exchange: ExchangeKind,
        symbol: &String,
    ) -> OResult<Ref<String, HashSet<OrderedExchangeSymbol<ArbitrageExchangeSymbol>>>> {
        if let Some(driver) = self.driver_map.get(&exchange) {
            return driver.get_interchanged_symbols(symbol);
        }

        Err(ObserverError::ExchangeNotFound(exchange).into())
    }

    /// Fetches price on certain symbol from the observer
    pub fn get_price(
        &self,
        exchange: ExchangeKind,
        symbol: &ArbitrageExchangeSymbol,
    ) -> OResult<Arc<Mutex<AskBidValues>>> {
        if let Some(driver) = self.driver_map.get(&exchange) {
            return driver.get_price_from_table(symbol);
        }

        Err(ObserverError::ExchangeNotFound(exchange).into())
    }

    /// Returns value of certain token to usd if available
    pub fn get_usd_value(&self, exchange: ExchangeKind, sym: &String) -> OResult<f64> {
        if let Some(driver) = self.driver_map.get(&exchange) {
            return driver.get_usd_value(sym);
        }

        Err(ObserverError::ExchangeNotFound(exchange).into())
    }

    /// Function to dump the existing prices into a newly created HashMap.
    pub fn dump_price_table(
        &self,
        exchange: ExchangeKind,
    ) -> OResult<HashMap<ArbitrageExchangeSymbol, AskBidValues>> {
        if let Some(driver) = self.driver_map.get(&exchange) {
            return Ok(driver.dump_price_table());
        }

        Err(ObserverError::ExchangeNotFound(exchange).into())
    }

    pub fn get_watching_symbols(
        &self,
        exchange: ExchangeKind,
    ) -> OResult<Arc<DashSet<ArbitrageExchangeSymbol>>> {
        if let Some(driver) = self.driver_map.get(&exchange) {
            return Ok(driver.get_watching_symbols());
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
    fn set_tx_fifo(&mut self, tx: mpsc::Sender<PriceUpdateEvent<ArbitrageExchangeSymbol>>);

    /// Function to dump the existing prices into a newly created HashMap. Pretty expensive
    /// function to call.
    fn dump_price_table(&self) -> HashMap<Symbol, Self::Values>;
}
*/
