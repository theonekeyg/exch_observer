use std::{
    fs::File,
    io::Read
};
use exch_observer_types::{ExchangeSymbol, ExchangeObserver};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct BinanceConfig {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub symbols_path: String,
    pub num_threads: Option<usize>
}

#[derive(Debug, Deserialize)]
pub struct ObserverConfig {
    pub binance_config: Option<BinanceConfig>
}

impl ObserverConfig {
    pub fn new() -> Self {
        Self {
            binance_config: None
        }
    }

    pub fn parse_config(config_path: String) -> Result<Self, Box<dyn std::error::Error>> {

        let mut file = File::open(config_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }
}
