use crate::internal::MulticonObserverDriver;
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ObserverWorkerThreadData, OrderedExchangeSymbol,
    PairedExchangeSymbol,
};
use log::info;
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    sync::{Arc, Mutex},
};
use tokio::runtime::Runtime;

pub struct MockerObserver<Symbol>
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
    driver: MulticonObserverDriver<Symbol, Self>,
}

impl<Symbol> MockerObserver<Symbol>
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
            driver: MulticonObserverDriver::new(async_runner, 1, Self::new_instance),
        }
    }

    fn new_instance(
        _symbols: &Vec<Symbol>,
        _price_table: Arc<HashMap<String, Arc<Mutex<<Self as ExchangeObserver<Symbol>>::Values>>>>,
        _thread_data: Arc<ObserverWorkerThreadData<Symbol>>,
    ) {
        println!("new_instance()");
    }
}

impl<Symbol> ExchangeObserver<Symbol> for MockerObserver<Symbol>
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
        self.driver.get_interchanged_symbols(symbol)
    }

    fn add_price_to_monitor(&mut self, symbol: &Symbol, price: Arc<Mutex<Self::Values>>) {
        self.driver.add_price_to_monitor(symbol, price);
        info!("Added {} to the watching symbols", &symbol);
    }

    fn get_price_from_table(&self, symbol: &Symbol) -> Option<&Arc<Mutex<Self::Values>>> {
        self.driver.get_price_from_table(symbol)
    }

    fn get_usd_value(&self, sym: &String) -> Option<f64> {
        self.driver.get_usd_value(sym)
    }

    fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting Mocker Observer");
        self.driver.start()
    }

    fn remove_symbol(&mut self, symbol: Symbol) {
        self.driver.remove_symbol(symbol);
    }

    fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        self.driver.get_watching_symbols()
    }
}
