use binance::{
    model::Symbol as BSymbol,
    websockets::{WebSockets, WebsocketEvent},
};
// use csv::{Reader, StringRecord};
use log::{info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    str::FromStr,
    sync::{Arc, Mutex},
    vec::Vec,
};
use tokio::{runtime::Runtime, task::JoinHandle};

use crate::internal::MulticonObserverDriver;
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ExchangeValues, ObserverWorkerThreadData,
    OrderedExchangeSymbol, PairedExchangeSymbol,
};

#[allow(unused)]
fn all_ticker_stream() -> &'static str {
    "!ticker@arr"
}

#[allow(unused)]
fn ticker_stream(symbol: &str) -> String {
    format!("{symbol}@ticker")
}

#[allow(unused)]
fn agg_trade_stream(symbol: &str) -> String {
    format!("{symbol}@aggTrade")
}

#[allow(unused)]
fn trade_stream(symbol: &str) -> String {
    format!("{symbol}@trade")
}

#[allow(unused)]
fn kline_stream(symbol: &str, interval: &str) -> String {
    format!("{symbol}@kline_{interval}")
}

#[allow(unused)]
fn book_ticker_stream(symbol: &str) -> String {
    format!("{symbol}@bookTicker")
}

#[allow(unused)]
fn all_book_ticker_stream() -> &'static str {
    "!bookTicker"
}

#[allow(unused)]
fn all_mini_ticker_stream() -> &'static str {
    "!miniTicker@arr"
}

#[allow(unused)]
fn mini_ticker_stream(symbol: &str) -> String {
    format!("{symbol}@miniTicker")
}

#[allow(unused)]
/// # Arguments
///
/// * `symbol`: the market symbol
/// * `levels`: 5, 10 or 20
/// * `update_speed`: 1000 or 100
fn partial_book_depth_stream(symbol: &str, levels: u16, update_speed: u16) -> String {
    format!("{symbol}@depth{levels}@{update_speed}ms")
}

#[allow(unused)]
/// # Arguments
///
/// * `symbol`: the market symbol
/// * `update_speed`: 1000 or 100
fn diff_book_depth_stream(symbol: &str, update_speed: u16) -> String {
    format!("{symbol}@depth@{update_speed}ms")
}

pub struct BinanceObserver<Symbol>
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
        + From<BSymbol>
        + 'static,
{
    driver: MulticonObserverDriver<Symbol, Self>,
}

impl<Symbol> BinanceObserver<Symbol>
where
    Symbol: Eq
        + Hash
        + Clone
        + Display
        + Debug
        + Into<String>
        + Send
        + Sync
        + From<BSymbol>
        + PairedExchangeSymbol
        + 'static,
{
    pub fn new(async_runner: Arc<Runtime>) -> Self {
        Self {
            driver: MulticonObserverDriver::new(async_runner, 20, Self::launch_worker_multiple),
        }
    }

    /// Launches the observer thread with the given symbols
    pub fn launch_worker_multiple(
        runner: &Runtime,
        symbols: &Vec<Symbol>,
        price_table: Arc<HashMap<String, Arc<Mutex<<Self as ExchangeObserver<Symbol>>::Values>>>>,
        thread_data: Arc<ObserverWorkerThreadData<Symbol>>,
    ) -> JoinHandle<()> {
        info!("Started another batch of symbols");
        let ws_query_subs = symbols
            .iter()
            .map(|sym| {
                let sym = <Symbol as Into<String>>::into(sym.clone());
                book_ticker_stream(&sym)
            })
            .collect::<Vec<_>>();
        runner.spawn_blocking(move || {
            let mut websock = WebSockets::new(move |event: WebsocketEvent| {
                match event {
                    WebsocketEvent::OrderBook(order_book) => {
                        trace!("OrderBook: {:?}", order_book);
                    }
                    WebsocketEvent::BookTicker(book) => {
                        let sym_index = book.symbol.clone().to_ascii_lowercase();

                        let ask_price = f64::from_str(&book.best_ask).unwrap();
                        let bid_price = f64::from_str(&book.best_bid).unwrap();
                        let price = (ask_price + bid_price) / 2.0;

                        let update_value = price_table.get(&sym_index).unwrap();
                        update_value
                            .lock()
                            .unwrap()
                            .update_price((ask_price, bid_price));
                        trace!("[{}] Price: {:?}", book.symbol, price);
                    }
                    WebsocketEvent::Kline(kline) => {
                        let kline = kline.kline;

                        let sym_index = kline.symbol.clone().to_ascii_lowercase();

                        let price_high = f64::from_str(kline.high.as_ref()).unwrap();
                        let price_low = f64::from_str(kline.low.as_ref()).unwrap();
                        let price = (price_high + price_low) / 2.0;

                        let update_value = price_table.get(&sym_index).unwrap();
                        update_value
                            .lock()
                            .unwrap()
                            .update_price((price_high, price_low));
                        trace!("[{}] Price: {:?}", kline.symbol, price);
                    }
                    _ => (),
                }
                Ok(())
            });
            websock.connect_multiple_streams(&ws_query_subs).unwrap();
            websock.event_loop(&thread_data.is_running).unwrap();
        })
    }
}

impl<Symbol> ExchangeObserver<Symbol> for BinanceObserver<Symbol>
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
        + From<BSymbol>
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
        info!("Starting Binance Observer");
        self.driver.start()
    }

    fn remove_symbol(&mut self, symbol: Symbol) {
        self.driver.remove_symbol(symbol);
    }

    fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        self.driver.get_watching_symbols()
    }
}
