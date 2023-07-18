use exch_observer_types::{ExchangeBalance, ExchangeClient, PairedExchangeSymbol};
use krakenrs::{
    AssetPair, BsType, KrakenCredentials, KrakenRestAPI, KrakenRestConfig, LimitOrder, OrderFlag,
};
use log::{error, info};
use std::{
    collections::{BTreeSet, HashMap},
    fmt::{Debug, Display},
    hash::Hash,
    marker::PhantomData,
    sync::Arc,
    time::Duration,
};

/// Interface to Kraken REST API.
pub struct KrakenClient<Symbol: Eq + Hash> {
    pub api: Arc<KrakenRestAPI>,
    marker: PhantomData<Symbol>,
}

impl<Symbol> KrakenClient<Symbol>
where
    Symbol: Eq + Hash + Clone + Display + Debug + Into<String> + From<AssetPair>,
{
    pub fn new(api_key: String, api_secret: String) -> Self {
        let creds = KrakenCredentials {
            key: api_key,
            secret: api_secret,
        };
        let config = KrakenRestConfig {
            timeout: Duration::from_secs(10),
            creds: creds,
        };

        let api = KrakenRestAPI::try_from(config).unwrap();

        Self {
            api: Arc::new(api),
            marker: PhantomData,
        }
    }

    /// Fetches symbols from the exchange, performs no filtration of modification of symbols
    fn fetch_symbols_unfiltered(
        &self,
    ) -> Result<HashMap<String, AssetPair>, Box<dyn std::error::Error>> {
        let symbols = self.api.asset_pairs(vec![]).unwrap();
        Ok(symbols)
    }

    /// Fetches symbols from the exchange
    fn fetch_and_convert_symbols(&self) -> Result<Vec<Symbol>, Box<dyn std::error::Error>> {
        let symbols = self.fetch_symbols_unfiltered()?;
        Ok(symbols
            .iter()
            .map(|(_, v)| {
                let symbol: Symbol = From::<AssetPair>::from(v.clone());
                symbol
            })
            .collect())
    }
}

impl<Symbol> ExchangeClient<Symbol> for KrakenClient<Symbol>
where
    Symbol:
        Eq + Hash + Clone + Display + Debug + Into<String> + From<AssetPair> + PairedExchangeSymbol,
{
    fn symbol_exists(&self, symbol: &Symbol) -> bool {
        let pairs = self.api.asset_pairs(vec![symbol.pair()]);
        return pairs.is_ok() && pairs.unwrap().len() > 0;
    }

    /// Fetches the balance for the given asset from Kraken API
    fn get_balance(&self, asset: &String) -> Option<ExchangeBalance> {
        let res = self.api.get_asset_balance(&asset.clone()).unwrap();
        let balance = res.get(&asset.clone()).unwrap();
        let rv = ExchangeBalance::new(asset.clone(), (*balance).try_into().unwrap(), 0.0);

        Some(rv)
    }

    /// Sends Buy GTC limit order to Kraken REST API
    fn buy_order(&self, symbol: &Symbol, qty: f64, price: f64) {
        info!(
            "calling Buy order on symbol: {}; qty: {}; price: {}",
            &symbol, qty, price
        );

        let oflags = BTreeSet::from_iter(vec![OrderFlag::Fciq]);
        let res = self.api.add_limit_order(
            LimitOrder {
                bs_type: BsType::Buy,
                volume: qty.to_string(),
                pair: symbol.to_string(),
                price: price.to_string(),
                oflags: oflags,
            },
            None, // userref
            false,
        );

        if let Ok(res) = res {
            info!("Buy order completed: {:?}", res);
        } else {
            error!("Error placing buy order: {:?}", res.err().unwrap());
        }
    }

    /// Sends Sell GTC limit order to Kraken REST API
    fn sell_order(&self, symbol: &Symbol, qty: f64, price: f64) {
        info!(
            "calling Buy order on symbol: {}; qty: {}; price: {}",
            &symbol, qty, price
        );

        let oflags = BTreeSet::from_iter(vec![OrderFlag::Fcib]);
        let res = self.api.add_limit_order(
            LimitOrder {
                bs_type: BsType::Sell,
                volume: qty.to_string(),
                pair: symbol.to_string(),
                price: price.to_string(),
                oflags: oflags,
            },
            None, // userref
            false,
        );

        if let Ok(res) = res {
            info!("Buy order completed: {:?}", res);
        } else {
            error!("Error placing buy order: {:?}", res.err().unwrap());
        }
    }

    /// Fetches the balances for all assets from Kraken API
    fn get_balances(&self) -> Result<HashMap<String, ExchangeBalance>, Box<dyn std::error::Error>> {
        let mut rv_map: HashMap<String, ExchangeBalance> = HashMap::new();

        for (key, val) in self.api.get_account_balance().unwrap().iter() {
            let balance = ExchangeBalance::new(key.clone(), (*val).try_into().unwrap(), 0.0);
            rv_map.insert(key.clone(), balance);
        }

        Ok(rv_map)
    }

    fn fetch_symbols(&self) -> Result<Vec<Symbol>, Box<dyn std::error::Error>> {
        self.fetch_and_convert_symbols()
    }

    /// Fetches online symbols from the exchange and returns list of symbols
    fn fetch_online_symbols(&self) -> Result<Vec<Symbol>, Box<dyn std::error::Error>> {
        Ok(self
            .fetch_symbols_unfiltered()?
            .iter()
            .filter(|(_, v)| v.status == "online")
            .map(|(_, v)| {
                let symbol: Symbol = From::<AssetPair>::from(v.clone());
                symbol
            })
            .collect())
    }
}
