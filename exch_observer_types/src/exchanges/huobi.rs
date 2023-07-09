use std::{
    hash::Hash,
};
use crate::types::{
    ExchangeSymbol, ArbitrageExchangeSymbol, ExchangeAccount,
    ExchangeAccountState, ExchangeAccountType
};
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;

/// Huobi symbol info.
/// Documentation: https://huobiapi.github.io/docs/spot/v1/en/#get-all-supported-trading-symbol-v1-deprecated.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct HuobiSymbol {
    /// Base currency in a trading symbol
    #[serde(rename = "base-currency")]
    pub base: String,
    #[serde(rename = "quote-currency")]
    /// Quote currency in a trading symbol
    pub quote: String,
    /// Quote currency precision when quote price(decimal places)）
    #[serde(rename = "price-precision")]
    pub price_precision: u8,
    /// Base currency precision when quote amount(decimal places)）
    #[serde(rename = "amount-precision")]
    pub amount_precision: u8,
    /// Trading section, possible values: [main，innovation]
    #[serde(rename = "symbol-partition")]
    pub symbol_partition: String,
    /// symbol
    pub symbol: String,
    /// The status of the symbol；Allowable values: [online, offline, suspend]. "online" - Listed,
    /// available for trading, "offline" - de-listed, not available for trading,
    /// "suspend"-suspended for trading, "pre-online" - to be online soon
    pub state: String,
    /// Precision of value in quote currency (value = price * amount)
    #[serde(rename = "value-precision")]
    pub value_precision: u8,
    /// Minimum order amount of limit order in base currency (to be obsoleted)
    #[serde(rename = "min-order-amt")]
    pub min_order_amt: Decimal,
    ///	Max order amount of limit order in base currency (to be obsoleted)
    #[serde(rename = "max-order-amt")]
    pub max_order_amt: Decimal,
    /// Minimum order value of limit order and buy-market order in quote currency
    #[serde(rename = "min-order-value")]
    pub min_order_value: Decimal,
    /// Minimum order amount of limit order in base currency (NEW)
    #[serde(rename = "limit-order-min-order-amt")]
    pub limit_order_min_order_amt: Decimal,
    /// Max order amount of limit order in base currency (NEW)
    #[serde(rename = "limit-order-max-order-amt")]
    pub limit_order_max_order_amt: Decimal,
    #[serde(rename = "limit-order-max-buy-amt")]
    pub limit_order_max_buy_amt: Decimal,
    #[serde(rename = "limit-order-max-sell-amt")]
    pub limit_order_max_sell_amt: Decimal,
    /// decimal(10,6) Buy limit must less than
    #[serde(rename = "buy-limit-must-less-than")]
    pub buy_limit_must_less_than: Decimal,
    /// decimal(10,6) Sell limit must greater than
    #[serde(rename = "sell-limit-must-greater-than")]
    pub sell_limit_must_greater_than: Decimal,
    /// Minimum order amount of sell-market order in base currency (NEW)
    #[serde(rename = "sell-market-min-order-amt")]
    pub sell_market_min_order_amt: Decimal,
    /// Max order amount of sell-market order in base currency (NEW)
    #[serde(rename = "sell-market-max-order-amt")]
    pub sell_market_max_order_amt: Decimal,
    /// Max order value of buy-market order in quote currency (NEW)
    #[serde(rename = "buy-market-max-order-value")]
    pub buy_market_max_order_value: Decimal,
    ///	decimal(10,6) Market sell order rate must less than
    #[serde(rename = "market-sell-order-rate-must-less-than")]
    pub market_sell_order_rate_must_less_than: Decimal,
    /// decimal(10,6) Market buy order rate must less than
    #[serde(rename = "market-buy-order-rate-must-less-than")]
    pub market_buy_order_rate_must_less_than: Decimal,
    /// API trading enabled or not (possible value: enabled, disabled)
    #[serde(rename = "api-trading")]
    pub api_trading: String,
    /// Tags, multiple tags are separated by commas, such as: st, hadax
    pub tags: Option<String>,
}

impl From<HuobiSymbol> for ExchangeSymbol {
    fn from(symbol: HuobiSymbol) -> Self {
        ExchangeSymbol::from(symbol.base, symbol.quote)
    }
}

impl From<HuobiSymbol> for ArbitrageExchangeSymbol {
    fn from(symbol: HuobiSymbol) -> Self {
        let min_price = 1.0 / 10f64.powf(symbol.price_precision as f64);
        let qty_step_size = 1.0 / 10f64.powf(symbol.amount_precision as f64);
        ArbitrageExchangeSymbol::new(
            symbol.base,
            symbol.quote,
            symbol.symbol,
            min_price.try_into().expect("Error converting min_price to Decimal"),
            symbol.amount_precision,
            qty_step_size.try_into().expect("Error converting qty_step_size to Decimal"),
            min_price.try_into().expect("Error converting min_price to Decimal"),
            symbol.min_order_value.try_into().expect("Error converting min_order_value to Decimal"),
            symbol.limit_order_min_order_amt.try_into().expect("Error converting limit_order_min_order_amt to Decimal"),
        )
    }
}

/// Respose for `/v1/account/accounts` API
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct HuobiAccount {
    /// Unique account id
    pub id: u64,
    /// The type of this account
    #[serde(rename = "type")]
    pub type_field: String,
    /// The type of sub account (applicable only for isolated margin accout)
    pub subtype: String,
    /// Account state
    pub state: String
}

impl Into<ExchangeAccount> for &HuobiAccount {
    fn into(self) -> ExchangeAccount {
        let account_type = match self.type_field.as_str() {
            "spot" => ExchangeAccountType::Spot,
            "margin" => ExchangeAccountType::Margin,
            "otc" => ExchangeAccountType::Otc,
            "point" => ExchangeAccountType::Point,
            _ => ExchangeAccountType::Unknown,
        };

        let account_state = match self.state.as_str() {
            "working" => ExchangeAccountState::Working,
            "lock" => ExchangeAccountState::Locked,
            _ => ExchangeAccountState::Unknown,
        };

        ExchangeAccount {
            id: self.id.to_string(),
            account_type,
            state: account_state,
        }
    }
}

/// Respose for `/v1/account/accounts` API
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct HuobiAccountsResponse {
    pub status: String,
    pub data: Vec<HuobiAccount>,
}

// #[derive(Debug, Clone, Hash, Serialize, Deserialize)]
// pub struct HuobiAccountBalance {
// }

/// Respose for `/v1/account/accounts/{account-id}/balance` API
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct HuobiAccountBalanceResponse {
    pub status: String,
    pub data: HuobiAccountBalance,
}
