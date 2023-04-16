use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{Arc, RwLock},
    io
};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};
use exch_observer_types::{
    ExchangeObserver, ExchangeObserverKind, ExchangeSymbol, ExchangeClient
};
use exch_observer_config::ObserverConfig;
use exch_clients::BinanceClient;
use crate::BinanceObserver;

struct ExchangeClientsTuple {
    pub binance_client: Option<Arc<RwLock<BinanceClient>>>
    // TODO: Implement this struct to replace `observers` field in `CombinedObserver` struct. 
}

impl ExchangeClientsTuple {
    pub fn new() -> Self {
        Self {
            binance_client: None
        }
    }

    pub fn set_binance_client(&mut self, client: Arc<RwLock<BinanceClient>>) {
        self.binance_client = Some(client);
    }
}

pub struct CombinedObserver {
    pub observers: HashMap<ExchangeObserverKind, Box<dyn ExchangeObserver>>,
    // pub clients: HashMap<ExchangeObserverKind, Arc<Box<dyn ExchangeClient>>>,
    clients: ExchangeClientsTuple,
    pub is_running: bool,
    pub config: ObserverConfig,
    pub runtime: Option<Arc<Runtime>>
}

impl CombinedObserver {
    pub fn new(config: ObserverConfig) -> Self {
        let mut rv = Self {
            observers: HashMap::new(),
            // clients: HashMap::new(),
            clients: ExchangeClientsTuple::new(),
            is_running: false,
            config: config,
            runtime: None
        };

        // rv.create_clients();

        rv
    }

    pub fn new_with_runtime(config: ObserverConfig, async_runtime: Arc<Runtime>) -> Self {
        Self {
            observers: HashMap::new(),
            // clients: HashMap::new(),
            clients: ExchangeClientsTuple::new(),
            is_running: false,
            config: config,
            runtime: Some(async_runtime)
        }
    }

    pub fn set_runtime(&mut self, runtime: Arc<Runtime>) {
        self.runtime = Some(runtime);
    }

    fn create_clients(&mut self) {
        if let Some(binance_config) = &self.config.binance {
            let mut binance_client = Arc::new(RwLock::new(BinanceClient::new(
                binance_config.api_key.clone(),
                binance_config.api_secret.clone(),
            )));
            // self.clients.insert(ExchangeObserverKind::Binance, binance_client as Arc<Box<dyn ExchangeClient>>);
            self.clients.set_binance_client(binance_client);
        }
    }

    pub async fn launch(&mut self) -> Result<(), Box<dyn std::error::Error>>{
        if self.is_running {
            return Ok(());
        }

        let runtime = if let Some(runtime) = &self.runtime {
            runtime.clone()
        } else {
            return Err(Box::new(io::Error::new(io::ErrorKind::Other, "No runtime set")));
        };

        if let Some(binance_config) = &self.config.binance {
            // let mut binance_client = Arc::new(Box::new(BinanceClient::new(
            //     binance_config.api_key.clone(),
            //     binance_config.api_secret.clone(),
            // )) as Box<dyn ExchangeClient>);
            /*
            let binance_client = if let Some(ref mut client) = self.clients.get(&ExchangeObserverKind::Binance) {
                if !(client as &mut &Arc<Box<BinanceClient>>).has_runtime() {
                    (client as &mut &Arc<Box<BinanceClient>>).set_runtime(runtime.clone());
                }
                client.clone()
            } else {
                let mut client = Arc::new(Box::new(BinanceClient::new_with_runtime(
                    binance_config.api_key.clone(),
                    binance_config.api_secret.clone(),
                    runtime
                )) as Box<dyn ExchangeClient>);
                self.clients.insert(ExchangeObserverKind::Binance, client.clone());
                client
            };
            */

            /*
            let binance_client = if let Some(ref mut client) = self.clients.binance_client {
                let mut w_client = client.write().unwrap();
                if !w_client.has_runtime() {
                    w_client.set_runtime(runtime.clone());
                }
                Some(client.clone())
            } else {
                let mut client = Arc::new(RwLock::new(BinanceClient::new_with_runtime(
                    binance_config.api_key.clone(),
                    binance_config.api_secret.clone(),
                    runtime.clone()
                )));
                self.clients.set_binance_client(client.clone());
                Some(client)
            };
            */
            let binance_client = self.clients.binance_client.clone();

            let mut binance_observer = BinanceObserver::new(binance_config.clone(), binance_client, runtime);
            binance_observer.load_symbols_from_csv();
            binance_observer.start()?;
            self.observers.insert(ExchangeObserverKind::Binance, Box::new(binance_observer));
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
}

