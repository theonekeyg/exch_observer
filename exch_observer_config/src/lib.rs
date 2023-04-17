use std::{default::Default, fs::File, io::Read};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct BinanceConfig {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub symbols_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpcConfig {
    pub port: Option<u16>,
    pub host: Option<String>,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            port: Some(51011),
            host: Some("127.0.0.1".to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ObserverConfig {
    pub binance: Option<BinanceConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExchObserverConfig {
    pub num_threads: Option<usize>,
    pub observer: ObserverConfig,
    pub rpc: Option<RpcConfig>,
}

impl ExchObserverConfig {
    pub fn new() -> Self {
        Self {
            num_threads: None,
            observer: ObserverConfig::default(),
            rpc: None,
        }
    }

    pub fn parse_config(config_path: String) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file = File::open(config_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let mut config: Self = toml::from_str(&contents)?;

        if config.rpc.is_none() {
            config.rpc = Some(RpcConfig::default());
        }

        Ok(config)
    }
}
