use crate::internal::{kraken_symbol, ObserverWorkerThreadData};
use exch_apis::{
    common::{Result as WsResult, WebsocketEvent},
    kraken_ws::KrakenWebsocket,
};
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ExchangeValues, OrderedExchangeSymbol, PairedExchangeSymbol,
    SwapOrder,
};
use log::{debug, info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    ops::Deref,
    sync::{Arc, Mutex},
};
use tokio::{runtime::Runtime, task::JoinHandle};

pub static KRAKEN_USD_STABLES: [&str; 4] = ["USDT", "USD", "DAI", "USDC"];

pub struct KrakenObserver<Symbol>
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
    pub connected_symbols: HashMap<String, Vec<OrderedExchangeSymbol<Symbol>>>,
    price_table: Arc<HashMap<String, Arc<Mutex<AskBidValues>>>>,
    // client: Option<Arc<RwLock<BinanceClient<Symbol>>>>,
    async_runner: Arc<Runtime>,

    /// Necessary for getting control of the threads execution from a function.
    /// One example of such usage might be killing the thread with multiple symbols
    /// when `remove_symbol` was called on every symbol in this thread.
    threads_data_mapping: HashMap<Symbol, Arc<ObserverWorkerThreadData<Symbol>>>,
    running_handles: Vec<(
        JoinHandle<WsResult<()>>,
        Arc<ObserverWorkerThreadData<Symbol>>,
    )>,
    /// Symbols in the queue to be added to the new thread, which is created when
    /// `symbols_queue_limit` is reached
    symbols_in_queue: Vec<Symbol>,
    /// The maximum number of symbols that can be in the queue at any given time
    symbols_queue_limit: usize,
}

impl<Symbol> KrakenObserver<Symbol>
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
    pub fn new(async_runner: Arc<Runtime>) -> Self {
        Self {
            watching_symbols: vec![],
            connected_symbols: HashMap::new(),
            price_table: Arc::new(HashMap::new()),
            // client: client,
            async_runner: async_runner,

            threads_data_mapping: HashMap::new(),
            running_handles: vec![],
            symbols_in_queue: vec![],
            symbols_queue_limit: 16,
        }
    }

    fn launch_worker_multiple(
        runner: &Runtime,
        symbols: &Vec<Symbol>,
        price_table: Arc<HashMap<String, Arc<Mutex<AskBidValues>>>>,
        thread_data: Arc<ObserverWorkerThreadData<Symbol>>,
    ) -> JoinHandle<WsResult<()>> {
        info!("Started another batch of symbols");
        let ws_query_subs = symbols
            .iter()
            .map(|sym| {
                format!(
                    "{}/{}",
                    sym.base().to_uppercase(),
                    sym.quote().to_uppercase()
                )
            })
            .collect::<Vec<_>>();
        runner.spawn_blocking(move || {
            let mut websock = KrakenWebsocket::new(move |event: WebsocketEvent| -> WsResult<()> {
                match event {
                    WebsocketEvent::KLineEvent(kline) => {
                        let price_high = kline.high;
                        let price_low = kline.low;
                        let price = (price_high + price_low) / 2.0;
                        let sym_index = kline.sym.clone();
                        let update_value = price_table.get(&sym_index).unwrap();
                        update_value
                            .lock()
                            .unwrap()
                            .update_price((price_high, price_low));
                        trace!("[{}] Price: {:?}", sym_index, price);
                    }
                    WebsocketEvent::BookTickerEvent(book) => {
                        let ask_price = book.best_ask;
                        let bid_price = book.best_bid;
                        let sym_index = book.sym.clone();
                        let update_value = price_table.get(&sym_index).unwrap();
                        update_value
                            .lock()
                            .unwrap()
                            .update_price((ask_price, bid_price));
                        trace!("[{}] Ask: {:?}, Bid: {:?}", sym_index, ask_price, bid_price);
                    }
                }

                WsResult::Ok(())
            });
            websock.connect_multiple_streams(ws_query_subs)?;
            websock.event_loop(&thread_data.is_running)
        })
    }
}

