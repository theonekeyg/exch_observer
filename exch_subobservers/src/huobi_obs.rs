use crate::internal::MulticonObserverDriver;
use dashmap::DashMap;
use exch_apis::{common::WebsocketEvent, huobi_ws::HuobiWebsocket};
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ExchangeValues, ObserverWorkerThreadData,
    OrderedExchangeSymbol, PairedExchangeSymbol, PriceUpdateEvent
};
use log::{info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    sync::{Arc, Mutex, mpsc},
};
use tokio::runtime::Runtime;

#[allow(unused)]
fn kline_stream(symbol: &str, interval: &str) -> String {
    format!("market.{symbol}.kline.{interval}")
}

#[allow(unused)]
fn book_ticker_stream(symbol: &str) -> String {
    format!("market.{symbol}.ticker")
}

pub struct HuobiObserver<Symbol>
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

impl<Symbol> HuobiObserver<Symbol>
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
            driver: MulticonObserverDriver::new(async_runner, 16, Self::launch_worker_multiple),
        }
    }

    fn launch_worker_multiple(
        symbols: &Vec<Symbol>,
        price_table: Arc<DashMap<String, Arc<Mutex<<Self as ExchangeObserver<Symbol>>::Values>>>>,
        _str_symbol_mapping: Arc<DashMap<String, Symbol>>,
        thread_data: Arc<Mutex<ObserverWorkerThreadData<Symbol>>>,
    ) {
        info!("Started another batch of symbols");
        let ws_query_subs = symbols
            .iter()
            .map(|sym| {
                let sym = <Symbol as Into<String>>::into(sym.clone());
                book_ticker_stream(&sym)
            })
            .collect::<Vec<_>>();
        let mut websock = HuobiWebsocket::new(move |event: WebsocketEvent| match event {
            WebsocketEvent::KLineEvent(kline) => {
                let price_high = kline.high;
                let price_low = kline.low;
                let price = (price_high + price_low) / 2.0;
                let sym_index = kline.sym.clone().to_ascii_lowercase();
                let update_value = price_table.get(&sym_index).unwrap();
                update_value
                    .lock()
                    .unwrap()
                    .update_price((price_high, price_low));
                trace!("[{}] Price: {:?}", sym_index, price);
            }
            WebsocketEvent::BookTickerEvent(book) => {
                let ask_price = book.best_ask;
                let bid_price = book.best_bid;
                let sym_index = book.sym.clone().to_ascii_lowercase();
                let update_value = price_table.get(&sym_index).unwrap();
                update_value
                    .lock()
                    .unwrap()
                    .update_price((ask_price, bid_price));
                trace!("[{}] Ask: {:?}, Bid: {:?}", sym_index, ask_price, bid_price);
            }
        });
        websock.connect_multiple_streams(ws_query_subs).unwrap();
        websock
            .event_loop(&thread_data.lock().unwrap().is_running)
            .unwrap();
    }
}

impl<Symbol> ExchangeObserver<Symbol> for HuobiObserver<Symbol>
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
        info!("Adding {} to Huobi watching symbols", &symbol);
        self.driver.add_price_to_monitor(symbol, price);
    }

    fn get_price_from_table(&self, symbol: &Symbol) -> Option<Arc<Mutex<Self::Values>>> {
        self.driver.get_price_from_table(symbol)
    }

    fn get_usd_value(&self, sym: &String) -> Option<f64> {
        self.driver.get_usd_value(sym)
    }

    fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting Huobi Observer");
        self.driver.start()
    }

    fn remove_symbol(&mut self, symbol: Symbol) {
        info!("Removing symbol {} from Huobi observer", symbol);
        self.driver.remove_symbol(symbol);
    }

    fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        self.driver.get_watching_symbols()
    }

    fn set_tx_fifo(&mut self, tx: mpsc::Sender<PriceUpdateEvent>) {
        self.driver.set_tx_fifo(tx);
    }

    fn dump_price_table(&self) -> HashMap<String, Self::Values> {
        self.driver.dump_price_table()
    }
}
