use exch_apis::websockets::{HuobiWebsocket, WebsocketEvent};
use exch_observer_types::{
    AskBidValues, ExchangeObserver, ExchangeValues, OrderedExchangeSymbol, PairedExchangeSymbol,
    SwapOrder,
};
use log::{debug, info, trace};
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
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

static HUOBI_USD_STABLES: [&str; 4] = ["usdt", "usdc", "busd", "dai"];

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
    price_table: Arc<HashMap<Symbol, Arc<Mutex<AskBidValues>>>>,
    is_running_table: Arc<HashMap<Symbol, AtomicBool>>,
    // client: Option<Arc<RwLock<BinanceClient<Symbol>>>>,
    async_runner: Arc<Runtime>,
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
            is_running_table: Arc::new(HashMap::new()),
            // client: client,
            async_runner: async_runner,
        }
    }

    fn launch_worker(
        runner: &Runtime,
        symbol: Symbol,
        update_value: Arc<Mutex<AskBidValues>>,
        is_running_table: Arc<HashMap<Symbol, AtomicBool>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        runner.spawn_blocking(move || {
            let _symbol = symbol.clone();
            let mut websock = HuobiWebsocket::new(move |event: WebsocketEvent| {
                match event {
                    WebsocketEvent::KLineEvent(kline) => {
                        let price_high = kline.high;
                        let price_low = kline.low;
                        let price = (price_high + price_low) / 2.0;
                        update_value
                            .lock()
                            .unwrap()
                            .update_price((price_high, price_low));
                        trace!("[{}] Price: {:?}", _symbol, price);
                    }
                    WebsocketEvent::BookTickerEvent(book) => {
                        let ask_price = book.best_ask;
                        let bid_price = book.best_bid;
                        update_value.lock().unwrap().update_price((ask_price, bid_price));
                        trace!("[{}] Ask: {:?}, Bid: {:?}", _symbol, ask_price, bid_price);
                    }
                }

                Ok(())
            });

            websock
                .connect(&book_ticker_stream(&symbol.clone().into()))
                .unwrap();
            let is_running = is_running_table.get(&symbol).unwrap();
            is_running.store(true, Ordering::Relaxed);
            websock.event_loop(is_running).unwrap();
        });

        Ok(())
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
        &self.connected_symbols.get::<String>(symbol).unwrap()
    }

    fn add_price_to_monitor(&mut self, symbol: &Symbol, price: &Arc<Mutex<Self::Values>>) {
        if !self.price_table.contains_key(&symbol) {
            // Since with this design it's impossible to modify external ExchangeValues from
            // thread, we're not required to lock the whole HashMap, since each thread modifies
            // it's own *ExchangeValue.
            //
            // Unfortunately unsafe is required to achieve that in modern Rust, as there are no
            // other options to expose that logic to the compiler.
            unsafe {
                let ptable_ptr = Arc::as_ptr(&self.price_table)
                    as *mut HashMap<Symbol, Arc<Mutex<Self::Values>>>;
                (*ptable_ptr).insert(symbol.clone(), price.clone());
            }

            if !self.connected_symbols.contains_key(symbol.base()) {
                self.connected_symbols
                    .insert(symbol.base().to_string().clone(), Vec::new());
            }

            if !self.connected_symbols.contains_key(symbol.quote()) {
                self.connected_symbols
                    .insert(symbol.quote().to_string().clone(), Vec::new());
            }

            self.connected_symbols
                .get_mut(symbol.base())
                .unwrap()
                .push(OrderedExchangeSymbol::new(&symbol, SwapOrder::Sell));
            self.connected_symbols
                .get_mut(symbol.quote())
                .unwrap()
                .push(OrderedExchangeSymbol::new(&symbol, SwapOrder::Buy));

            let update_value = price.clone();
            // self.is_running_table.insert(symbol.clone(), AtomicBool::new(false));

            unsafe {
                let is_running_ptr =
                    Arc::as_ptr(&self.is_running_table) as *mut HashMap<Symbol, AtomicBool>;
                (*is_running_ptr).insert(symbol.clone(), AtomicBool::new(false));
            }

            Self::launch_worker(
                self.async_runner.deref(),
                symbol.clone(),
                update_value,
                self.is_running_table.clone(),
            )
            .unwrap();

            info!("Added {} to the watching symbols", &symbol);
            self.watching_symbols.push(symbol.clone())
        }
    }

    fn get_price_from_table(&self, symbol: &Symbol) -> Option<&Arc<Mutex<Self::Values>>> {
        self.price_table.get(symbol)
    }

    fn get_usd_value(&self, sym: &String) -> Option<f64> {
        // TODO: This lock USD wrapped tokens to 1 seems to be unnecessary,
        // considering to remove this later
        if let Some(_) = HUOBI_USD_STABLES.into_iter().find(|v| v == sym) {
            return Some(1.0);
        };

        let connected = self.get_interchanged_symbols(sym);

        for ordered_sym in connected.iter() {
            for stable in &HUOBI_USD_STABLES {
                if <&str as Into<String>>::into(stable) == ordered_sym.symbol.base()
                    || <&str as Into<String>>::into(stable) == ordered_sym.symbol.quote()
                {
                    return self.get_price_from_table(&ordered_sym.symbol).map(|v| {
                        let unlocked = v.lock().unwrap();
                        (unlocked.get_ask_price() + unlocked.get_bid_price()) / 2.0
                    });
                }
            }
        }

        None
    }

    fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting Huobi Observer");
        // for symbol in &self.watching_symbols {
        //     let update_value;

        //     unsafe {
        //         let table_entry_ptr = Arc::get_mut_unchecked(&mut self.price_table);

        //         update_value = (*table_entry_ptr).get_mut(&symbol).unwrap().clone();
        //     }

        //     Self::launch_worker(
        //         self.async_runner.deref(),
        //         symbol.clone(),
        //         update_value,
        //         self.is_running_table.clone(),
        //     )
        //     .unwrap();
        // }
        Ok(())
    }

    fn remove_symbol(&mut self, symbol: Symbol) {
        // Remove symbol from `watching_symbols`, `connected_symbols` from both base and quote
        // symbols and `price_table`, also stop it's worker by flipping it's bool in
        // `is_running_table`.

        // Remove from the `watching_symbols`
        for (i, sym) in self.watching_symbols.iter().enumerate() {
            if sym == &symbol {
                self.watching_symbols.remove(i);
                break;
            }
        }

        // Remove from maptrees.
        let base_connected = self.connected_symbols.get_mut(symbol.base()).unwrap();
        for (i, sym) in base_connected.iter().enumerate() {
            if sym.symbol == symbol {
                base_connected.remove(i);
                break;
            }
        }

        let quote_connected = self.connected_symbols.get_mut(symbol.quote()).unwrap();
        for (i, sym) in quote_connected.iter().enumerate() {
            if sym.symbol == symbol {
                quote_connected.remove(i);
                break;
            }
        }

        // Flip running flag
        let is_running = self.is_running_table.get(&symbol).unwrap();
        is_running.store(false, Ordering::Relaxed);

        // Remove from `price_table` and `is_running_table`
        unsafe {
            let ptable_ptr =
                Arc::as_ptr(&self.price_table) as *mut HashMap<Symbol, Arc<Mutex<Self::Values>>>;
            (*ptable_ptr).remove(&symbol);

            let runing_table_ptr =
                Arc::as_ptr(&self.is_running_table) as *mut HashMap<Symbol, AtomicBool>;
            (*runing_table_ptr).remove(&symbol);
        }

        debug!("Removed symbol {} from Huobi observer", symbol);
    }

    fn get_watching_symbols(&self) -> &'_ Vec<Symbol> {
        return &self.watching_symbols;
    }
}
