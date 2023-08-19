use dashmap::DashMap;
use exch_observer_types::{
    ExchangeObserver, ExchangeValues, ObserverWorkerThreadData, OrderedExchangeSymbol,
    PairedExchangeSymbol, PriceUpdateEvent, SwapOrder, USD_STABLES,
};
use log::debug;
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    marker::PhantomData,
    ops::Deref,
    sync::{mpsc, Arc, Mutex},
};
use tokio::runtime::Runtime;

pub struct MulticonObserverDriver<Symbol, Impl>
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
    Impl: ExchangeObserver<Symbol>,
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
    price_table: Arc<DashMap<String, Arc<Mutex<Impl::Values>>>>,
    /// Internal mapping of symbol string representation to the symbol itself.
    str_symbol_mapping: Arc<DashMap<String, Symbol>>,
    async_runner: Arc<Runtime>,

    /// Necessary for getting control of the threads execution from a function.
    /// One example of such usage might be killing the thread with multiple symbols
    /// when `remove_symbol` was called on every symbol in this thread.
    threads_data_mapping: HashMap<Symbol, Arc<Mutex<ObserverWorkerThreadData<Symbol>>>>,
    /// Vector of unique threads that are currently running.
    threads_data_vec: Vec<Arc<Mutex<ObserverWorkerThreadData<Symbol>>>>,
    /// Symbols in the queue to be added to the new thread, which is created when
    /// `symbols_queue_limit` is reached
    symbols_in_queue: Vec<Symbol>,
    /// The maximum number of symbols that can be in the queue at any given time
    symbols_queue_limit: usize,

    marker: PhantomData<Impl>,

    /// Blocking callback that creates WS connections and updates the price table
    spawn_callback: Arc<
        dyn Fn(
                &Vec<Symbol>,
                Arc<DashMap<String, Arc<Mutex<Impl::Values>>>>,
                Arc<DashMap<String, Symbol>>,
                Arc<Mutex<ObserverWorkerThreadData<Symbol>>>,
            ) + Send
            + Sync
            + 'static,
    >,
}

