use binance::{
    account::OrderSide,
    model::{Balance as BinanceBalance, Filters as BFilters, Symbol as BSymbol},
};
use exch_observer_utils::get_current_timestamp;
use krakenrs::AssetPair;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Debug,
    fmt::{self, Display, Formatter},
    hash::{Hash, Hasher},
    str::FromStr,
    sync::{Arc, Mutex},
};

pub static USD_STABLES: [&str; 4] = ["usdt", "usdc", "busd", "dai"];

/// Base trait for a symbol of the exchange pair.
pub trait PairedExchangeSymbol {
    /// Returns the base token in the symbol (e.g. `eth` in `ethbtc`)
    fn base(&self) -> &str;
    /// Returns the quote token in the symbol (e.g. `btc` in `ethbtc`)
    fn quote(&self) -> &str;
    /// Returns the name of the trading pair
    /// (e.g. `ethbtc` on Binance, `XXBTZUSD` on Kraken)
    fn pair(&self) -> String;
}

/// Very basic symbol structure, used in the observer RPC server.
/// For more complicated applications (such as trading bots or information-gathering tools)
/// using more complex structure might be more convenient.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeSymbol {
    pub base: String,
    pub quote: String,
}

impl From<BSymbol> for ExchangeSymbol {
    fn from(symbol: BSymbol) -> Self {
        Self {
            base: symbol.base_asset.clone(),
            quote: symbol.quote_asset,
        }
    }
}

impl PairedExchangeSymbol for ExchangeSymbol {
    fn base(&self) -> &str {
        &self.base
    }

    fn quote(&self) -> &str {
        &self.quote
    }

    fn pair(&self) -> String {
        self.into()
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

/// Generic symbol on the exchange. Meant to have everything that
/// any exchange would have.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct ArbitrageExchangeSymbol {
    /// Base and Quote tokens
    pub inner: ExchangeSymbol,
    /// Pair name (e.g. `ethbtc` on Binance, `XXBTZUSD` on Kraken)
    /// mostly used to correct Websocket connections
    pub pair_name: String,
    /// Minimum price for the symbol
    pub min_price: Decimal,
    /// Precision of a base symbol
    pub base_precision: u8,
    /// Precision of a quote symbol for a bid
    pub qty_step_size: Decimal,
    /// Precision of price for a bid
    pub price_tick_size: Decimal,
    /// Minimum notional value for a symbol
    pub min_notional: Decimal,
    /// Minimum quantity for a symbol
    pub min_qty: Decimal,
}

impl PairedExchangeSymbol for ArbitrageExchangeSymbol {
    fn base(&self) -> &str {
        self.inner.base()
    }

    fn quote(&self) -> &str {
        self.inner.quote()
    }

