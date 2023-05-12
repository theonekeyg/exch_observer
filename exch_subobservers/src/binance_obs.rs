use binance::{
    errors::Result as BResult,
    websockets::{WebSockets, WebsocketEvent},
};
// use csv::{Reader, StringRecord};
use log::{debug, info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    ops::Deref,
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
    vec::Vec,
};
use tokio::runtime::Runtime;

use exch_clients::BinanceClient;
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ExchangeValues, ObserverWorkerThreadData,
    OrderedExchangeSymbol, PairedExchangeSymbol, SwapOrder,
};

#[allow(unused)]
fn all_ticker_stream() -> &'static str {
    "!ticker@arr"
}

#[allow(unused)]
fn ticker_stream(symbol: &str) -> String {
    format!("{symbol}@ticker")
}

#[allow(unused)]
fn agg_trade_stream(symbol: &str) -> String {
    format!("{symbol}@aggTrade")
}

#[allow(unused)]
fn trade_stream(symbol: &str) -> String {
    format!("{symbol}@trade")
}

#[allow(unused)]
fn kline_stream(symbol: &str, interval: &str) -> String {
    format!("{symbol}@kline_{interval}")
}

#[allow(unused)]
fn book_ticker_stream(symbol: &str) -> String {
    format!("{symbol}@bookTicker")
}

#[allow(unused)]
fn all_book_ticker_stream() -> &'static str {
    "!bookTicker"
}

#[allow(unused)]
fn all_mini_ticker_stream() -> &'static str {
    "!miniTicker@arr"
}

#[allow(unused)]
fn mini_ticker_stream(symbol: &str) -> String {
    format!("{symbol}@miniTicker")
}

#[allow(unused)]
/// # Arguments
///
/// * `symbol`: the market symbol
/// * `levels`: 5, 10 or 20
/// * `update_speed`: 1000 or 100
fn partial_book_depth_stream(symbol: &str, levels: u16, update_speed: u16) -> String {
    format!("{symbol}@depth{levels}@{update_speed}ms")
}

#[allow(unused)]
/// # Arguments
///
/// * `symbol`: the market symbol
/// * `update_speed`: 1000 or 100
fn diff_book_depth_stream(symbol: &str, update_speed: u16) -> String {
    format!("{symbol}@depth@{update_speed}ms")
}

static BINANCE_USD_STABLES: [&str; 4] = ["usdt", "usdc", "busd", "dai"];

pub struct BinanceObserver<Symbol>
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
    pub watching_symbols: Vec<Symbol>,
    /// `connected_symbols` - Connected symbols represent all existing pools on certain token,
    /// hence here `key` is single token (e.g. eth, btc), not pair (e.g. ethusdt).
    pub connected_symbols: HashMap<String, Vec<OrderedExchangeSymbol<Symbol>>>,
    /// `price_table` - Represents the main storage for prices, as well as accessing the
    /// prices in the storage. Here key is the pair name (e.g. ethusdt), not the single
    /// token like in `connected_symbols`. It is made this way to be able to index this
    /// map from already concatenated pair names, when turning concatenated string into
    /// ExchangeSymbol is impossible.
    price_table: Arc<HashMap<String, Arc<Mutex<AskBidValues>>>>,
    #[allow(dead_code)]
    client: Option<Arc<RwLock<BinanceClient<Symbol>>>>,
    async_runner: Arc<Runtime>,

    /// Necessary for getting control of the threads execution from a function.
    /// One example of such usage might be killing the thread with multiple symbols
    /// when `remove_symbol` was called on every symbol in this thread.
    threads_data_mapping: HashMap<Symbol, Arc<Mutex<ObserverWorkerThreadData<Symbol>>>>,
    /// Symbols in the queue to be added to the new thread, which is created when
    /// `symbols_queue_limit` is reached
    symbols_in_queue: Vec<Symbol>,
    /// The maximum number of symbols that can be in the queue at any given time
    symbols_queue_limit: usize,
}

