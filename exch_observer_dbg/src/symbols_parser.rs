use crate::types::{ArbitrageExchangeSymbolRow, SymbolsParser};
use exch_clients::{BinanceClient, KrakenClient};
use exch_observer_types::{ArbitrageExchangeSymbol, ExchangeClient, PairedExchangeSymbol};

pub struct BinanceSymbolsParser {
    pub client: BinanceClient<ArbitrageExchangeSymbol>,
}

impl BinanceSymbolsParser {
    pub fn new(api_key: Option<String>, secret_key: Option<String>) -> Self {
        let client = BinanceClient::new(api_key, secret_key);
        Self { client }
    }
}

pub struct KrakenSymbolsParser {
    pub client: KrakenClient<ArbitrageExchangeSymbol>,
}

impl KrakenSymbolsParser {
    pub fn new(api_key: Option<String>, secret_key: Option<String>) -> Self {
        let client = KrakenClient::new(
            api_key.unwrap_or("".to_string()),
            secret_key.unwrap_or("".to_string()),
        );

        Self { client }
    }
}

impl SymbolsParser for BinanceSymbolsParser {
    fn fetch_symbols(&self) -> Vec<ArbitrageExchangeSymbol> {
        self.client.fetch_online_symbols().unwrap()
    }
}

impl SymbolsParser for KrakenSymbolsParser {
    fn fetch_symbols(&self) -> Vec<ArbitrageExchangeSymbol> {
        self.client.fetch_online_symbols().unwrap()
    }
}