    fn pair(&self) -> String {
        self.pair_name.clone()
    }
}

impl Into<String> for ArbitrageExchangeSymbol {
    fn into(self) -> String {
        self.inner.into()
    }
}

impl Into<String> for &ArbitrageExchangeSymbol {
    fn into(self) -> String {
        self.inner.to_string()
    }
}

impl From<BSymbol> for ArbitrageExchangeSymbol {
    /// Converts a Binance symbol into an ArbitrageExchangeSymbol
    fn from(symbol: BSymbol) -> Self {
        let base_asset = symbol.base_asset;
        let quote_asset = symbol.quote_asset;
        let inner = ExchangeSymbol::new(base_asset, quote_asset);
        let pair_name = symbol.symbol;
        let mut min_price = dec!(0.00000000);
        let base_precision = symbol.base_asset_precision as u8;
        let mut qty_step_size = dec!(0.00000001);
        let mut price_tick_size = dec!(0.00000001);
        let mut min_notional = dec!(0.00000001);
        let mut min_qty = dec!(0.00000001);

        // On binance, most limits are enforced by filters.
        for filt in symbol.filters {
            match filt {
                BFilters::PriceFilter {
                    min_price: min,
                    max_price: _,
                    tick_size,
                } => {
                    min_price = Decimal::from_str(&min).unwrap();
                    price_tick_size = Decimal::from_str(&tick_size).unwrap();
                }
                BFilters::LotSize {
                    min_qty: min,
                    max_qty: _,
                    step_size,
                } => {
                    min_qty = Decimal::from_str(&min).unwrap();
                    qty_step_size = Decimal::from_str(&step_size).unwrap();
                }
                BFilters::MinNotional {
                    notional: _,
                    min_notional: min,
                    apply_to_market: _,
                    avg_price_mins: _,
                } => {
                    min_notional = Decimal::from_str(&min.unwrap()).unwrap();
                }
                _ => {}
            }
        }

        Self {
            inner: inner,
            pair_name: pair_name,
            min_price: min_price,
            base_precision: base_precision,
            qty_step_size: qty_step_size,
            price_tick_size: price_tick_size,
            min_notional: min_notional,
            min_qty: min_qty,
        }
    }
}

impl From<AssetPair> for ArbitrageExchangeSymbol {
    fn from(symbol: AssetPair) -> Self {
        // Split symbol.wsname into base and quote, since we don't need the wsname
        let wsname = symbol.wsname.unwrap();
        let mut split = wsname.split('/');
        let base_asset = split.next().unwrap();
        let quote_asset = split.next().unwrap();
        let min_price = symbol.costmin.unwrap();
        let base_precision: u8 = symbol.cost_decimals.try_into().unwrap();
        let qty_step_size = 1 / ((10 as u64).pow(symbol.pair_decimals.try_into().unwrap()));
        let price_tick_size = symbol.tick_size.unwrap();
        let min_notional = symbol.costmin.unwrap();
        let min_qty = symbol.ordermin.unwrap();

        Self::new(
            base_asset,
            quote_asset,
            &wsname,
            min_price,
            base_precision,
            qty_step_size.into(),
            price_tick_size,
            min_notional,
            min_qty,
        )
    }
}

impl ArbitrageExchangeSymbol {
    pub fn new<S>(
        base: S,
        quote: S,
        pair_name: S,
        min_price: Decimal,
        base_precision: u8,
        qty_step_size: Decimal,
        price_tick_size: Decimal,
        min_notional: Decimal,
        min_qty: Decimal,
    ) -> Self
    where
        S: Into<String>,
    {
        Self {
            inner: ExchangeSymbol::new(base, quote),
            pair_name: pair_name.into(),
            min_price: min_price,
            base_precision: base_precision,
            qty_step_size: qty_step_size,
            price_tick_size: price_tick_size,
            min_notional: min_notional,
            min_qty: min_qty,
        }
    }
}

impl PartialEq for ArbitrageExchangeSymbol {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Display for ArbitrageExchangeSymbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "{}", Into::<String>::into(self))
    }
}

impl Eq for ArbitrageExchangeSymbol {}

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

    /// Uninitializes the value
    fn uninitialize(&mut self);

    /// Returns showable price in single float value format.
    fn showable_price(&self) -> f64;

    /// Returns the timestamp of the last update
    fn update_timestamp(&self) -> u64;
}

/// Very basic structure to store a price as a single value
#[derive(Debug, Clone)]
pub struct ExchangeSingleValues {
    pub base_price: f64,
    pub update_timestamp: u64,
}

unsafe impl Send for ExchangeSingleValues {}
unsafe impl Sync for ExchangeSingleValues {}

impl ExchangeSingleValues {
    /// Create new ExchangeSingleValues with price set to 0.
    pub fn new() -> Self {
        Self {
            base_price: 0.0,
            update_timestamp: 0,
        }
    }

    /// Create new ExchangeSingleValues with price set to `base_price`.
    pub fn new_with_prices(base_price: f64) -> Self {
        Self {
            base_price: base_price,
            update_timestamp: get_current_timestamp().unwrap(),
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
        self.update_timestamp != 0
    }

    fn uninitialize(&mut self) {
        self.base_price = 0.0;
        self.update_timestamp = 0;
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
    pub update_timestamp: u64,
}
unsafe impl Send for AskBidValues {}
unsafe impl Sync for AskBidValues {}

impl AskBidValues {
    /// Creates new AskBidValues with prices set to 0.
    pub fn new() -> Self {
        Self {
            ask_price: 0.0,
            bid_price: 0.0,
            update_timestamp: 0,
        }
    }

    /// Creates new AskBidValues with prices set to `ask_price` and `bid_price`.
    pub fn new_with_prices(ask_price: f64, bid_price: f64) -> Self {
        Self {
            ask_price: ask_price,
            bid_price: bid_price,
            update_timestamp: get_current_timestamp().unwrap(),
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

    fn uninitialize(&mut self) {
        self.ask_price = 0.0;
        self.bid_price = 0.0;
        self.update_timestamp = 0;
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

    /// Starts the threads that will ping existing threads, if some handle appears to be dead, it
    /// will be removed from the observer
    // fn spawn_await_on_handles(

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
        Ok(match s.to_lowercase().as_str() {
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

    /// Fetches balances for the current user whose api key is used
    fn get_balances(&self) -> Result<HashMap<String, ExchangeBalance>, Box<dyn std::error::Error>>;

    /// Fetches all symbols from the exchange and returns list of symbols
    fn fetch_symbols(&self) -> Result<Vec<Symbol>, Box<dyn std::error::Error>>;
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