impl<Symbol> BinanceObserver<Symbol>
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
        client: Option<Arc<RwLock<BinanceClient<Symbol>>>>,
        async_runner: Arc<Runtime>,
    ) -> Self {
        Self {
            watching_symbols: vec![],
            connected_symbols: HashMap::new(),
            price_table: Arc::new(HashMap::new()),
            client: client,
            async_runner: async_runner,

            threads_data_mapping: HashMap::new(),
            symbols_in_queue: vec![],
            symbols_queue_limit: 20,
        }
    }

    fn launch_worker_multiple(
        runner: &Runtime,
        symbols: &Vec<Symbol>,
        price_table: Arc<HashMap<String, Arc<Mutex<AskBidValues>>>>,
        thread_data: Arc<Mutex<ObserverWorkerThreadData<Symbol>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("Started another batch of symbols");
        let ws_query_subs = symbols
            .iter()
            .map(|sym| {
                let sym = <Symbol as Into<String>>::into(sym.clone());
                book_ticker_stream(&sym)
            })
            .collect::<Vec<_>>();
        runner.spawn_blocking(move || {
            let mut websock = WebSockets::new(move |event: WebsocketEvent| -> BResult<()> {
                match event {
                    WebsocketEvent::OrderBook(order_book) => {
                        trace!("OrderBook: {:?}", order_book);
                    }
                    WebsocketEvent::BookTicker(book) => {
                        let sym_index = book.symbol.clone().to_ascii_lowercase();
                        let ask_price = f64::from_str(&book.best_ask).unwrap();
                        let bid_price = f64::from_str(&book.best_bid).unwrap();
                        let price = (ask_price + bid_price) / 2.0;

                        let update_value = price_table.get(&sym_index).unwrap();
                        update_value
                            .lock()
                            .unwrap()
                            .update_price((ask_price, bid_price));
                        trace!("[{}] Price: {:?}", book.symbol, price);
                    }
                    WebsocketEvent::Kline(kline) => {
                        let kline = kline.kline;

                        let sym_index = kline.symbol.clone().to_ascii_lowercase();

                        let price_high = f64::from_str(kline.high.as_ref()).unwrap();
                        let price_low = f64::from_str(kline.low.as_ref()).unwrap();
                        let price = (price_high + price_low) / 2.0;

                        let update_value = price_table.get(&sym_index).unwrap();
                        update_value
                            .lock()
                            .unwrap()
                            .update_price((price_high, price_low));
                        trace!("[{}] Price: {:?}", kline.symbol, price);
                    }
                    _ => (),
                }
                Ok(())
            });
            websock.connect_multiple_streams(&ws_query_subs)?;
            websock.event_loop(&thread_data.lock().unwrap().is_running)?;
            BResult::Ok(())
        });

        Ok(())
    }
}

