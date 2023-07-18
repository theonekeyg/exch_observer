use crate::{BinanceObserver, HuobiObserver, KrakenObserver};
use binance::model::Symbol as BSymbol;
use csv::{Reader, StringRecord};
use exch_observer_config::ObserverConfig;
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ExchangeObserverKind, ExchangeValues, OrderedExchangeSymbol,
    PairedExchangeSymbol,
};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    io,
    sync::{Arc, Mutex},
};
use tokio::runtime::Runtime;

/// Main struct for the observer, contains all the observers, using this
/// as main observer for bots & other services is recommended and not
/// using regular observers by themselves.
pub struct CombinedObserver<Symbol>
where
    Symbol: Eq
        + Hash
        + Clone
        + Display
        + Debug
        + Into<String>
        + PairedExchangeSymbol
        + Into<String>
        + From<BSymbol>
        + Send
        + Sync
        + 'static,
{
    pub observers:
        HashMap<ExchangeObserverKind, Box<dyn ExchangeObserver<Symbol, Values = AskBidValues>>>,
    // TODO: Rewrite `clients` back to dynamic hashmap like `observers`,
    // turns out we can use `downcast_mut` to get the correct inner type
    // for each client, so struct functions will be available
    pub is_running: bool,
    pub config: ObserverConfig,
    pub runtime: Option<Arc<Runtime>>,
}

