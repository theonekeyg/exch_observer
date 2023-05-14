use binance::{account::OrderSide, errors::Result as BResult, model::Balance as BinanceBalance};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Debug,
    fmt::{self, Display, Formatter},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
    str::FromStr
};

use crate::utils::get_current_timestamp;

/// Base trait for a symbol of the exchange pair.
pub trait PairedExchangeSymbol {
    /// Returns the base token in the symbol (e.g. `eth` in `ethbtc`)
    fn base(&self) -> &str;
    /// Returns the quote token in the symbol (e.g. `btc` in `ethbtc`)
    fn quote(&self) -> &str;
}

/// Very basic symbol structure, used in the observer RPC server.
/// For more complicated applications (such as trading bots or information-gathering tools)
/// using more complex structure might be more convenient.
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

/// SwapOrder represents buy/sell orders.
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

/// OrderedExchangeSymbol is a symbol with a specific order (buy/sell).
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

    /// Returns the output token of the possible swap
    /// (e.g. `eth` in `ethbtc` if the order is `Buy`)
    pub fn get_output_symbol(&self) -> String {
        let rv = match self.order {
            SwapOrder::Buy => self.symbol.base().into(),
            SwapOrder::Sell => self.symbol.quote().into(),
        };

        rv
    }

    /// Returns the input token of the possible swap
    /// (e.g. `btc` in `ethbtc` if the order is `Buy`)
    pub fn get_input_symbol(&self) -> String {
        let rv = match self.order {
            SwapOrder::Buy => self.symbol.quote().into(),
            SwapOrder::Sell => self.symbol.base().into(),
        };

        rv
    }
}

/// Trait to represent a type of a current price on the exchange
pub trait ExchangeValues {
    /// Values that the price is represented with
    type Values = (f64, f64);

    /// Updates the price with the new values
    fn update_price(&mut self, price: Self::Values);

    /// Returns the current best ask price
    fn get_ask_price(&self) -> f64;

    /// Returns the current best bid price
    fn get_bid_price(&self) -> f64;

    /// Returns true if value is initialized at least once
    fn is_initialized(&self) -> bool;

    /// Returns showable price in single float value format.
    fn showable_price(&self) -> f64;

    /// Returns the timestamp of the last update
    fn update_timestamp(&self) -> u64;
}

/// Very basic structure to store a price as a single value
#[derive(Debug, Clone)]
pub struct ExchangeSingleValues {
    pub base_price: f64,
    pub update_timestamp: u64
}

unsafe impl Send for ExchangeSingleValues {}
unsafe impl Sync for ExchangeSingleValues {}

impl ExchangeSingleValues {
    /// Create new ExchangeSingleValues with price set to 0.
    pub fn new() -> Self {
        Self {
            base_price: 0.0,
            update_timestamp: get_current_timestamp().unwrap()
        }
    }

    /// Create new ExchangeSingleValues with price set to `base_price`.
    pub fn new_with_prices(base_price: f64) -> Self {
        Self {
            base_price: base_price,
            update_timestamp: get_current_timestamp().unwrap()
        }
    }
}

impl ExchangeValues for ExchangeSingleValues {
    type Values = f64;

    fn update_price(&mut self, price: Self::Values) {
        self.base_price = price;
        self.update_timestamp = get_current_timestamp().unwrap();
    }

    fn get_ask_price(&self) -> f64 {
        self.base_price
    }

    fn get_bid_price(&self) -> f64 {
        self.base_price
    }

    fn is_initialized(&self) -> bool {
        self.base_price != 0.0
    }

    fn showable_price(&self) -> f64 {
        self.base_price
    }

    fn update_timestamp(&self) -> u64 {
        self.update_timestamp
    }
}

/// Structure to store a price as a pair of values (ask/bid).
/// Value it stores is best ask/bid price currently in the exchange pair.
/// This structure is used by default in all observers.
#[derive(Debug, Clone)]
pub struct AskBidValues {
    pub ask_price: f64,
    pub bid_price: f64,
    pub update_timestamp: u64
}
unsafe impl Send for AskBidValues {}
unsafe impl Sync for AskBidValues {}

impl AskBidValues {
    /// Creates new AskBidValues with prices set to 0.
    pub fn new() -> Self {
        Self {
            ask_price: 0.0,
            bid_price: 0.0,
            update_timestamp: 0
        }
    }

    /// Creates new AskBidValues with prices set to `ask_price` and `bid_price`.
    pub fn new_with_prices(ask_price: f64, bid_price: f64) -> Self {
        Self {
            ask_price: ask_price,
            bid_price: bid_price,
            update_timestamp: get_current_timestamp().unwrap()
        }
    }
}

