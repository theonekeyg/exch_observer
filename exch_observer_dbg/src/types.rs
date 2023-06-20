use exch_observer_types::{ArbitrageExchangeSymbol, PairedExchangeSymbol};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display, Formatter},
    hash::{Hash, Hasher},
};

/// CSV row of symbol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageExchangeSymbolRow {
    pub base: String,
    pub quiote: String,
    pub pair_name: String,
    pub min_price: Decimal,
    pub base_precision: u8,
    pub qty_step_size: Decimal,
    pub price_tick_size: Decimal,
    pub min_notional: Decimal,
    pub min_qty: Decimal,
}

impl From<ArbitrageExchangeSymbol> for ArbitrageExchangeSymbolRow {
    fn from(symbol: ArbitrageExchangeSymbol) -> Self {
        Self {
            base: symbol.base().to_string(),
            quiote: symbol.quote().to_string(),
            pair_name: symbol.pair_name,
            min_price: symbol.min_price,
            base_precision: symbol.base_precision,
            qty_step_size: symbol.qty_step_size,
            price_tick_size: symbol.price_tick_size,
            min_notional: symbol.min_notional,
            min_qty: symbol.min_qty,
        }
    }
}

impl Into<ArbitrageExchangeSymbol> for ArbitrageExchangeSymbolRow {
    fn into(self) -> ArbitrageExchangeSymbol {
        ArbitrageExchangeSymbol::new(
            self.base,
            self.quiote,
            self.pair_name,
            self.min_price,
            self.base_precision,
            self.qty_step_size,
            self.price_tick_size,
            self.min_notional,
            self.min_qty,
        )
    }
}

pub trait SymbolsParser {
    fn fetch_symbols(&self) -> Vec<ArbitrageExchangeSymbol>;
}
