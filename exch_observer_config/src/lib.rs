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
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct RpcConfig {
    pub port: Option<u16>,
    pub host: Option<String>
}

#[derive(Debug, Deserialize)]
pub struct ObserverConfig {
    pub num_threads: Option<usize>,
    pub binance: Option<BinanceConfig>,
    pub rpc: Option<RpcConfig>
}

impl ObserverConfig {
    pub fn new() -> Self {
        Self {
            num_threads: None,
            binance: None,
            rpc: None
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