impl ExchangeValues for AskBidValues {
    type Values = (f64, f64);

    fn update_price(&mut self, price: Self::Values) {
        self.ask_price = price.0;
        self.bid_price = price.1;
        self.update_timestamp = get_current_timestamp().unwrap();
    }

    fn get_ask_price(&self) -> f64 {
        self.ask_price
    }

    fn get_bid_price(&self) -> f64 {
        self.bid_price
    }

    fn is_initialized(&self) -> bool {
        self.update_timestamp != 0
    }

    fn showable_price(&self) -> f64 {
        (self.ask_price + self.bid_price) / 2.0
    }

    fn update_timestamp(&self) -> u64 {
        self.update_timestamp
    }
}

/// Trait to represent an observer of an exchange. All observers must implement this trait.
pub trait ExchangeObserver<Symbol: Eq + Hash> {
    // Appropriate fields for your observer:
    //
    // watching_symbols: Vec<Symbol>;
    // symbols_maptree: HashMap<String, Vec<OrderedExchangeSymbol>>;
    // price_table: Arc<HashMap<Symbol, Arc<Mutex<Values>>>>;
    // is_running_table: Arc<HashMap<Symbol, AtomicBool>>;
    // async_runner: Runtime;

    type Values: ExchangeValues;

    /// Get all pools in which this symbol appears, very useful for most strategies
    fn get_interchanged_symbols(&self, symbol: &String) -> &'_ Vec<OrderedExchangeSymbol<Symbol>>;

    /// Adds price to the monitor
    fn add_price_to_monitor(&mut self, symbol: &Symbol, price: Arc<Mutex<Self::Values>>);

    /// Fetches price on certain symbol from the observer
    fn get_price_from_table(&self, symbol: &Symbol) -> Option<&Arc<Mutex<Self::Values>>>;

    /// Initialize the runtime, if observer requires one
    fn start(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    /// Allegedly remove symbol from watching table, if your observer has one, if not,
    /// this might be an nop
    fn remove_symbol(&mut self, symbol: Symbol);

    /// Returns value of certain token to usd if available
    fn get_usd_value(&self, sym: &String) -> Option<f64>;

    /// Returns the reference to vector of symbols that are being watched
    fn get_watching_symbols(&self) -> &'_ Vec<Symbol>;
}

/// Enum to represent an exchange type
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

impl ExchangeObserverKind {
    /// Serializes ExchangeObserverKind to string
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Binance => "binance",
            Self::Bitfinex => "bitfinex",
            Self::Bitmex => "bitmex",
            Self::Bittrex => "bittrex",
            Self::Coinbase => "coinbase",
            Self::Deribit => "deribit",
            Self::Ftx => "ftx",
            Self::Huobi => "huobi",
            Self::Kraken => "kraken",
            Self::Okex => "okex",
            Self::Poloniex => "poloniex",
            Self::Uniswap => "uniswap",
            Self::Unknown => "unknown",
        }
    }
}

impl FromStr for ExchangeObserverKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "binance" => Self::Binance,
            "bitfinex" => Self::Bitfinex,
            "bitmex" => Self::Bitmex,
            "bittrex" => Self::Bittrex,
            "coinbase" => Self::Coinbase,
            "deribit" => Self::Deribit,
            "ftx" => Self::Ftx,
            "huobi" => Self::Huobi,
            "kraken" => Self::Kraken,
            "okex" => Self::Okex,
            "poloniex" => Self::Poloniex,
            "uniswap" => Self::Uniswap,
            _ => Self::Unknown,
        })
    }
}

/// Structure to represent balance on the exchange.
/// Not used in observer, but often used in exchange clients.
#[derive(Debug, Serialize, Deserialize, Clone)]
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

/// Trait to represent an exchange client. All exchange clients must implement this trait.
pub trait ExchangeClient<Symbol: Eq + Hash> {
    /// Checks if symbol exists on the exchange
    fn symbol_exists(&self, symbol: &Symbol) -> bool;

    /// Fetches the balance of the current logged in user
    fn get_balance(&self, asset: &String) -> Option<ExchangeBalance>;

    /// Makes buy order on the exchange
    fn buy_order(&self, symbol: &Symbol, qty: f64, price: f64);

    /// Makes sell order on the exchange
    fn sell_order(&self, symbol: &Symbol, qty: f64, price: f64);

    fn get_balances(&self) -> BResult<HashMap<String, ExchangeBalance>>;
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

