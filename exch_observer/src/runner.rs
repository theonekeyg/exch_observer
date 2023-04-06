use std::{
    collections::HashMap,
};
use exch_observer_types::{ExchangeObserver, ExchangeObserverKind, ExchangeSymbol};
use exch_observer_config::ObserverConfig;
use exch_subobservers::BinanceObserver;

pub struct ObserverRunner {
    pub observers: HashMap<ExchangeObserverKind, Box<dyn ExchangeObserver>>,
    pub is_running: bool,
    pub config: ObserverConfig
}

impl ObserverRunner {
    pub fn new(config: ObserverConfig) -> Self {
        Self {
            observers: HashMap::new(),
            is_running: false,
            config: config,
        }
    }

    pub fn launch(&mut self) -> Result<(), Box<dyn std::error::Error>>{
        if let Some(binance_config) = &self.config.binance_config {
            let mut binance_observer = BinanceObserver::new(binance_config.clone());
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
