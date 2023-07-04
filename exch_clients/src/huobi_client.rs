use chrono::Utc;
use exch_observer_types::{
    ArbitrageExchangeSymbol, ExchangeBalance, ExchangeClient, ExchangeSymbol,
    exchanges::huobi::HuobiSymbol
};
use hmac::{Hmac, Mac};
use reqwest::{Body, Request, RequestBuilder};
use sha2::Sha256;
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    marker::PhantomData,
};

/// Type of huobi signature in market requests (HMAC-SHA256)
type HuobiSignature = Hmac<Sha256>;

const HUOBI_API_URL: &'static str = "https://api.huobi.pro";

pub struct HuobiClient<Symbol: Eq + Hash + From<HuobiSymbol>> {
    /// Huobi API KEY
    pub api_key: String,
    /// Huobi Secret KEY
    pub secret_key: String,
    marker: PhantomData<Symbol>,
}

impl<Symbol> HuobiClient<Symbol>
where
    Symbol: Eq + Hash + Clone + Display + Debug + Into<String> + From<HuobiSymbol>,
{
    pub fn new(api_key: String, secret_key: String) -> Self {
        HuobiClient {
            api_key,
            secret_key,
            marker: PhantomData,
        }
    }

    pub fn get_signature(&self, body: &[u8]) -> String {
        let mut mac = HuobiSignature::new_from_slice(self.secret_key.as_bytes()).unwrap();

        mac.update(body);
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    }

    fn fetch_symbols_unfiltered(&self) -> Result<Vec<HuobiSymbol>, Box<dyn std::error::Error>> {
        let res_body =
            reqwest::blocking::get(format!("{}/v1/common/symbols", HUOBI_API_URL).as_str())?
                .text()?;
        let binding = serde_json::from_str::<serde_json::Value>(&res_body).unwrap();
        // Convert data field into Vec<HuobiSymbol>
        let symbols_vec = binding
            .get("data")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|s| HuobiSymbol::from(serde_json::from_value::<HuobiSymbol>(s.clone()).unwrap()))
            .collect::<Vec<HuobiSymbol>>();

        Ok(symbols_vec)
    }

    // pub fn new_order(
}

impl<Symbol> ExchangeClient<Symbol> for HuobiClient<Symbol>
where
    Symbol: Eq + Hash + Clone + Display + Debug + Into<String> + From<HuobiSymbol>,
{
    fn symbol_exists(&self, symbol: &Symbol) -> bool {
        true
    }

    /// Fetches the balance of the current logged in user
    fn get_balance(&self, asset: &String) -> Option<ExchangeBalance> {
        None
    }

    /// Makes buy order on the exchange
    fn buy_order(&self, symbol: &Symbol, qty: f64, price: f64) {}

    /// Makes sell order on the exchange
    fn sell_order(&self, symbol: &Symbol, qty: f64, price: f64) {}

    /// Fetches balances for the current user whose api key is used
    fn get_balances(&self) -> Result<HashMap<String, ExchangeBalance>, Box<dyn std::error::Error>> {
        Ok(HashMap::new())
    }

    /// Fetches all symbols from the exchange and returns list of symbols
    fn fetch_symbols(&self) -> Result<Vec<Symbol>, Box<dyn std::error::Error>> {
        Ok(self
            .fetch_symbols_unfiltered()?
            .iter()
            .map(|s| Symbol::from(s.clone()))
            .collect())
    }

    /// Fetches online symbols from the exchange and returns list of symbols
    fn fetch_online_symbols(&self) -> Result<Vec<Symbol>, Box<dyn std::error::Error>> {
        Ok(self
            .fetch_symbols_unfiltered()?
            .iter()
            .filter(|s| s.state == "online")
            .map(|s| Symbol::from(s.clone()))
            .collect())
    }
}

#[cfg(test)]
mod test {
    #[allow(dead_code)]
    use super::*;

    const API_KEY: &str = "26a15081-qz5c4v5b6n-e24b3e6c-06545";
    const SECRET_KEY: &str = "a4ec0775-2845fec2-640a8a28-b3fa6";

    #[test]
    fn test_signature() {
        let client =
            HuobiClient::<ExchangeSymbol>::new(API_KEY.to_string(), SECRET_KEY.to_string());
        let request = client.get_signature(b"Hello world");
        assert_eq!(
            request,
            "03ad9ec9882cadd851ce13d1af19df3da4f4133dd41ddf8ceddd77ccf6148cd6"
        );
    }

    #[test]
    fn test_fetch_symbols() {
        let client = HuobiClient::<ArbitrageExchangeSymbol>::new(
            API_KEY.to_string(),
            SECRET_KEY.to_string(),
        );
        let symbols = client.fetch_symbols().unwrap();
        panic!("Symbols: {:?}", symbols);
    }
}
