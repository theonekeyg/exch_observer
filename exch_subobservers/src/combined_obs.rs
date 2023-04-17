use crate::BinanceObserver;
use exch_clients::BinanceClient;
use exch_observer_config::ObserverConfig;
use exch_observer_types::{
    ExchangeObserver, ExchangeObserverKind, ExchangeSymbol,
    ExchangeValues, OrderedExchangeSymbol
};
use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex, RwLock},
};
use tokio::runtime::Runtime;

struct ExchangeClientsTuple {
    pub binance_client: Option<Arc<RwLock<BinanceClient>>>,
}

impl ExchangeClientsTuple {
    pub fn new() -> Self {
        Self {
            binance_client: None,
        }
    }

    #[allow(dead_code)]
    pub fn set_binance_client(&mut self, client: Arc<RwLock<BinanceClient>>) {
        self.binance_client = Some(client);
    }
}

pub struct CombinedObserver {
    pub observers: HashMap<ExchangeObserverKind, Box<dyn ExchangeObserver>>,
    clients: ExchangeClientsTuple,
    pub is_running: bool,
    pub config: ObserverConfig,
    pub runtime: Option<Arc<Runtime>>,
}

impl CombinedObserver {
    pub fn new(config: ObserverConfig) -> Self {
        let rv = Self {
            observers: HashMap::new(),
            clients: ExchangeClientsTuple::new(),
            is_running: false,
            config: config,
            runtime: None,
        };

        // rv.create_clients();

        rv
    }

    pub fn new_with_runtime(config: ObserverConfig, async_runtime: Arc<Runtime>) -> Self {
        Self {
            observers: HashMap::new(),
            clients: ExchangeClientsTuple::new(),
            is_running: false,
            config: config,
            runtime: Some(async_runtime),
        }
    }

    pub fn set_runtime(&mut self, runtime: Arc<Runtime>) {
        self.runtime = Some(runtime);
    }

    #[allow(dead_code)]
    fn create_clients(&mut self) {
        if let Some(binance_config) = &self.config.binance {
            let binance_client = Arc::new(RwLock::new(BinanceClient::new(
                binance_config.api_key.clone(),
                binance_config.api_secret.clone(),
            )));
            self.clients.set_binance_client(binance_client);
        }
    }

    pub fn launch(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.is_running {
            return Ok(());
        }

        let runtime = if let Some(runtime) = &self.runtime {
            runtime.clone()
        } else {
            return Err(Box::new(io::Error::new(
                io::ErrorKind::Other,
                "No runtime set",
            )));
        };

        if let Some(binance_config) = &self.config.binance {
            let binance_client = self.clients.binance_client.clone();

            let mut binance_observer =
                BinanceObserver::new(binance_config.clone(), binance_client, runtime);
            binance_observer.load_symbols_from_csv();
            binance_observer.start()?;
            self.observers
                .insert(ExchangeObserverKind::Binance, Box::new(binance_observer));
        }

        self.is_running = true;
        Ok(())
    }

    pub fn get_price(&self, kind: ExchangeObserverKind, symbol: &ExchangeSymbol) -> Option<f64> {
        if let Some(observer) = self.observers.get(&kind) {
            if let Some(price) = observer.get_price_from_table(&symbol) {
                return Some(price.lock().unwrap().base_price);
            }
        }

        None
    }

    pub fn remove_symbol(&mut self, kind: ExchangeObserverKind, symbol: ExchangeSymbol) {
        if let Some(observer) = self.observers.get_mut(&kind) {
            observer.remove_symbol(symbol);
        }
    }

    pub fn add_price_to_monitor(
        &mut self,
        kind: ExchangeObserverKind,
        symbol: &ExchangeSymbol,
        price: &Arc<Mutex<ExchangeValues>>,
    ) {
        if let Some(observer) = self.observers.get_mut(&kind) {
            observer.add_price_to_monitor(symbol, price);
        }
    }

    pub fn get_interchanged_symbols(&self, kind: ExchangeObserverKind, symbol: &String)
        -> Option<&'_ Vec<OrderedExchangeSymbol>> {
        if let Some(observer) = self.observers.get(&kind) {
            return Some(observer.get_interchanged_symbols(symbol));
        }

        None
    }

    pub fn get_usd_value(&self, kind: ExchangeObserverKind, symbol: String) -> Option<f64> {
        if let Some(observer) = self.observers.get(&kind) {
            return observer.get_usd_value(symbol);
        }

        None
    }

    pub fn get_watching_symbols(&self, kind: ExchangeObserverKind)
        -> Option<&'_ Vec<ExchangeSymbol>> {
        if let Some(observer) = self.observers.get(&kind) {
            return Some(observer.get_watching_symbols());
        }

        None
    }
}
