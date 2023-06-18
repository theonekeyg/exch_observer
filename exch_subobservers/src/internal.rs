use exch_observer_types::PairedExchangeSymbol;
use std::{
    collections::HashMap,
    hash::Hash,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::task::JoinHandle;

/// Internal data structure for the observer worker threads.
/// Used to implement voting mechanism for symbols to stop their threads,
/// this mechanism allowes threads to monitor multiple symbols at once
pub struct ObserverWorkerThreadData<Symbol: Eq + Hash> {
    pub length: usize,
    pub requests_to_stop: usize,
    pub requests_to_stop_map: HashMap<Symbol, bool>,
    pub is_running: AtomicBool,
    pub handle: Option<JoinHandle<()>>,
}

impl<Symbol: Eq + Hash + Clone> ObserverWorkerThreadData<Symbol> {
    pub fn from(symbols: &Vec<Symbol>) -> Self {
        let length = symbols.len();

        let mut symbols_map = HashMap::new();
        for symbol in symbols {
            symbols_map.insert((*symbol).clone(), false);
        }

        Self {
            length: length,
            requests_to_stop: 0,
            requests_to_stop_map: symbols_map,
            is_running: AtomicBool::new(true),
            handle: None,
        }
    }

    #[allow(dead_code)]
    pub fn start_thread(&self) {
        self.is_running.store(true, Ordering::Relaxed);
    }

    pub fn stop_thread(&self) {
        self.is_running.store(false, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn get_running_symbols_iter(&self) -> impl Iterator<Item = &Symbol> {
        self.requests_to_stop_map
            .iter()
            .filter(|(_, &is_running)| is_running)
            .map(|(symbol, _)| symbol)
    }
}

pub fn kraken_symbol(symbol: impl PairedExchangeSymbol) -> String {
    format!("{}/{}", symbol.base(), symbol.quote())
}
