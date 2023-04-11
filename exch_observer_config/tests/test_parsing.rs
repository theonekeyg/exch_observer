use exch_observer_config::{ObserverConfig, BinanceConfig};

#[test]
fn test_config_parsing() {
    let config =
        ObserverConfig::parse_config("tests/assets/valid_config.toml".to_string())
        .unwrap();
    println!("{:#?}", config);

    assert!(config.binance.is_some());

    let binance_config = config.binance.unwrap();
    assert_eq!(binance_config.num_threads, Some(4));
    assert_eq!(binance_config.symbols_path, "./assets/binance_symbols.csv");
}
