use binance::{account::OrderSide, model::Balance as BinanceBalance};
use std::{
    fmt::Debug,
    fmt::{self, Display, Formatter},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

pub trait PairedExchangeSymbol {
    fn base(&self) -> &str;
    fn quote(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct ExchangeSymbol {
    pub base: String,
    pub quote: String,
}

impl PairedExchangeSymbol for ExchangeSymbol {
    fn base(&self) -> &str {
        &self.base
    }

    fn quote(&self) -> &str {
        &self.quote
    }
}

impl Hash for ExchangeSymbol {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.base.hash(state);
        self.quote.hash(state);
    }
}

impl Into<String> for ExchangeSymbol {
    fn into(self) -> String {
        return self.base + &self.quote;
    }
}

impl Into<String> for &ExchangeSymbol {
    fn into(self) -> String {
        return self.base.clone() + &self.quote;
    }
}

impl ExchangeSymbol {
    pub fn from<S: Into<String>>(base: S, quote: S) -> Self {
        Self {
            base: base.into(),
            quote: quote.into(),
        }
    }

    pub fn new<S: Into<String>>(base: S, quote: S) -> Self {
        Self {
            base: base.into(),
            quote: quote.into(),
        }
    }
}

impl PartialEq for ExchangeSymbol {
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base && self.quote == other.quote
    }
}

impl Display for ExchangeSymbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "{}{}", self.base, self.quote)
    }
}

impl Eq for ExchangeSymbol {}

#[derive(Debug, Eq, Hash, PartialEq, Copy, Clone)]
pub enum SwapOrder {
    Sell,
    Buy,
}

impl Display for SwapOrder {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "{:?}", self)
    }
}

impl Into<OrderSide> for SwapOrder {
    fn into(self) -> OrderSide {
        match self {
            SwapOrder::Sell => OrderSide::Sell,
            SwapOrder::Buy => OrderSide::Buy,
        }
    }
}

#[derive(Debug, Eq, Hash, PartialEq, Clone)]
pub struct OrderedExchangeSymbol<Symbol: Eq + Hash> {
    pub symbol: Symbol,
    pub order: SwapOrder,
}

impl<Symbol: Eq + Hash + Clone + PairedExchangeSymbol> OrderedExchangeSymbol<Symbol> {
    pub fn new(symbol: &Symbol, order: SwapOrder) -> Self {
        Self {
            symbol: symbol.clone(),
            order: order,
        }
    }

    pub fn get_output_symbol(&self) -> String {
        let rv = match self.order {
            SwapOrder::Buy => self.symbol.base().into(),
            SwapOrder::Sell => self.symbol.quote().into(),
        };

        rv
    }

    pub fn get_input_symbol(&self) -> String {
        let rv = match self.order {
            SwapOrder::Buy => self.symbol.quote().into(),
            SwapOrder::Sell => self.symbol.base().into(),
        };

        rv
    }
}

#[derive(Debug, Clone)]
pub struct ExchangeValues {
    pub base_price: f64,
}

unsafe impl Send for ExchangeValues {}
unsafe impl Sync for ExchangeValues {}

impl ExchangeValues {
    pub fn new() -> Self {
        Self { base_price: 0.0 }
    }

    pub fn new_with_price(base_price: f64) -> Self {
        Self {
            base_price: base_price,
        }
    }

    pub fn update_price(&mut self, base_price: f64) {
        self.base_price = base_price;
    }
}

pub trait ExchangeObserver<Symbol: Eq + Hash> {
    // Appropriate fields for your observer:
    //
    // watching_symbols: Vec<Symbol>;
    // symbols_maptree: HashMap<String, Vec<OrderedExchangeSymbol>>;
    // price_table: Arc<HashMap<Symbol, Arc<Mutex<ExchangeValues>>>>;
    // is_running_table: Arc<HashMap<Symbol, AtomicBool>>;
    // async_runner: Runtime;

    /// Get all pools in which this symbol appears, very useful for most strategies
    fn get_interchanged_symbols(&self, symbol: &String) -> &'_ Vec<OrderedExchangeSymbol<Symbol>>;

    /// Adds price to the monitor
    fn add_price_to_monitor(&mut self, symbol: &Symbol, price: &Arc<Mutex<ExchangeValues>>);

    /// Fetches price on certain symbol from the observer
    fn get_price_from_table(&self, symbol: &Symbol) -> Option<&Arc<Mutex<ExchangeValues>>>;

    /// Initialize the runtime, if observer requires one
    fn start(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    /// Allegedly remove symbol from watching table, if your observer has one, if not,
    /// this might be an nop
    fn remove_symbol(&mut self, symbol: Symbol);

    /// Returns value of certain token to usd if available
    fn get_usd_value(&self, sym: String) -> Option<f64>;

    /// Returns the reference to vector of symbols that are being watched
    fn get_watching_symbols(&self) -> &'_ Vec<Symbol>;
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum ExchangeObserverKind {
    Binance,
    Bitfinex,
    Bitmex,
    Bittrex,
    Coinbase,
    Deribit,
    Ftx,
    Huobi,
    Kraken,
    Okex,
    Poloniex,
    Uniswap,
    Unknown,
}

#[derive(Debug)]
pub struct ExchangeBalance {
    pub asset: String,
    pub free: f64,
    pub locked: f64,
}

impl ExchangeBalance {
    pub fn new(asset: String, free: f64, locked: f64) -> Self {
        Self {
            asset: asset,
            free: free,
            locked: locked,
        }
    }
}

pub trait ExchangeClient<Symbol: Eq + Hash> {
    /// Checks if symbol exists on the exchange
    fn symbol_exists(&self, symbol: &Symbol) -> bool;

    /// Fetches the balance of the current logged in user
    fn get_balance(&self, asset: &String) -> Option<ExchangeBalance>;

    /// Makes buy order on the exchange
    fn buy_order(&self, symbol: &Symbol, qty: f64, price: f64);

    /// Makes sell order on the exchange
    fn sell_order(&self, symbol: &Symbol, qty: f64, price: f64);
}

impl Into<ExchangeBalance> for BinanceBalance {
    fn into(self) -> ExchangeBalance {
        ExchangeBalance::new(
            self.asset,
            self.free.parse::<f64>().unwrap(),
            self.locked.parse::<f64>().unwrap(),
        )
    }
}
