use exch_observer_config::{ExchObserverConfig, BinanceConfig};

#[test]
fn test_config_parsing() {
    let config =
        ExchObserverConfig::parse_config("tests/assets/valid_config.toml".to_string())
        .unwrap();
    println!("{:#?}", config);

    assert!(config.observer.binance.is_some());

    let binance_config = config.observer.binance.unwrap();
    assert_eq!(config.num_threads, Some(4));
    assert_eq!(binance_config.symbols_path, "./assets/binance_symbols.csv");
}
