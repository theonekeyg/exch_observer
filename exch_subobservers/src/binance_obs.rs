use binance::{
    errors::Result as BResult,
    websockets::{WebSockets, WebsocketEvent},
};
// use csv::{Reader, StringRecord};
use log::{debug, info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    ops::Deref,
    str::FromStr,
    sync::{Arc, Mutex, RwLock},
    vec::Vec,
};
use tokio::{task::JoinHandle, runtime::Runtime};

use exch_clients::BinanceClient;
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ExchangeValues, OrderedExchangeSymbol,
    PairedExchangeSymbol, SwapOrder, USD_STABLES
};
use exch_observer_derive::ExchangeObserver;
use crate::internal::{ObserverWorkerThreadData};

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

#[derive(ExchangeObserver)]
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
    pub watching_symbols: Vec<Symbol>,
    /// `connected_symbols` - Connected symbols represent all existing pools on certain token,
    /// hence here `key` is single token (e.g. eth, btc), not pair (e.g. ethusdt).
    pub connected_symbols: HashMap<String, Vec<OrderedExchangeSymbol<Symbol>>>,
    /// `price_table` - Represents the main storage for prices, as well as accessing the
    /// prices in the storage. Here key is the pair name (e.g. ethusdt), not the single
    /// token like in `connected_symbols`. It is made this way to be able to index this
    /// map from already concatenated pair names, when turning concatenated string into
    /// ExchangeSymbol is impossible.
    price_table: Arc<HashMap<String, Arc<Mutex<AskBidValues>>>>,
    #[allow(dead_code)]
    client: Option<Arc<RwLock<BinanceClient<Symbol>>>>,
    async_runner: Arc<Runtime>,

    /// Necessary for getting control of the threads execution from a function.
    /// One example of such usage might be killing the thread with multiple symbols
    /// when `remove_symbol` was called on every symbol in this thread.
    threads_data_mapping: HashMap<Symbol, Arc<ObserverWorkerThreadData<Symbol>>>,
    running_handles: Vec<(JoinHandle<BResult<()>>, Arc<ObserverWorkerThreadData<Symbol>>)>,
    /// Symbols in the queue to be added to the new thread, which is created when
    /// `symbols_queue_limit` is reached
    symbols_in_queue: Vec<Symbol>,
    /// The maximum number of symbols that can be in the queue at any given time
    symbols_queue_limit: usize,
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
    pub fn new(
        client: Option<Arc<RwLock<BinanceClient<Symbol>>>>,
        async_runner: Arc<Runtime>,
    ) -> Self {
        Self {
            watching_symbols: vec![],
            connected_symbols: HashMap::new(),
            price_table: Arc::new(HashMap::new()),
            client: client,
            async_runner: async_runner,

            threads_data_mapping: HashMap::new(),
            running_handles: vec![],
            symbols_in_queue: vec![],
            symbols_queue_limit: 20,
        }
    }

    fn launch_worker_multiple(
        runner: &Runtime,
        symbols: &Vec<Symbol>,
        price_table: Arc<HashMap<String, Arc<Mutex<AskBidValues>>>>,
        thread_data: Arc<ObserverWorkerThreadData<Symbol>>,
    ) -> JoinHandle<BResult<()>> {
        info!("Started another batch of symbols");
        let ws_query_subs = symbols
            .iter()
            .map(|sym| {
                let sym = <Symbol as Into<String>>::into(sym.clone());
                book_ticker_stream(&sym)
            })
            .collect::<Vec<_>>();
        runner.spawn_blocking(move || {
            let mut websock = WebSockets::new(move |event: WebsocketEvent| -> BResult<()> {
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
            BResult::Ok(())
        })
    }
}
