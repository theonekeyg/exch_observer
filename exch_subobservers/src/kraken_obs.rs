use crate::internal::MulticonObserverDriver;
use exch_apis::{
    common::{Result as WsResult, WebsocketEvent},
    kraken_ws::KrakenWebsocket,
};
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ExchangeValues, ObserverWorkerThreadData,
    OrderedExchangeSymbol, PairedExchangeSymbol,
};
use log::{info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    sync::{Arc, Mutex},
};
use tokio::{runtime::Runtime, task::JoinHandle};

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
        runner: &Runtime,
        symbols: &Vec<Symbol>,
        price_table: Arc<HashMap<String, Arc<Mutex<<Self as ExchangeObserver<Symbol>>::Values>>>>,
        thread_data: Arc<ObserverWorkerThreadData<Symbol>>,
    ) -> JoinHandle<()> {
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
        runner.spawn_blocking(move || {
            let mut websock = KrakenWebsocket::new(move |event: WebsocketEvent| -> WsResult<()> {
                match event {
                    WebsocketEvent::KLineEvent(kline) => {
                        let price_high = kline.high;
                        let price_low = kline.low;
                        let price = (price_high + price_low) / 2.0;
                        let sym_index = kline.sym.clone();
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
                    }
                }

                WsResult::Ok(())
            });
            websock
                .connect_multiple_streams(ws_query_subs)
                .expect("Failed connect streams");
            websock
                .event_loop(&thread_data.is_running)
                .expect("Failed event loop");
        })
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
        info!("Starting Huobi Observer");
        self.driver.start()
    }

    fn remove_symbol(&mut self, symbol: Symbol) {
        self.driver.remove_symbol(symbol);
    }

    fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        self.driver.get_watching_symbols()
    }
}
