use exch_observer_types::{ExchangeSymbol, OrderedExchangeSymbol, PairedExchangeSymbol, SwapOrder};

#[test]
fn test_exchange_symbol() {
    let symbol = ExchangeSymbol::new("eth", "btc");
    assert_eq!(symbol.base(), "eth");
    assert_eq!(symbol.quote(), "btc");
    assert_eq!(symbol.to_string(), "ethbtc");
}

#[test]
fn test_ordered_exchange_symbol_ouput_symbol() {
    let base_symbol = ExchangeSymbol::new("btc", "usdt");
    let ordered_symbol = OrderedExchangeSymbol::new(&base_symbol, SwapOrder::Buy);

    assert_eq!(ordered_symbol.get_output_symbol(), "btc");
    assert_eq!(ordered_symbol.get_input_symbol(), "usdt");
}
