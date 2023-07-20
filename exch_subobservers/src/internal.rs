use exch_observer_types::{
    ExchangeObserver, ExchangeValues, ObserverWorkerThreadData, OrderedExchangeSymbol,
    PairedExchangeSymbol, SwapOrder, USD_STABLES,
};
use log::debug;
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    marker::PhantomData,
    ops::Deref,
    sync::{Arc, Mutex},
};
use tokio::{runtime::Runtime, task::JoinHandle};

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
    price_table: Arc<HashMap<String, Arc<Mutex<Impl::Values>>>>,
    async_runner: Arc<Runtime>,

    /// Necessary for getting control of the threads execution from a function.
    /// One example of such usage might be killing the thread with multiple symbols
    /// when `remove_symbol` was called on every symbol in this thread.
    threads_data_mapping: HashMap<Symbol, Arc<ObserverWorkerThreadData<Symbol>>>,
    running_handles: Vec<(JoinHandle<()>, Arc<ObserverWorkerThreadData<Symbol>>)>,
    /// Symbols in the queue to be added to the new thread, which is created when
    /// `symbols_queue_limit` is reached
    symbols_in_queue: Vec<Symbol>,
    /// The maximum number of symbols that can be in the queue at any given time
    symbols_queue_limit: usize,

    marker: PhantomData<Impl>,

    spawn_callback: Arc<
        dyn Fn(
                &Runtime,
                &Vec<Symbol>,
                Arc<HashMap<String, Arc<Mutex<Impl::Values>>>>,
                Arc<ObserverWorkerThreadData<Symbol>>,
            ) -> JoinHandle<()>
            + Send
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
{
    pub fn new<F>(async_runner: Arc<Runtime>, symbols_queue_limit: usize, spawn_callback: F) -> Self
    where
        F: Fn(
                &Runtime,
                &Vec<Symbol>,
                Arc<HashMap<String, Arc<Mutex<Impl::Values>>>>,
                Arc<ObserverWorkerThreadData<Symbol>>,
            ) -> JoinHandle<()>
            + Send
            + Sync
            + 'static,
    {
        Self {
            watching_symbols: vec![],
            connected_symbols: HashMap::new(),
            price_table: Arc::new(HashMap::new()),
            async_runner: async_runner,

            threads_data_mapping: HashMap::new(),
            running_handles: vec![],
            symbols_in_queue: vec![],
            symbols_queue_limit: symbols_queue_limit,
            marker: PhantomData,
            spawn_callback: Arc::new(spawn_callback),
        }
    }

    fn consume_queue_symbols(&mut self) {
        if self.symbols_in_queue.len() > 0 {
            let thread_data = Arc::new(ObserverWorkerThreadData::from(&self.symbols_in_queue));

            for sym in &self.symbols_in_queue {
                self.threads_data_mapping
                    .insert(sym.clone(), thread_data.clone());
            }

            let handle = (self.spawn_callback)(
                self.async_runner.deref(),
                &self.symbols_in_queue,
                self.price_table.clone(),
                thread_data.clone(),
            );
            self.running_handles.push((handle, thread_data.clone()));

            self.symbols_in_queue.clear();
        }
    }

    pub fn get_interchanged_symbols(
        &self,
        symbol: &String,
    ) -> &'_ Vec<OrderedExchangeSymbol<Symbol>> {
        &self.connected_symbols.get::<String>(symbol).unwrap()
    }

    pub fn add_price_to_monitor(&mut self, symbol: &Symbol, price: Arc<Mutex<Impl::Values>>) {
        let _symbol = <Symbol as Into<String>>::into(symbol.clone());
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
                self.consume_queue_symbols();
            }

            self.watching_symbols.push(symbol.clone())
        }
    }

    pub fn get_price_from_table(&self, symbol: &Symbol) -> Option<&Arc<Mutex<Impl::Values>>> {
        let symbol = <Symbol as Into<String>>::into(symbol.clone());
        self.price_table.get(&symbol)
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
                        let unlocked = v.lock().unwrap();
                        (unlocked.get_ask_price() + unlocked.get_bid_price()) / 2.0
                    });
                }
            }
        }

        None
    }

    pub fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.symbols_in_queue.len() > 0 {
            self.consume_queue_symbols();
        }

        Ok(())
    }

    pub fn remove_symbol(&mut self, symbol: Symbol) {
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
                        ptable_ptr.remove(&symbol.clone().into());
                    }
                }
            }
        }

        self.threads_data_mapping.remove(&symbol);
        debug!("Removed symbol {} from Binace observer", symbol);
    }

    pub fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        return &self.watching_symbols;
    }
}
