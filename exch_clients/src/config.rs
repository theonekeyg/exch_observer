
#[derive(Debug, Clone)]
struct HuobiClientConfig {
    /// API key for Huobi (HUOBI_API_KEY)
    api_key: Option<String>,
    /// API secret for Huobi (HUOBI_API_SECRET)
    api_secret: Option<String>,
}

#[derive(Debug, Clone)]
struct KrakenClientConfig {
    /// API key for Kraken (KRAKEN_API_KEY)
    api_key: String,
    /// API secret for Kraken (KRAKEN_API_SECRET)
    api_secret: String,
}

#[derive(Debug, Clone)]
struct BinanceClientConfig {
    /// API key for Binance (BINANCE_API_KEY)
    api_key: Option<String>,
    /// API secret for Binance (BINANCE_API_SECRET)
    api_secret: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Binance client configuration
    binance: Option<BinanceClientConfig>,
    /// Kraken client configuration
    kraken: Option<KrakenClientConfig>,
    /// Huobi client configuration
    huobi: Option<HuobiClientConfig>,
}

impl ClientConfig {
    /// Create ClientConfig from environment variables.
    /// Variable names are listed in individual structs.
    pub fn from_env() -> ClientConfig {
        let binance_api_key = std::env::var("BINANCE_API_KEY");
        let binance_api_secret = std::env::var("BINANCE_API_SECRET");
        let binance_config = Some(BinanceClientConfig {
            api_key: binance_api_key.ok(),
            api_secret: binance_api_secret.ok(),
        });

        let kraken_api_key = std::env::var("KRAKEN_API_KEY");
        let kraken_api_secret = std::env::var("KRAKEN_API_SECRET");
        let kraken_config = if kraken_api_key.is_ok() && kraken_api_secret.is_ok() {
            Some(KrakenClientConfig {
                api_key: kraken_api_key.unwrap(),
                api_secret: kraken_api_secret.unwrap(),
            })
        } else {
            None
        };

        let huobi_api_key = std::env::var("HUOBI_API_KEY");
        let huobi_api_secret = std::env::var("HUOBI_API_SECRET");
        let huobi_config = Some(HuobiClientConfig {
            api_key: huobi_api_key.ok(),
            api_secret: huobi_api_secret.ok(),
        });

        ClientConfig {
            binance: binance_config,
            kraken: kraken_config,
            huobi: huobi_config,
        }
    }
}
