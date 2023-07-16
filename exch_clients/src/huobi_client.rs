use chrono::Utc;
use exch_observer_types::{
    ExchangeBalance, ExchangeClient, ExchangeAccount, ExchangeAccountType,
    exchanges::huobi::{HuobiSymbol, HuobiAccountsResponse, HuobiAccountBalanceResponse, HuobiError}
};
use hmac::{Hmac, Mac};
use reqwest::{
    blocking::{Client as ReqwestClient, Request},
};
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

/// Huobi HTTPS REST client.
/// Documentation - https://huobiapi.github.io/docs/spot/v1/en
pub struct HuobiClient<Symbol: Eq + Hash + From<HuobiSymbol>> {
    /// Huobi API KEY
    pub api_key: Option<String>,
    /// Huobi Secret KEY
    pub secret_key: Option<String>,
    pub client: ReqwestClient,
    marker: PhantomData<Symbol>,
}

impl<Symbol> HuobiClient<Symbol>
where
    Symbol: Eq + Hash + Clone + Display + Debug + Into<String> + From<HuobiSymbol>,
{
    pub fn new(api_key: Option<String>, secret_key: Option<String>) -> Self {
        HuobiClient {
            api_key,
            secret_key,
            client: ReqwestClient::new(),
            marker: PhantomData,
        }
    }

    /// Checks if API keys are provided, returns them as a tuple if so. Otherwise returns an error.
    fn unwrap_api_keys(&self, req_name: &str) -> Result<(&String, &String), Box<dyn std::error::Error>> {
        if self.api_key.is_none() || self.secret_key.is_none() {
            return Err(Box::new(HuobiError::ApiKeyNotProvided(
                String::from(format!("API key wasn't provided for request: {}", req_name))
            )));
        }

        Ok((&self.api_key.as_ref().unwrap(), &self.secret_key.as_ref().unwrap()))
    }

    /// Generates request signature for Huobi API. Returns base64-encoded signature string.
    pub fn get_signature(&self, req: &Request) -> String {
        let mut mac = HuobiSignature::new_from_slice(self.secret_key.as_ref().unwrap().as_bytes())
            .expect("Failed to create HMAC-SHA256 instance");

        let method = format!("{}", req.method().as_str());
        let host = format!("{}", req.url().host_str().unwrap_or(HUOBI_API_URL));
        let path = format!("{}", req.url().path());
        let query = req.url().query().unwrap_or("");

        // Concatenate method, host, path and query into single stirng
        let body = format!("{}\n{}\n{}\n{}", method, host, path, query);

        mac.update(body.as_ref());
        base64::encode(mac.finalize().into_bytes())
    }

    /// API to get information about acccounts of the user.
    fn accounts(&self) -> Result<HashMap<ExchangeAccountType, ExchangeAccount>, Box<dyn std::error::Error>> {

        let (api_key, _) = self.unwrap_api_keys("/v1/account/accounts")?;

        let mut req = self.client.get(format!("{}/v1/account/accounts", HUOBI_API_URL).as_str())
            .query(&[
                ("AccessKeyId", api_key.as_str()),
                ("SignatureMethod", "HmacSHA256"),
                ("SignatureVersion", "2"),
                ("Timestamp", Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string().as_str()),
            ])
            .build()?;

        // Generate request signature
        let signature = self.get_signature(&req);

        // Add signature to the request query
        req.url_mut().query_pairs_mut().append_pair("Signature", signature.as_str());

        // Send request and get response body
        let res_body = self.client.execute(req)?.text()?;

        // Parse response body into HashMap<ExchangeAccountType, ExchangeAccount>
        let res = serde_json::from_str::<HuobiAccountsResponse>(&res_body)?
            .data
            .iter()
            .map(|a| {
                let r = Into::<ExchangeAccount>::into(a);
                (r.account_type.clone(), r)
            })
            .collect::<HashMap<ExchangeAccountType, ExchangeAccount>>();

        Ok(res)
    }

    /// API to get the balance of an account.
    fn get_account_balance(&self, account_id: String)
        -> Result<HashMap<String, ExchangeBalance>, Box<dyn std::error::Error>> {

        let (api_key, _) = self.unwrap_api_keys(&format!("/v1/account/accounts/{}/balance", account_id))?;

        let mut req = self.client.get(format!("{}/v1/account/accounts/{}/balance", HUOBI_API_URL, account_id).as_str())
            .query(&[
                ("AccessKeyId", api_key.as_str()),
                ("SignatureMethod", "HmacSHA256"),
                ("SignatureVersion", "2"),
                ("Timestamp", Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string().as_str()),
            ])
            .build()?;

        // Generate request signature
        let signature = self.get_signature(&req);

        // Add signature to the request query
        req.url_mut().query_pairs_mut().append_pair("Signature", signature.as_str());

        // Send request and get response body
        let res_body = self.client.execute(req)?.text()?;

        // Parse response body
        let res = serde_json::from_str::<HuobiAccountBalanceResponse>(&res_body)
            .expect("Failed to parse account balance response");

        let mut rv: HashMap<String, ExchangeBalance> = HashMap::new();

        for currency in res.data.list {
            let balance = currency.balance.parse::<f64>()
                .expect("Failed to parse account balance");

            if balance > 0.0 {
                match currency.type_field.as_str() {
                    "trade" => {
                        if let Some(mut balance_data) = rv.get_mut(currency.currency.as_str()) {
                            balance_data.free = balance_data.free + balance;
                        } else {
                            let asset = currency.currency.clone();
                            rv.insert(currency.currency, ExchangeBalance {
                                asset: asset,
                                free: balance,
                                locked: 0.0,
                            });
                        }
                    },
                    _ => unimplemented!(),
                }
            }
        }

        Ok(rv)
    }

    fn fetch_symbols_unfiltered(&self) -> Result<Vec<HuobiSymbol>, Box<dyn std::error::Error>> {
        let res_body =
            reqwest::blocking::get(format!("{}/v1/common/symbols", HUOBI_API_URL).as_str())?
                .text()?;
        let binding = serde_json::from_str::<serde_json::Value>(&res_body).expect("Failed to parse Huobi symbols response");

        // Convert `data` field into Vec<HuobiSymbol>
        let symbols_vec = binding
            .get("data")
            .expect("Failed to get data field from Huobi symbols response")
            .as_array()
            .expect("Failed to convert data field into array")
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
    fn symbol_exists(&self, _symbol: &Symbol) -> bool {
        true
    }

    /// Fetches the balance of the current logged in user
    fn get_balance(&self, asset: &String) -> Option<ExchangeBalance> {
        // TODO: do caching instead of fetching all balances every time,
        // which is currently the only supported way to get balances from Huobi
        let balances = self.get_balances().expect("Failed to get balances");
        balances.get(asset).map(|b| b.clone())
    }

    /// Makes buy order on the exchange
    fn buy_order(&self, _symbol: &Symbol, _qty: f64, _price: f64) {
    }

    /// Makes sell order on the exchange
    fn sell_order(&self, _symbol: &Symbol, _qty: f64, _price: f64) {
    }

    /// Fetches balances for the current user whose api key is used
    fn get_balances(&self) -> Result<HashMap<String, ExchangeBalance>, Box<dyn std::error::Error>> {

        // Fetch accounts 
        let accounts = self.accounts()?;
        // Get spot account
        let spot_account = accounts.get(&ExchangeAccountType::Spot)
            .expect("Failed to get spot account");
        // Parse balances from spot account
        let balances = self.get_account_balance(spot_account.id.clone())
            .expect("Failed to construct balances");

        Ok(balances)
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

    use exch_observer_types::{ExchangeSymbol};

    const API_KEY: &str = "";
    const SECRET_KEY: &str = "";

    #[test]
    fn test_signature() {
        let client = HuobiClient::<ExchangeSymbol>::new(
            Some(API_KEY.to_string()),
            Some(SECRET_KEY.to_string())
        );
        let req = client.client.get(format!("{}/v1/account/accounts", HUOBI_API_URL).as_str())
            .query(&[
                ("AccessKeyId", API_KEY),
                ("SignatureMethod", "HmacSHA256"),
                ("SignatureVersion", "2"),
                ("Timestamp", "2023-06-11T15:19:30"),
            ])
            .build().expect("Failed to build request");

        let request = client.get_signature(&req);
        assert_eq!(
            request,
            "MuR/t0iLoXEwHLyOoBgX1/dL/BE3wuhO4xVngjkM7vM="
        );
    }

    /*
    #[test]
    fn test_account_balance() {
        let client = HuobiClient::<ArbitrageExchangeSymbol>::new(
            Some(API_KEY.to_string()),
            Some(SECRET_KEY.to_string()),
        );

        let accounts = client.get_balance(&String::from("sol")).unwrap();
        panic!("Accounts: {:?}", accounts);
    }
    */
}
