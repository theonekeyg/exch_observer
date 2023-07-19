use crate::{
    binance_client::BinanceClient, config::CombinedClientConfig, huobi_client::HuobiClient,
    kraken_client::KrakenClient,
};
use anyhow::Result;
use binance::model::Symbol as BSymbol;
use exch_observer_types::{
    exchanges::huobi::HuobiSymbol, ExchangeBalance, ExchangeClient, ExchangeObserverKind,
    PairedExchangeSymbol,
};
use krakenrs::AssetPair;
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
};

/// CombinedClient is a helper struct to interact with multiple exchanges
/// through a single entity
pub struct CombinedClient<Symbol: Eq + Hash> {
    /// Internal map of clients for each exchange
    clients: HashMap<ExchangeObserverKind, Box<dyn ExchangeClient<Symbol>>>,
    /// Configuration for the combined client, used to enable clients
    config: CombinedClientConfig,
}

impl<Symbol> CombinedClient<Symbol>
where
    Symbol: Eq
        + Hash
        + Clone
        + Into<String>
        + From<BSymbol>
        + From<AssetPair>
        + From<HuobiSymbol>
        + Display
        + Debug
        + PairedExchangeSymbol
        + 'static,
{
    pub fn new(config: CombinedClientConfig) -> Self {
        Self {
            clients: HashMap::new(),
            config: config,
        }
    }

    /// Creates clients for each exchange that is `Some` in the config
    pub fn create_clients(&mut self) {
        // Create binance client
        if let Some(conf) = &self.config.binance {
            let client = BinanceClient::new(conf.api_key.clone(), conf.api_secret.clone());
            self.clients
                .insert(ExchangeObserverKind::Binance, Box::new(client));
        }

        // Create huobi client
        if let Some(conf) = &self.config.huobi {
            let client = HuobiClient::new(conf.api_key.clone(), conf.api_secret.clone());
            self.clients
                .insert(ExchangeObserverKind::Huobi, Box::new(client));
        }

        // Create kraken client
        if let Some(conf) = &self.config.kraken {
            let client = KrakenClient::new(conf.api_key.clone(), conf.api_secret.clone());
            self.clients
                .insert(ExchangeObserverKind::Kraken, Box::new(client));
        }
    }

    /// Checks if symbol exists on the exchange
    pub fn symbol_exists(&self, kind: ExchangeObserverKind, symbol: &Symbol) -> bool {
        if let Some(client) = self.clients.get(&kind) {
            client.symbol_exists(symbol)
        } else {
            false
        }
    }

    /// Fetches the balance of the current logged in user
    pub fn get_balance(
        &self,
        kind: ExchangeObserverKind,
        asset: &String,
    ) -> Option<ExchangeBalance> {
        if let Some(client) = self.clients.get(&kind) {
            client.get_balance(asset)
        } else {
            None
        }
    }

    /// Makes buy order on the exchange
    pub fn buy_order(&self, kind: ExchangeObserverKind, symbol: &Symbol, qty: f64, price: f64) {
        if let Some(client) = self.clients.get(&kind) {
            client.buy_order(symbol, qty, price)
        }
    }

    /// Makes sell order on the exchange
    pub fn sell_order(&self, kind: ExchangeObserverKind, symbol: &Symbol, qty: f64, price: f64) {
        if let Some(client) = self.clients.get(&kind) {
            client.sell_order(symbol, qty, price)
        }
    }

    /// Fetches balances for the current user whose api key is used
    pub fn get_balances(
        &self,
        kind: ExchangeObserverKind,
    ) -> Result<HashMap<String, ExchangeBalance>, Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(&kind) {
            Ok(client.get_balances()?)
        } else {
            Ok(HashMap::new())
        }
    }

    /// Fetches all symbols from the exchange and returns list of symbols
    pub fn fetch_symbols(
        &self,
        kind: ExchangeObserverKind,
    ) -> Result<Vec<Symbol>, Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(&kind) {
            client.fetch_symbols()
        } else {
            Ok(Vec::new())
        }
    }

    #[allow(dead_code)]
    /// Fetches online symbols from the exchange and returns list of symbols
    fn fetch_online_symbols(
        &self,
        kind: ExchangeObserverKind,
    ) -> Result<Vec<Symbol>, Box<dyn std::error::Error>> {
        if let Some(client) = self.clients.get(&kind) {
            client.fetch_online_symbols()
        } else {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    /*
    use super::*;
    use exch_observer_types::{
        ExchangeSymbol, ArbitrageExchangeSymbol,
    };
    use crate::config::{HuobiClientConfig, BinanceClientConfig};

    #[test]
    fn test_combined_client() {
        let mut config = CombinedClientConfig {
            binance: Some(BinanceClientConfig { api_key: None, api_secret: None }),
            huobi: Some(HuobiClientConfig { api_key: None, api_secret: None }),
            ..Default::default()
        };
        let mut client = CombinedClient::<ArbitrageExchangeSymbol>::new(config);
        client.create_clients();
        let symbols = client.fetch_online_symbols(ExchangeObserverKind::Huobi);
        panic!("{:?}", symbols);
        assert!(symbols.is_ok());
    }
    */
}