impl<Symbol> CombinedObserver<Symbol>
where
    Symbol: Eq
        + Hash
        + Clone
        + Display
        + Debug
        + Into<String>
        + PairedExchangeSymbol
        + Send
        + From<BSymbol>
        + Sync
        + 'static,
{
    pub fn new(config: ObserverConfig) -> Self {
        let rv = Self {
            observers: HashMap::new(),
            is_running: false,
            config: config,
            runtime: None,
        };

        rv
    }

    /// Creates new CombinedObserver object with provided tokio runtime.
    pub fn new_with_runtime(config: ObserverConfig, async_runtime: Arc<Runtime>) -> Self {
        Self {
            observers: HashMap::new(),
            is_running: false,
            config: config,
            runtime: Some(async_runtime),
        }
    }

    /// Sets the tokio runtime for the observer.
    pub fn set_runtime(&mut self, runtime: Arc<Runtime>) {
        self.runtime = Some(runtime);
    }

    /// Creates observers for each exchange in the config, must be called before `load_symbols`.
    pub fn create_observers(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Make sure we have a valid tokio runtime
        let runtime = if let Some(runtime) = &self.runtime {
            runtime.clone()
        } else {
            return Err(Box::new(io::Error::new(
                io::ErrorKind::Other,
                "No runtime set",
            )));
        };

        // Create observers based on the provided config
        if let Some(conf) = &self.config.binance {
            if conf.enable {
                let binance_observer = BinanceObserver::new(runtime.clone());
                self.observers
                    .insert(ExchangeObserverKind::Binance, Box::new(binance_observer));
            }
        }

        if let Some(conf) = &self.config.huobi {
            if conf.enable {
                let huobi_observer = HuobiObserver::new(runtime.clone());
                self.observers
                    .insert(ExchangeObserverKind::Huobi, Box::new(huobi_observer));
            }
        }

        if let Some(conf) = &self.config.kraken {
            if conf.enable {
                let kraken_observer = KrakenObserver::new(runtime.clone());
                self.observers
                    .insert(ExchangeObserverKind::Kraken, Box::new(kraken_observer));
            }
        }

        Ok(())
    }

    /// Loads symbols in the observer from the config, must be called after `create_observers` and
    /// before `launch`.
    pub fn load_symbols(&mut self, f: impl Fn(&StringRecord) -> Option<Symbol>) {
        // Load symbols from csv file, pass them to provided closure,
        // and add them to each observer we had in the config.

        if let Some(binance_config) = &self.config.binance {
            if let Some(observer) = self.observers.get_mut(&ExchangeObserverKind::Binance) {
                // let mut observer = observer.downcast_mut::<BinanceObserver<Symbol>>().unwrap();
                // observer.load_symbols_from_csv(f);
                let mut rdr = Reader::from_path(&binance_config.symbols_path).unwrap();
                for result in rdr.records() {
                    let result = result.unwrap();

                    let symbol = f(&result);
                    if symbol.is_none() {
                        continue;
                    }

                    let symbol = symbol.unwrap();
                    observer
                        .add_price_to_monitor(&symbol, Arc::new(Mutex::new(AskBidValues::new())));
                }
            }
        }

        if let Some(huobi_config) = &self.config.huobi {
            if let Some(observer) = self.observers.get_mut(&ExchangeObserverKind::Huobi) {
                let mut rdr = Reader::from_path(&huobi_config.symbols_path).unwrap();
                for result in rdr.records() {
                    let result = result.unwrap();

                    let symbol = f(&result);
                    if symbol.is_none() {
                        continue;
                    }

                    let symbol = symbol.unwrap();
                    observer
                        .add_price_to_monitor(&symbol, Arc::new(Mutex::new(AskBidValues::new())));
                }
            }
        }

        if let Some(kraken_config) = &self.config.kraken {
            if let Some(observer) = self.observers.get_mut(&ExchangeObserverKind::Kraken) {
                let mut rdr = Reader::from_path(&kraken_config.symbols_path).unwrap();
                for result in rdr.records() {
                    let result = result.unwrap();

                    let symbol = f(&result);
                    if symbol.is_none() {
                        continue;
                    }

                    let symbol = symbol.unwrap();
                    observer
                        .add_price_to_monitor(&symbol, Arc::new(Mutex::new(AskBidValues::new())));
                }
            }
        }
    }

    /// Removes all symbols from the specified observer that are uninitialized at this moment.
    /// Useful when using exch_observer with trade bot locally, since uninitialized symbols might
    /// introduce various race-condition issues as well as intruduce potential bugs in
    /// calculations.
    pub fn clear_uninitialized_symbols(&mut self, kind: ExchangeObserverKind) {
        if let Some(observer) = self.observers.get_mut(&kind) {
            let mut uninitialized_symbols = Vec::new();

            // Fill uninitialized symbols vector
            for symbol in observer.get_watching_symbols().clone() {
                let price = observer.get_price_from_table(&symbol).unwrap();
                if !price.lock().unwrap().is_initialized() {
                    uninitialized_symbols.push(symbol.clone());
                }
            }

            // Remove all uninitialized symbols
            for symbol in uninitialized_symbols {
                observer.remove_symbol(symbol);
            }
        }
    }

    /// Main function to launch the built observer, must be called after `create_observers` and
    /// `load_symbols`.
    pub fn launch(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.is_running {
            return Ok(());
        }

        // Start each observer.
        if let Some(binance_observer) = self.observers.get_mut(&ExchangeObserverKind::Binance) {
            binance_observer.start()?;
        }

        if let Some(huobi_observer) = self.observers.get_mut(&ExchangeObserverKind::Huobi) {
            huobi_observer.start()?;
        }

        if let Some(kraken_observer) = self.observers.get_mut(&ExchangeObserverKind::Kraken) {
            kraken_observer.start()?;
        }

        self.is_running = true;
        Ok(())
    }

    /// Returns the reference to the price stored in the observer.
    ///
    /// # Arguments
    /// * `kind` - The kind of the observer to get the prices from.
    /// * `symbol` - The symbol to get the prices for.
    pub fn get_price(
        &self,
        kind: ExchangeObserverKind,
        symbol: &Symbol,
    ) -> Option<&Arc<Mutex<AskBidValues>>> {
        if let Some(observer) = self.observers.get(&kind) {
            return observer.get_price_from_table(&symbol);
        }

        None
    }

    /// Removes the symbol from the observer.
    pub fn remove_symbol(&mut self, kind: ExchangeObserverKind, symbol: Symbol) {
        if let Some(observer) = self.observers.get_mut(&kind) {
            observer.remove_symbol(symbol);
        }
    }

    /// Adds the symbol to the observer.
    pub fn add_price_to_monitor(
        &mut self,
        kind: ExchangeObserverKind,
        symbol: &Symbol,
        price: Arc<Mutex<AskBidValues>>,
    ) {
        if let Some(observer) = self.observers.get_mut(&kind) {
            observer.add_price_to_monitor(symbol, price);
        }
    }

    /// Get interchanged symbols for the specified token.
    /// Interchanged means symbols that this token can be exchanged in
    /// (e.g. `eth` token can be exchanged in `ethusdt` `ethbtc` pairs).
    pub fn get_interchanged_symbols(
        &self,
        kind: ExchangeObserverKind,
        symbol: &String,
    ) -> Option<&'_ Vec<OrderedExchangeSymbol<Symbol>>> {
        if let Some(observer) = self.observers.get(&kind) {
            return Some(observer.get_interchanged_symbols(symbol));
        }

        None
    }

    /// Gets the USD value for the specified token.
    /// Beware that this function gets usd value by searching
    /// pairs that this token can be exchanged in (looking for USD stables),
    /// if the token doesn't exist in any known USD stable pairs (in the same observer),
    /// None is returned.
    pub fn get_usd_value(&self, kind: ExchangeObserverKind, symbol: &String) -> Option<f64> {
        if let Some(observer) = self.observers.get(&kind) {
            return observer.get_usd_value(symbol);
        }

        None
    }

    /// Returns the reference to the `watching_symbols` in the observer.
    pub fn get_watching_symbols(&self, kind: ExchangeObserverKind) -> Option<&'_ Vec<Symbol>> {
        if let Some(observer) = self.observers.get(&kind) {
            return Some(observer.get_watching_symbols());
        }

        None
    }
}