impl<Symbol, Impl> MulticonObserverDriver<Symbol, Impl>
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
    Impl: ExchangeObserver<Symbol>,
    Impl::Values: Send + Copy + 'static,
{
    pub fn new<F>(async_runner: Arc<Runtime>, symbols_queue_limit: usize, spawn_callback: F) -> Self
    where
        F: Fn(
                &Vec<Symbol>,
                Arc<DashMap<String, Arc<Mutex<Impl::Values>>>>,
                Arc<DashMap<String, Symbol>>,
                Arc<Mutex<ObserverWorkerThreadData<Symbol>>>,
            ) + Send
            + Sync
            + 'static,
    {
        Self {
            watching_symbols: vec![],
            connected_symbols: HashMap::new(),
            price_table: Arc::new(DashMap::new()),
            str_symbol_mapping: Arc::new(DashMap::new()),
            async_runner: async_runner,

            threads_data_mapping: HashMap::new(),
            threads_data_vec: vec![],
            symbols_in_queue: vec![],
            symbols_queue_limit: symbols_queue_limit,
            marker: PhantomData,
            spawn_callback: Arc::new(spawn_callback),
        }
    }

    /// Spawns a new thread for the symbols in the queue. It clear the queue after spawning a
    /// thread. It also doesn't perform checks for queue limit, simply spawns the thread for
    /// the current queue.
    fn spawn_tasks_for_queue_symbols(&mut self) {
        if self.symbols_in_queue.len() > 0 {
            let thread_data = Arc::new(Mutex::new(ObserverWorkerThreadData::from(
                &self.symbols_in_queue,
            )));

            for sym in &self.symbols_in_queue {
                self.threads_data_mapping
                    .insert(sym.clone(), thread_data.clone());
            }
            self.threads_data_vec.push(thread_data.clone());

            let spawn_callback = self.spawn_callback.clone();
            let symbols_in_queue = self.symbols_in_queue.clone();
            let price_table = self.price_table.clone();
            let str_symbol_mapping = self.str_symbol_mapping.clone();

            // Spawn a new thread for the symbols in the queue
            self.async_runner.clone().spawn_blocking(move || {
                // Run blocking callback defined in end implementation
                (spawn_callback)(
                    &symbols_in_queue,
                    price_table,
                    str_symbol_mapping,
                    thread_data,
                );
            });

            // Clear symbols queue
            self.symbols_in_queue.clear();
        }
    }

    pub fn get_interchanged_symbols(
        &self,
        symbol: &String,
    ) -> &'_ Vec<OrderedExchangeSymbol<Symbol>> {
        &self
            .connected_symbols
            .get::<String>(symbol)
            .expect("Symbol wasn't found")
    }

    pub fn add_price_to_monitor(&mut self, symbol: &Symbol, price: Arc<Mutex<Impl::Values>>) {
        let _symbol = <Symbol as Into<String>>::into(symbol.clone());
        if !self.price_table.contains_key(&_symbol) {
            self.price_table.insert(_symbol.clone(), price);

            // The symbol could be added to the connected_symbolsk
            if !self.connected_symbols.contains_key(symbol.base()) {
                self.connected_symbols
                    .insert(symbol.base().to_string().clone(), Vec::new());
            }

            if !self.connected_symbols.contains_key(symbol.quote()) {
                self.connected_symbols
                    .insert(symbol.quote().to_string().clone(), Vec::new());
            }

            // Insert OrderedExchangeSymbol into the connected_symbols map for both
            // base and quote tokens
            self.connected_symbols
                .get_mut(symbol.base())
                .expect("INTERNAL ERROR: Base symbol wasn't found")
                .push(OrderedExchangeSymbol::new(&symbol, SwapOrder::Sell));
            self.connected_symbols
                .get_mut(symbol.quote())
                .expect("INTERNAL ERROR: Quote symbol wasn't found")
                .push(OrderedExchangeSymbol::new(&symbol, SwapOrder::Buy));

            // Add symbol to `str_symbol_mapping`
            self.str_symbol_mapping
                .insert(_symbol.clone(), symbol.clone());

            // Insert symbol into the symbol queue
            self.symbols_in_queue.push(symbol.clone());

            // If the queue limit is reached, spawn a new thread for the symbols in the queue
            if self.symbols_in_queue.len() >= self.symbols_queue_limit {
                self.spawn_tasks_for_queue_symbols();
            }

            // Add new symbol to watching symbols
            self.watching_symbols.push(symbol.clone())
        }
    }

    pub fn get_price_from_table(&self, symbol: &Symbol) -> Option<Arc<Mutex<Impl::Values>>> {
        let symbol = <Symbol as Into<String>>::into(symbol.clone());

        if let Some(inner) = self.price_table.get(&symbol) {
            return Some(inner.value().clone());
        }

        None
    }

    pub fn get_usd_value(&self, sym: &String) -> Option<f64> {
        // TODO: This lock USD wrapped tokens to 1 seems to be unnecessary,
        // considering to remove this later
        if let Some(_) = USD_STABLES.into_iter().find(|v| v == sym) {
            return Some(1.0);
        };

        let connected = self.get_interchanged_symbols(sym);

        for ordered_sym in connected.iter() {
            for stable in &USD_STABLES {
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

        None
    }

    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.symbols_in_queue.len() > 0 {
            self.spawn_tasks_for_queue_symbols();
        }

        Ok(())
    }

    pub fn remove_symbol(&mut self, symbol: Symbol) {
        // Remove symbol from `watching_symbols`, `connected_symbols` from both base and quote
        // symbols and `price_table`, also vote for this symbol's thread to stop (won't stop
        // untill all symbols related to this thread vote to stop it).

        let mut found = false;

        // Remove from the `watching_symbols`
        for (i, sym) in self.watching_symbols.iter().enumerate() {
            if sym == &symbol {
                self.watching_symbols.remove(i);
                found = true;
                break;
            }
        }

        // If symbol is not loaded in the driver, simply return
        if !found {
            debug!("`remove_symbol` failed silently, since symbol wans't found in the driver");
            return;
        }

        // Remove from maptrees.
        let base_connected = self
            .connected_symbols
            .get_mut(symbol.base())
            .expect("Failed to get base symbol");
        for (i, sym) in base_connected.iter().enumerate() {
            if sym.symbol == symbol {
                base_connected.remove(i);
                break;
            }
        }

        let quote_connected = self
            .connected_symbols
            .get_mut(symbol.quote())
            .expect("Failed to get quote symbol");
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
            let mut data = self
                .threads_data_mapping
                .get_mut(&symbol)
                .expect("Failed to get thread data for symbol")
                .lock()
                .expect("Failed to lock thread data");

            // Check that the symbol is not already marked to be removed.
            if !data
                .requests_to_stop_map
                .get(&symbol)
                .expect("Failed to get symbol `requests_to_stop_map`")
            {
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
                    // Remove symbol from `price_table`.
                    self.price_table.remove(&symbol.clone().into());
                }
            }
        }

        // Remove symbol from `threads_data_mapping`
        self.threads_data_mapping.remove(&symbol);
    }

    pub fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        return &self.watching_symbols;
    }

    /// Sets tx price-update fifo for all running threads
    pub fn set_tx_fifo(&mut self, tx: mpsc::Sender<PriceUpdateEvent>) {
        // Set tx fifo for all running threads
        for thread_data in &self.threads_data_vec {
            thread_data.lock().unwrap().set_tx_fifo(tx.clone());
        }
    }

    /// Function to dump the existing prices into a newly created HashMap.
    pub fn dump_price_table(&self) -> HashMap<Symbol, Impl::Values> {
        let mut price_table: HashMap<Symbol, Impl::Values> =
            HashMap::with_capacity(self.price_table.len());

        for element in self.price_table.iter() {
            let value = *element
                .value()
                .lock()
                .expect("Failed to receive Mutex")
                .deref();
            let key = element.key().clone();
            let symbol = self
                .str_symbol_mapping
                .get(&key)
                .unwrap();

            price_table.insert(symbol.clone(), value);
        }

        price_table
    }
}
