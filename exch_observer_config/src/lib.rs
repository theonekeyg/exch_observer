use std::{default::Default, fs::File, io::Read, path::Path};

use serde::Deserialize;

/// Struct representing the configuration for the Binance observer
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceConfig {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    /// Path to .csv file containing symbols to monitor
    pub symbols_path: String,
}

/// Struct representing the configuration for the Huobi observer
#[derive(Debug, Clone, Deserialize)]
pub struct HuobiConfig {
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    /// Path to .csv file containing symbols to monitor
    pub symbols_path: String,
}

/// Struct for configuring the RPC server/client
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

/// Struct representing the configuration for the observer,
/// currently contains only the configuration for inner-observers.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ObserverConfig {
    pub binance: Option<BinanceConfig>,
    pub huobi: Option<HuobiConfig>,
}

/// Struct representing the configuration for the `exch_observer` binary
#[derive(Debug, Clone, Deserialize)]
pub struct ExchObserverConfig {
    /// Maximum number of threads the observer can use
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

    /// Parses .toml configuration file from provided path
    pub fn parse_config<P: AsRef<Path>>(
        config_path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
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