impl<Symbol> ExchangeObserver<Symbol> for BinanceObserver<Symbol>
where
    Symbol: Eq
        + Hash
        + Clone
        + Display
        + Debug
        + Into<String>
        + PairedExchangeSymbol
        + Send
        + Sync
        + 'static,
{
    type Values = AskBidValues;

    fn get_interchanged_symbols(&self, symbol: &String) -> &'_ Vec<OrderedExchangeSymbol<Symbol>> {
        &self.connected_symbols.get::<String>(symbol).unwrap()
    }

    fn add_price_to_monitor(&mut self, symbol: &Symbol, price: &Arc<Mutex<Self::Values>>) {
        let _symbol = <Symbol as Into<String>>::into(symbol.clone());
        if !self.price_table.contains_key(&_symbol) {
            // Since with this design it's impossible to modify external ExchangeValues from
            // thread, we're not required to lock the whole HashMap, since each thread modifies
            // it's own *ExchangeValue.
            //
            // Unfortunately unsafe is required to achieve that in modern Rust, as there are no
            // other options to expose that logic to the compiler.
            unsafe {
                let ptable_ptr = Arc::as_ptr(&self.price_table)
                    as *mut HashMap<String, Arc<Mutex<Self::Values>>>;
                (*ptable_ptr).insert(_symbol.clone(), price.clone());
            }

            if !self.connected_symbols.contains_key(symbol.base()) {
                self.connected_symbols
                    .insert(symbol.base().to_string().clone(), Vec::new());
            }

            if !self.connected_symbols.contains_key(symbol.quote()) {
                self.connected_symbols
                    .insert(symbol.quote().to_string().clone(), Vec::new());
            }

            self.connected_symbols
                .get_mut(symbol.base())
                .unwrap()
                .push(OrderedExchangeSymbol::new(&symbol, SwapOrder::Sell));
            self.connected_symbols
                .get_mut(symbol.quote())
                .unwrap()
                .push(OrderedExchangeSymbol::new(&symbol, SwapOrder::Buy));

            self.symbols_in_queue.push(symbol.clone());

            if self.symbols_in_queue.len() >= self.symbols_queue_limit {
                let thread_data = Arc::new(Mutex::new(ObserverWorkerThreadData::from(
                    &self.symbols_in_queue,
                )));

                for sym in &self.symbols_in_queue {
                    self.threads_data_mapping
                        .insert(sym.clone(), thread_data.clone());
                }

                Self::launch_worker_multiple(
                    self.async_runner.deref(),
                    &self.symbols_in_queue,
                    self.price_table.clone(),
                    thread_data,
                )
                .unwrap();

                self.symbols_in_queue.clear();
            }

            info!("Added {} to the watching symbols", &symbol);
            self.watching_symbols.push(symbol.clone())
        }
    }

    fn get_price_from_table(&self, symbol: &Symbol) -> Option<&Arc<Mutex<Self::Values>>> {
        let symbol = <Symbol as Into<String>>::into(symbol.clone());
        self.price_table.get(&symbol)
    }

    fn get_usd_value(&self, sym: &String) -> Option<f64> {
        // TODO: This lock USD wrapped tokens to 1 seems to be unnecessary,
        // considering to remove this later
        if let Some(_) = BINANCE_USD_STABLES.into_iter().find(|v| v == sym) {
            return Some(1.0);
        };

        let connected = self.get_interchanged_symbols(sym);

        for ordered_sym in connected.iter() {
            for stable in &BINANCE_USD_STABLES {
                if <&str as Into<String>>::into(stable) == ordered_sym.symbol.base()
                    || <&str as Into<String>>::into(stable) == ordered_sym.symbol.quote()
                {
                    return self.get_price_from_table(&ordered_sym.symbol).map(|v| {
                        let unlocked = v.lock().unwrap();
                        (unlocked.get_ask_price() + unlocked.get_bid_price()) / 2.0
                    });
                }
            }
        }

        None
    }

    fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting Binance Observer");

        if self.symbols_in_queue.len() > 0 {
            let thread_data = Arc::new(Mutex::new(ObserverWorkerThreadData::from(
                &self.symbols_in_queue,
            )));

            for sym in &self.symbols_in_queue {
                self.threads_data_mapping
                    .insert(sym.clone(), thread_data.clone());
            }

            Self::launch_worker_multiple(
                self.async_runner.deref(),
                &self.symbols_in_queue,
                self.price_table.clone(),
                thread_data,
            )?;

            self.symbols_in_queue.clear();
        }

        Ok(())
    }

    fn remove_symbol(&mut self, symbol: Symbol) {
        // Remove symbol from `watching_symbols`, `connected_symbols` from both base and quote
        // symbols and `price_table`, also vote for this symbol's thread to stop (won't stop
        // untill all symbols related to this thread vote to stop it).

        // Remove from the `watching_symbols`
        for (i, sym) in self.watching_symbols.iter().enumerate() {
            if sym == &symbol {
                self.watching_symbols.remove(i);
                break;
            }
        }

        // Remove from maptrees.
        let base_connected = self.connected_symbols.get_mut(symbol.base()).unwrap();
        for (i, sym) in base_connected.iter().enumerate() {
            if sym.symbol == symbol {
                base_connected.remove(i);
                break;
            }
        }

        let quote_connected = self.connected_symbols.get_mut(symbol.quote()).unwrap();
        for (i, sym) in quote_connected.iter().enumerate() {
            if sym.symbol == symbol {
                quote_connected.remove(i);
                break;
            }
        }

        // Mark the symbol to be removed from the worker thread, so it's price won't be updated
        // and it shows the thread that one symbol is removed.
        {
            let mut data = self
                .threads_data_mapping
                .get(&symbol)
                .unwrap()
                .lock()
                .unwrap();

            // Check that the symbol is not already marked to be removed.
            if !data.requests_to_stop_map.get(&symbol).unwrap() {
                data.requests_to_stop += 1;
                data.requests_to_stop_map
                    .insert(symbol.clone(), true)
                    .unwrap();
            }

            // If all symbols are removed from the thread, stop it.
            if data.requests_to_stop >= data.length {
                data.stop_thread();
            }
        }

        // Remove from `price_table`
        unsafe {
            let ptable_ptr =
                Arc::as_ptr(&self.price_table) as *mut HashMap<Symbol, Arc<Mutex<Self::Values>>>;
            (*ptable_ptr).remove(&symbol);
        }

        debug!("Removed symbol {} from Binace observer", symbol);
    }

    fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        return &self.watching_symbols;
    }
}
