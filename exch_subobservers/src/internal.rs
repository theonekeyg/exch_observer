use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    hash::Hash
};

pub struct ObserverWorkerThreadData<Symbol: Eq + Hash> {
    pub length: usize,
    pub requests_to_stop: usize,
    pub requests_to_stop_map: HashMap<Symbol, bool>,
    pub is_running: AtomicBool,
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
        }
    }

    #[allow(dead_code)]
    pub fn start_thread(&self) {
        self.is_running.store(true, Ordering::Relaxed);
    }

    pub fn stop_thread(&self) {
        self.is_running.store(false, Ordering::Relaxed);
    }
}

