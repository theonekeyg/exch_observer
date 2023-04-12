use std::{
    collections::HashMap,
    fmt::Debug,
    sync::Arc
};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};
use exch_observer_types::{
    ExchangeObserver, ExchangeObserverKind, ExchangeSymbol, ExchangeClient
};
use exch_observer_config::ObserverConfig;
use exch_subobservers::BinanceObserver;
use exch_clients::BinanceClient;

pub struct ObserverRunner {
    pub observers: HashMap<ExchangeObserverKind, Box<dyn ExchangeObserver>>,
    pub clients: HashMap<ExchangeObserverKind, Arc<Box<dyn ExchangeClient>>>,
    pub is_running: bool,
    pub config: ObserverConfig,
    pub runtime: Arc<Runtime>
}

impl ObserverRunner {
    pub fn new(config: ObserverConfig) -> Self {
        let async_runtime = Arc::new(
            RuntimeBuilder::new_multi_thread()
                .worker_threads(config.num_threads.unwrap_or(4))
                .build()
                .unwrap()
        );
        Self {
            observers: HashMap::new(),
            clients: HashMap::new(),
            is_running: false,
            config: config,
            runtime: async_runtime
        }
    }

    pub fn get_async_runner(&self) -> &'_ Arc<Runtime> {
        return &self.runtime;
    }

    pub fn launch(&mut self) -> Result<(), Box<dyn std::error::Error>>{
        if let Some(binance_config) = &self.config.binance {
            let binance_client = Arc::new(Box::new(BinanceClient::new(
                binance_config.api_key.clone(),
                binance_config.api_secret.clone(),
                self.runtime.clone()
            )) as Box<dyn ExchangeClient>);
            let mut binance_observer = BinanceObserver::new(binance_config.clone(), Some(binance_client.clone()), self.runtime.clone());
            binance_observer.load_symbols_from_csv();
            binance_observer.start()?;
            self.observers.insert(ExchangeObserverKind::Binance, Box::new(binance_observer));
            self.clients.insert(ExchangeObserverKind::Binance, binance_client);
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
