use exch_apis::websockets::{HuobiWebsocket, WebsocketEvent};
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ExchangeValues, OrderedExchangeSymbol,
    PairedExchangeSymbol, SwapOrder, USD_STABLES
};
use exch_observer_derive::ExchangeObserver;
use log::{debug, info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    ops::Deref,
    sync::{Arc, Mutex},
};
use tokio::runtime::Runtime;
use crate::internal::{ObserverWorkerThreadData};

#[allow(unused)]
fn kline_stream(symbol: &str, interval: &str) -> String {
    format!("market.{symbol}.kline.{interval}")
}

#[allow(unused)]
fn book_ticker_stream(symbol: &str) -> String {
    format!("market.{symbol}.ticker")
}

#[derive(ExchangeObserver)]
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
    pub watching_symbols: Vec<Symbol>,
    pub connected_symbols: HashMap<String, Vec<OrderedExchangeSymbol<Symbol>>>,
    price_table: Arc<HashMap<String, Arc<Mutex<AskBidValues>>>>,
    // client: Option<Arc<RwLock<BinanceClient<Symbol>>>>,
    async_runner: Arc<Runtime>,

    /// Necessary for getting control of the threads execution from a function.
    /// One example of such usage might be killing the thread with multiple symbols
    /// when `remove_symbol` was called on every symbol in this thread.
    threads_data_mapping: HashMap<Symbol, Arc<ObserverWorkerThreadData<Symbol>>>,
    /// Symbols in the queue to be added to the new thread, which is created when
    /// `symbols_queue_limit` is reached
    symbols_in_queue: Vec<Symbol>,
    /// The maximum number of symbols that can be in the queue at any given time
    symbols_queue_limit: usize,
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
            watching_symbols: vec![],
            connected_symbols: HashMap::new(),
            price_table: Arc::new(HashMap::new()),
            // client: client,
            async_runner: async_runner,

            threads_data_mapping: HashMap::new(),
            symbols_in_queue: vec![],
            symbols_queue_limit: 16,
        }
    }

    fn launch_worker_multiple(
        runner: &Runtime,
        symbols: &Vec<Symbol>,
        price_table: Arc<HashMap<String, Arc<Mutex<AskBidValues>>>>,
        thread_data: Arc<ObserverWorkerThreadData<Symbol>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("Started another batch of symbols");
        let ws_query_subs = symbols
            .iter()
            .map(|sym| {
                let sym = <Symbol as Into<String>>::into(sym.clone());
                book_ticker_stream(&sym)
            })
            .collect::<Vec<_>>();
        runner.spawn_blocking(move || {
            let mut websock = HuobiWebsocket::new(
                move |event: WebsocketEvent| -> Result<(), Box<dyn std::error::Error>> {
                    match event {
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
                    }

                    Ok(())
                },
            );
            websock.connect_multiple_streams(ws_query_subs).unwrap();
            websock
                .event_loop(&thread_data.is_running)
                .unwrap();
        });

        Ok(())
    }
}