impl<Symbol> ExchangeObserver<Symbol> for KrakenObserver<Symbol>
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

    fn add_price_to_monitor(&mut self, symbol: &Symbol, price: Arc<Mutex<Self::Values>>) {
        let _symbol = kraken_symbol(symbol.clone());
        if !self.price_table.contains_key(&_symbol) {
            // Since with this design it's impossible to modify external ExchangeValues from
            // thread, we're not required to lock the whole HashMap, since each thread modifies
            // it's own *ExchangeValue.
            //
            // Unfortunately unsafe is required to achieve that in modern Rust, as there are no
            // other options to expose that logic to the compiler.
            unsafe {
                let ptable_ptr = Arc::get_mut_unchecked(&mut self.price_table);
                ptable_ptr.insert(_symbol.clone(), price);
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
                let thread_data = Arc::new(ObserverWorkerThreadData::from(&self.symbols_in_queue));

                let handle = Self::launch_worker_multiple(
                    self.async_runner.deref(),
                    &self.symbols_in_queue,
                    self.price_table.clone(),
                    thread_data.clone(),
                );
                self.running_handles.push((handle, thread_data.clone()));

                for sym in &self.symbols_in_queue {
                    self.threads_data_mapping
                        .insert(sym.clone(), thread_data.clone());
                }

                self.symbols_in_queue.clear();
            }

            info!("Added {} to the watching symbols", &_symbol);
            self.watching_symbols.push(symbol.clone())
        }
    }

    fn get_price_from_table(&self, symbol: &Symbol) -> Option<&Arc<Mutex<Self::Values>>> {
        // let symbol = <Symbol as Into<String>>::into(symbol.clone());
        let symbol = kraken_symbol(symbol.clone());
        self.price_table.get(&symbol)
    }

    fn get_usd_value(&self, sym: &String) -> Option<f64> {
        // TODO: This lock USD wrapped tokens to 1 seems to be unnecessary,
        // considering to remove this later
        if let Some(_) = KRAKEN_USD_STABLES.into_iter().find(|v| v == sym) {
            return Some(1.0);
        };

        let connected = self.get_interchanged_symbols(sym);

        for ordered_sym in connected.iter() {
            for stable in &KRAKEN_USD_STABLES {
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
        info!("Starting Kraken Observer");

        if self.symbols_in_queue.len() > 0 {
            let thread_data = Arc::new(ObserverWorkerThreadData::from(&self.symbols_in_queue));

            for sym in &self.symbols_in_queue {
                self.threads_data_mapping
                    .insert(sym.clone(), thread_data.clone());
            }

            let handle = Self::launch_worker_multiple(
                self.async_runner.deref(),
                &self.symbols_in_queue,
                self.price_table.clone(),
                thread_data.clone(),
            );
            self.running_handles.push((handle, thread_data.clone()));

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
            // This unsafe statement is safe because the only fields from the
            // ObserverWorkerThreadData struct other thread uses is AtomicBool. However this way we
            // won't have to deal with locks in this communication between threads. As long as
            // only AtomicBool in this structure is used in subthreads, this is thread-safe.
            unsafe {
                let mut data = self.threads_data_mapping.get_mut(&symbol).unwrap();

                let mut data = Arc::get_mut_unchecked(&mut data);

                // Check that the symbol is not already marked to be removed.
                if !data.requests_to_stop_map.get(&symbol).unwrap() {
                    data.requests_to_stop += 1;
                    data.requests_to_stop_map
                        .insert(symbol.clone(), true)
                        .unwrap();
                }

                // If all symbols are removed from the thread, stop it.
                if data.requests_to_stop >= data.length {
                    // Stop the running thread.
                    data.stop_thread();

                    // Only remove prices when all symbols are removed from the thread.
                    for symbol in data.requests_to_stop_map.keys() {
                        // Remove symbol from `threads_data_mapping`

                        // Remove symbol from `price_table` This code is unsafe, since it gets
                        // mutable data directly from Arc. Although, this unsafe block should be
                        // safe, since we stopped the thread before.
                        let ptable_ptr = Arc::get_mut_unchecked(&mut self.price_table);
                        ptable_ptr.remove(&kraken_symbol(symbol.clone()));
                    }
                }
            }
        }

        self.threads_data_mapping.remove(&symbol);
        debug!("Removed symbol {} from Binace observer", symbol);
    }

    fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        return &self.watching_symbols;
    }
}
