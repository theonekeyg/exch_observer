use crate::{ExchangeKind, PairedExchangeSymbol};

fn huobi_symbol(symbol: impl PairedExchangeSymbol) -> String {
    format!("{}{}", symbol.base(), symbol.quote())
}

fn binance_symbol(symbol: impl PairedExchangeSymbol) -> String {
    format!("{}{}", symbol.base(), symbol.quote())
}

fn kraken_symbol(symbol: impl PairedExchangeSymbol) -> String {
    format!("{}/{}", symbol.base(), symbol.quote())
}

pub fn format_symbol<Symbol: PairedExchangeSymbol>(
    symbol: Symbol,
    exchange: ExchangeKind,
) -> String {
    match exchange {
        ExchangeKind::Binance => binance_symbol(symbol),
        ExchangeKind::Huobi => huobi_symbol(symbol),
        ExchangeKind::Kraken => kraken_symbol(symbol),
        _ => unimplemented!(),
    }
}
