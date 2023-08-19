use crate::internal::MulticonObserverDriver;
use dashmap::DashMap;
use exch_apis::{
    common::{Result as WsResult, WebsocketEvent},
    kraken_ws::KrakenWebsocket,
};
use exch_observer_types::{
    AskBidValues, ExchangeKind, ExchangeObserver, ExchangeSymbol, ExchangeValues,
    ObserverWorkerThreadData, OrderedExchangeSymbol, PairedExchangeSymbol, PriceUpdateEvent,
};
use log::{info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    sync::{mpsc, Arc, Mutex},
};
use tokio::runtime::Runtime;

pub static KRAKEN_USD_STABLES: [&str; 4] = ["USDT", "USD", "DAI", "USDC"];

pub fn kraken_symbol(symbol: impl PairedExchangeSymbol) -> String {
    format!("{}/{}", symbol.base(), symbol.quote())
}

pub struct KrakenObserver<Symbol>
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

impl<Symbol> KrakenObserver<Symbol>
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

    fn launch_worker_multiple(
        symbols: &Vec<Symbol>,
        price_table: Arc<DashMap<String, Arc<Mutex<<Self as ExchangeObserver<Symbol>>::Values>>>>,
        str_symbol_mapping: Arc<DashMap<String, Symbol>>,
        thread_data: Arc<Mutex<ObserverWorkerThreadData<Symbol>>>,
    ) {
        info!("Started another batch of symbols");
        let ws_query_subs = symbols
            .iter()
            .map(|sym| {
                format!(
                    "{}/{}",
                    sym.base().to_uppercase(),
                    sym.quote().to_uppercase()
                )
            })
            .collect::<Vec<_>>();

        let thread_data_clone = thread_data.clone();
        let mut websock = KrakenWebsocket::new(move |event: WebsocketEvent| -> WsResult<()> {
            match event {
                WebsocketEvent::KLineEvent(kline) => {
                    let ask_price = kline.high;
                    let bid_price = kline.low;
                    // let price = (ask_price + bid_price) / 2.0;
                    let sym_index = kline.sym.clone();
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
                        ExchangeKind::Kraken,
                        ExchangeSymbol::new(symbol.base(), symbol.quote()),
                        AskBidValues::new_with_prices(ask_price, bid_price),
                    ));
                }
                WebsocketEvent::BookTickerEvent(book) => {
                    let ask_price = book.best_ask;
                    let bid_price = book.best_bid;
                    // let sym_index = book.sym.clone();
                    // split sym_index (which has format of "<base>/<quote>") into two parts
                    // and join them together
                    let sym_index = book.sym.split('/').collect::<Vec<&str>>().join("");
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
                        ExchangeKind::Kraken,
                        ExchangeSymbol::new(symbol.base(), symbol.quote()),
                        AskBidValues::new_with_prices(ask_price, bid_price),
                    ));
                }
            }

            WsResult::Ok(())
        });

        websock
            .connect_multiple_streams(ws_query_subs)
            .expect("Failed connect streams");

        let is_running = {
            let thread_data = thread_data_clone.lock().unwrap();
            thread_data.is_running.clone()
        };
        websock.event_loop(&is_running).expect("Failed event loop");
    }
}

impl<Symbol> ExchangeObserver<Symbol> for KrakenObserver<Symbol>
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
        info!("Adding {} to Kraken watching symbols", &symbol);
        self.driver.add_price_to_monitor(symbol, price);
    }

    fn get_price_from_table(&self, symbol: &Symbol) -> Option<Arc<Mutex<Self::Values>>> {
        self.driver.get_price_from_table(symbol)
    }

    fn get_usd_value(&self, sym: &String) -> Option<f64> {
        self.driver.get_usd_value(sym)
    }

    fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting Kraken Observer");
        self.driver.start()
    }

    fn remove_symbol(&mut self, symbol: Symbol) {
        info!("Removing symbol {} from Kraken observer", symbol);
        self.driver.remove_symbol(symbol);
    }

    fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        self.driver.get_watching_symbols()
    }

    fn set_tx_fifo(&mut self, tx: mpsc::Sender<PriceUpdateEvent>) {
        self.driver.set_tx_fifo(tx);
    }

    fn dump_price_table(&self) -> HashMap<Symbol, Self::Values> {
        self.driver.dump_price_table()
    }
}
