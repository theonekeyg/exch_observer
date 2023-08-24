use binance::websockets::{WebSockets, WebsocketEvent};
// use csv::{Reader, StringRecord};
use dashmap::DashMap;
use log::{info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    str::FromStr,
    sync::{mpsc, Arc, Mutex},
    vec::Vec,
};
use tokio::runtime::Runtime;

use crate::internal::MulticonObserverDriver;
use exch_observer_types::{
    AskBidValues, ExchangeKind, ExchangeObserver, ExchangeValues, ObserverWorkerThreadData,
    OrderedExchangeSymbol, PairedExchangeSymbol, PriceUpdateEvent,
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
        symbols: &Vec<Symbol>,
        price_table: Arc<DashMap<String, Arc<Mutex<<Self as ExchangeObserver<Symbol>>::Values>>>>,
        str_symbol_mapping: Arc<DashMap<String, Symbol>>,
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

        let thread_data_clone = thread_data.clone();
        let mut websock = WebSockets::new(move |event: WebsocketEvent| {
            match event {
                WebsocketEvent::OrderBook(order_book) => {
                    trace!("OrderBook: {:?}", order_book);
                }
                WebsocketEvent::BookTicker(book) => {
                    let sym_index = book.symbol.clone().to_ascii_lowercase();

                    let ask_price = f64::from_str(&book.best_ask).unwrap();
                    let bid_price = f64::from_str(&book.best_bid).unwrap();

                    let update_value = price_table.get(&sym_index).unwrap();
                    update_value
                        .lock()
                        .unwrap()
                        .update_price((ask_price, bid_price));
                    trace!("[{}] Ask: {:?}, Bid: {:?}", sym_index, ask_price, bid_price);

                    // Send price update events to the thread_data tx channel
                    let mut thread_data = thread_data.lock().unwrap();
                    let symbol = str_symbol_mapping
                        .get(&sym_index)
                        .expect(&format!(
                            "Symbol {} is not in the required mapping",
                            sym_index
                        ))
                        .clone();

                    thread_data.update_price_event(PriceUpdateEvent::new(
                        ExchangeKind::Binance,
                        symbol.clone(),
                        AskBidValues::new_with_prices(ask_price, bid_price),
                    ));
                }
                WebsocketEvent::Kline(kline) => {
                    let kline = kline.kline;

                    let sym_index = kline.symbol.clone().to_ascii_lowercase();

                    let ask_price = f64::from_str(kline.high.as_ref()).unwrap();
                    let bid_price = f64::from_str(kline.low.as_ref()).unwrap();

                    let update_value = price_table.get(&sym_index).unwrap();
                    update_value
                        .lock()
                        .unwrap()
                        .update_price((ask_price, bid_price));
                    trace!("[{}] Ask: {:?}, Bid: {:?}", sym_index, ask_price, bid_price);

                    // Send price update events to the thread_data tx channel
                    let mut thread_data = thread_data.lock().unwrap();
                    let symbol = str_symbol_mapping
                        .get(&sym_index)
                        .expect(&format!(
                            "Symbol {} is not in the required mapping",
                            sym_index
                        ))
                        .clone();
                    thread_data.update_price_event(PriceUpdateEvent::new(
                        ExchangeKind::Binance,
                        symbol.clone(),
                        AskBidValues::new_with_prices(ask_price, bid_price),
                    ));
                }
                _ => (),
            }
            Ok(())
        });

        websock.connect_multiple_streams(&ws_query_subs).unwrap();
        let is_running = {
            let thread_data = thread_data_clone.lock().unwrap();
            thread_data.is_running.clone()
        };
        websock.event_loop(&is_running).unwrap();
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
        + 'static,
{
    type Values = AskBidValues;

    fn get_interchanged_symbols(&self, symbol: &String) -> &'_ Vec<OrderedExchangeSymbol<Symbol>> {
        self.driver.get_interchanged_symbols(symbol)
    }

    fn add_price_to_monitor(&mut self, symbol: &Symbol, price: Arc<Mutex<Self::Values>>) {
        info!("Adding {} to Binance watching symbols", &symbol);
        self.driver.add_price_to_monitor(symbol, price);
    }

    fn get_price_from_table(&self, symbol: &Symbol) -> Option<Arc<Mutex<Self::Values>>> {
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
        info!("Removing symbol {} from Binance observer", symbol);
        self.driver.remove_symbol(symbol);
    }

    fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        self.driver.get_watching_symbols()
    }

    fn set_tx_fifo(&mut self, tx: mpsc::Sender<PriceUpdateEvent<Symbol>>) {
        self.driver.set_tx_fifo(tx);
    }

    fn dump_price_table(&self) -> HashMap<Symbol, Self::Values> {
        self.driver.dump_price_table()
    }
}
