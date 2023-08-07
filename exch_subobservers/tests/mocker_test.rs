use std::{
    sync::{Arc, Mutex}
};
use exch_observer_types::{
    ExchangeKind, ExchangeSymbol, AskBidValues, ExchangeObserver, ExchangeValues,
    PairedExchangeSymbol, SwapOrder, OrderedExchangeSymbol
};
use exch_observer_config::{
    ObserverConfig
};
use exch_subobservers::{CombinedObserver, MockerObserver};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

fn get_runtime() -> Runtime {
    RuntimeBuilder::new_multi_thread()
        .worker_threads(4)
        .enable_io()
        .build()
        .unwrap()
}

fn create_ask_bid_values() -> (f64, f64, Arc<Mutex<AskBidValues>>) {
    let ask_price = rand::random::<f64>();
    let bid_price = ask_price / 2.0;
    (
        ask_price, bid_price,
        Arc::new(Mutex::new(AskBidValues::new_with_prices(ask_price, bid_price)))
    )
}

enum ObserverWrapper {
    Combined(CombinedObserver<ExchangeSymbol>),
    Mocker(Box<dyn ExchangeObserver<ExchangeSymbol, Values = AskBidValues>>),
}

impl ObserverWrapper {
    pub fn add_price_to_monitor(&mut self, symbol: &ExchangeSymbol, price: Arc<Mutex<AskBidValues>>) {
        match self {
            ObserverWrapper::Combined(combined) => {
                combined.add_price_to_monitor(symbol, price);
            },
            ObserverWrapper::Mocker(mocker) => {
                mocker.add_price_to_monitor(symbol, price);
            }
        }
    }
}

/// General implementation to test the observer implementation
/// WARNING: to test remove functionality, it always removes the second element
/// (index 1) in in the provided `symbols_info` array, expect that in the
/// `expected_interchanged_symbols`
///
/// `Arguments`:
///
///  * symbols_info - Vector of tuples containing the symbol, ask price, bid price and 
///    the Arc<Mutex<AskBidValues>> to be used for the observer
///
///  * `interchange_str` - String of the symbol to be used for the interchanged symbol
///
///  * `expected_interchanged_symbols` - Vector of OrderedExchangeSymbol that should be
///    the result of the interchanged symbols
///
fn observer_tester_impl(
    mut observer: Box<dyn ExchangeObserver<ExchangeSymbol, Values = AskBidValues>>,
    symbols_info: Vec<(ExchangeSymbol, f64, f64, Arc<Mutex<AskBidValues>>)>,
    interchange_str: &String,
    expected_interchanged_symbols: Vec<OrderedExchangeSymbol<ExchangeSymbol>>,
    str_to_get_usd_value_for: &String,
    expected_usd_value: f64,
) {

    let mut symbols = Vec::with_capacity(symbols_info.len());
    // Add provided symbols info to the observer, meanwhile testing it
    for (symbol, ask_price, bid_price, price) in &symbols_info {

        // Add price and symbol to monitor
        observer.add_price_to_monitor(&symbol.clone(), price.clone());

        // Create another scope to drop the mutex lock on price right after
        // we're done with it
        {
            let price_bind = observer
                .get_price_from_table(symbol)
                .unwrap();

            let price_from_obs = price_bind
                .lock()
                .unwrap();

            // Assert ask and bid prices are the same as the ones we set
            assert_eq!(
                price_from_obs.get_ask_price(),
                *ask_price,
            );
            assert_eq!(
                price_from_obs.get_bid_price(),
                *bid_price,
            );

            // Also push symbol to symbols vector
            symbols.push(symbol.clone());
        }
    }

    // Assert watching symbols are the same as the ones we added
    assert_eq!(
        observer.get_watching_symbols(),
        &symbols,
    );

    observer.start().unwrap();

    // Remove the second symbol
    observer.remove_symbol(symbols_info[1].0.clone());
    symbols.remove(1);

    // Test `get_watching_symbols`.
    // Assert watching symbols are the same as the ones we added
    assert_eq!(
        observer.get_watching_symbols(),
        &symbols,
    );

    // Test `get_usd_value` for eth which is added earlier
    assert_eq!(
        observer.get_usd_value(str_to_get_usd_value_for).unwrap(),
        expected_usd_value,
    );

    // Test `get_interchanged_symbols`
    assert_eq!(
        observer.get_interchanged_symbols(interchange_str),
        &expected_interchanged_symbols,
    );
}

#[test]
fn test_mocker_observer_direct() {
    let mut mocker = MockerObserver::new(Arc::new(get_runtime()));

    // Create initial symbols to test,
    // note that uniusdt will be removed, note that for
    // `expected_interchanged_symbols` in `observer_tester_impl`
    let symbols_to_test = vec![
        ExchangeSymbol::from("eth", "usdt"),
        ExchangeSymbol::from("uni", "usdt"),
        ExchangeSymbol::from("uni", "eth"),
    ];

    // Create the symbols info for each token in `symbols_to_test`
    let mut symbols_infos = Vec::with_capacity(symbols_to_test.len());
    for symbol in &symbols_to_test {
        let (ask_price, bid_price, price) = create_ask_bid_values();
        symbols_infos.push((symbol.clone(), ask_price, bid_price, price));
    }

    // Select eth for usd query
    let token_for_usd_query = String::from("eth");
    // Get expected price
    let token_usd_price = {
        symbols_infos[0].3.lock().unwrap().showable_price()
    };

    // Expected interchanged_symblos for eth token
    let expected_interchanged_symbols = vec![
        OrderedExchangeSymbol::new(
            &ExchangeSymbol::from("eth", "usdt"),
            SwapOrder::Sell,
        ),
        OrderedExchangeSymbol::new(
            &ExchangeSymbol::from("uni", "eth"),
            SwapOrder::Buy,
        ),
    ];

    // Invoke tester
    observer_tester_impl(
        Box::new(mocker),
        symbols_infos,
        &String::from("eth"),
        expected_interchanged_symbols,
        &token_for_usd_query,
        token_usd_price,
    );
}

#[test]
fn test_mocker_observer_through_combined() {
}
