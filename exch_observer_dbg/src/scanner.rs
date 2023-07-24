use csv::Reader;
use exch_observer_config::{ExchObserverConfig, ObserverConfig};
use exch_observer_rpc::ObserverRpcClient;
use exch_observer_types::{ExchangeKind, ExchangeSymbol};
use std::path::Path;

/// SymbolScanner iteratres all the symbols in the observer config
/// and checks if the price is initialized
pub struct SymbolScanner {
    pub config: ObserverConfig,
    pub rpc: ObserverRpcClient,
}

impl SymbolScanner {
    pub async fn new(config: ExchObserverConfig) -> Self {
        let rpc_config = config.rpc.clone().unwrap_or_default();
        let rpc = ObserverRpcClient::new(rpc_config).await;

        Self {
            config: config.observer,
            rpc: rpc,
        }
    }

    /// Reads the symbols from the provided symbols_path and fetches prices for them
    /// from RPC API for the provided observer kind
    pub async fn scan_from_csv<P: AsRef<Path>>(
        &mut self,
        observer: ExchangeKind,
        symbols_path: P,
        mut f: impl FnMut(ExchangeSymbol, f64, u64),
    ) {
        let mut reader = Reader::from_path(symbols_path).unwrap();
        for record in reader.records() {
            let record = record.unwrap();
            let base = record.get(0).unwrap();
            let quote = record.get(1).unwrap();

            let (price, timestamp) = self
                .rpc
                .get_price_with_timestamp(observer.to_str(), base, quote)
                .await;

            let symbol = ExchangeSymbol::new(base, quote);
            f(symbol, price, timestamp);
        }
    }

    /// Executes the scan on provided observer with values from the config
    pub async fn run_with_obs(
        &mut self,
        observer: ExchangeKind,
        f: impl FnMut(ExchangeSymbol, f64, u64),
    ) {
        match &observer {
            &ExchangeKind::Binance => {
                let symbols_path = if let Some(binance_config) = &self.config.binance {
                    binance_config.symbols_path.clone()
                } else {
                    panic!("No binance config found, but was requested");
                };

                self.scan_from_csv(ExchangeKind::Binance, symbols_path, f)
                    .await;
            }
            &ExchangeKind::Huobi => {
                let symbols_path = if let Some(huobi_config) = &self.config.huobi {
                    huobi_config.symbols_path.clone()
                } else {
                    panic!("No huobi config found, but was requested");
                };

                self.scan_from_csv(ExchangeKind::Huobi, symbols_path, f)
                    .await;
            }
            &ExchangeKind::Kraken => {
                let symbols_path = if let Some(kraken_config) = &self.config.kraken {
                    kraken_config.symbols_path.clone()
                } else {
                    panic!("No kraken config found, but was requested");
                };

                self.scan_from_csv(ExchangeKind::Kraken, symbols_path, f)
                    .await;
            }
            _ => todo!("Please implement us!!! (handlers for other observers)"),
        };
    }
}
