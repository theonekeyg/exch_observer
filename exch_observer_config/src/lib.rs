use std::{default::Default, fs::File, io::Read, path::Path};

use serde::Deserialize;

/// Struct representing the configuration for the Kraken observer
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenConfig {
    pub allowed_symbols: Option<Vec<String>>,
    /// Path to .csv file containing symbols to monitor
    pub symbols_path: String,
    /// Flag to enable the observer or utilities related with them
    pub enable: bool,
    /// Port for the WS server
    pub ws_port: u16,
}

/// Struct representing the configuration for the Binance observer
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceConfig {
    pub allowed_symbols: Option<Vec<String>>,
    /// Path to .csv file containing symbols to monitor
    pub symbols_path: String,
    /// Flag to enable the observer or utilities related with them
    pub enable: bool,
    /// Port for the WS server
    pub ws_port: u16,
}

/// Struct representing the configuration for the Huobi observer
#[derive(Debug, Clone, Deserialize)]
pub struct HuobiConfig {
    pub allowed_symbols: Option<Vec<String>>,
    /// Path to .csv file containing symbols to monitor
    pub symbols_path: String,
    /// Flag to enable the observer or utilities related with them
    pub enable: bool,
    /// Port for the WS server
    pub ws_port: u16,
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

/// Struct for configuring the WS server.
#[derive(Debug, Clone, Deserialize)]
pub struct WsConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
}

impl Default for WsConfig {
    fn default() -> Self {
        Self {
            host: Some("127.0.0.1".to_string()),
            port: Some(51012),
        }
    }
}

/// Struct representing the configuration for the observer,
/// currently contains only the configuration for inner-observers.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ObserverConfig {
    pub binance: Option<BinanceConfig>,
    pub huobi: Option<HuobiConfig>,
    pub kraken: Option<KrakenConfig>,
}

/// Struct representing the configuration for the `exch_observer` binary
#[derive(Debug, Clone, Deserialize)]
pub struct ExchObserverConfig {
    /// Maximum number of threads the observer can use
    pub num_threads: Option<usize>,
    pub observer: ObserverConfig,
    pub rpc: Option<RpcConfig>,
    pub ws: Option<WsConfig>,
}

impl ExchObserverConfig {
    pub fn new() -> Self {
        Self {
            num_threads: None,
            observer: ObserverConfig::default(),
            rpc: None,
            ws: None,
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

        if config.ws.is_none() {
            config.ws = Some(WsConfig::default());
        }

        Ok(config)
    }
}
