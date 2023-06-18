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
use tokio::runtime::Runtime;

/// Interface to Kraken REST API.
pub struct KrakenClient<Symbol: Eq + Hash> {
    pub api: Arc<KrakenRestAPI>,
    pub runtime: Option<Arc<Runtime>>,
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
            runtime: None,
            marker: PhantomData,
        }
    }

    pub fn new_with_runtime(api_key: String, api_secret: String, runtime: Arc<Runtime>) -> Self {
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
            runtime: Some(runtime),
            marker: PhantomData,
        }
    }

    pub fn set_runtime(&mut self, runtime: Arc<Runtime>) {
        self.runtime = Some(runtime);
    }

    pub fn has_runtime(&self) -> bool {
        self.runtime.is_some()
    }

    fn buy_order1(runner: &Runtime, api: Arc<KrakenRestAPI>, symbol: Symbol, qty: f64, price: f64) {
        let symbol: String = symbol.into();
        runner.spawn_blocking(move || {
            info!(
                "calling Buy order on symbol: {}; qty: {}; price: {}",
                &symbol, qty, price
            );

            let oflags = BTreeSet::from_iter(vec![OrderFlag::Fciq]);
            let res = api.add_limit_order(
                LimitOrder {
                    bs_type: BsType::Buy,
                    volume: qty.to_string(),
                    pair: symbol,
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
        });
    }

    fn sell_order1(
        runner: &Runtime,
        api: Arc<KrakenRestAPI>,
        symbol: Symbol,
        qty: f64,
        price: f64,
    ) {
        let symbol: String = symbol.into();
        runner.spawn_blocking(move || {
            info!(
                "calling Buy order on symbol: {}; qty: {}; price: {}",
                &symbol, qty, price
            );

            let oflags = BTreeSet::from_iter(vec![OrderFlag::Fcib]);
            let res = api.add_limit_order(
                LimitOrder {
                    bs_type: BsType::Sell,
                    volume: qty.to_string(),
                    pair: symbol,
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
        });
    }

    /// Fetches symbols from the exchange
    fn fetch_and_convert_symbols(&self) -> Result<Vec<Symbol>, Box<dyn std::error::Error>> {
        let symbols = self.api.asset_pairs(vec![]).unwrap();
        // TODO: Rewrite this function to return Iterator instead of Vec.
        // This function doesn't need to return collected Vec,
        // instead returning the iterator would be far more better solution.
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
        let runtime = if let Some(runtime) = &self.runtime {
            runtime.clone()
        } else {
            panic!("No runtime set for KrakenClient, cannot execute buy order");
        };
        Self::buy_order1(&runtime, self.api.clone(), symbol.clone(), qty, price);
    }

    /// Sends Sell GTC limit order to Kraken REST API
    fn sell_order(&self, symbol: &Symbol, qty: f64, price: f64) {
        let runtime = if let Some(runtime) = &self.runtime {
            runtime.clone()
        } else {
            panic!("No runtime set for KrakenClient, cannot execute sell order");
        };
        Self::sell_order1(&runtime, self.api.clone(), symbol.clone(), qty, price);
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
}
