use std::{
    sync::Arc
};
use tokio::runtime::{Runtime};
use exch_observer_types::{
    ExchangeSymbol,
    ExchangeBalance, ExchangeClient
};
use binance::{
    account::{Account, OrderSide, OrderType, TimeInForce},
    api::Binance,
    market::Market,
};
use log::{info};

pub struct BinanceClient {
    pub account: Arc<Account>,
    pub market: Arc<Market>,
    pub runtime: Option<Arc<Runtime>>,
}

impl BinanceClient {
    pub fn new(api_key: Option<String>, secret_key: Option<String>) -> Self {
        Self {
            account: Arc::new(Account::new(api_key.clone(), secret_key.clone())),
            market: Arc::new(Market::new(api_key, secret_key)),
            runtime: None,
        }
    }

    pub fn new_with_runtime(api_key: Option<String>, secret_key: Option<String>, async_runner: Arc<Runtime>) -> Self {
        Self {
            account: Arc::new(Account::new(api_key.clone(), secret_key.clone())),
            market: Arc::new(Market::new(api_key, secret_key)),
            runtime: Some(async_runner),
        }
    }

    pub fn set_runtime(&mut self, runtime: Arc<Runtime>) {
        self.runtime = Some(runtime);
    }

    pub fn has_runtime(&self) -> bool {
        self.runtime.is_some()
    }

    fn buy_order(
        runner: &Runtime,
        account: Arc<Account>,
        symbol: ExchangeSymbol,
        qty: f64,
        price: f64,
    ) {
        let mut symbol: String = symbol.into();
        symbol.make_ascii_uppercase();
        runner.spawn_blocking(move || {
            info!("calling Buy order on symbol: {}; qty: {}; price: {}", &symbol, qty, price);
            let recipe = account
                .custom_order::<String, f64>(
                    symbol.clone().into(),
                    f64::try_from(qty).unwrap(),
                    f64::try_from(price).unwrap(),
                    None,
                    OrderSide::Buy,
                    OrderType::Limit,
                    TimeInForce::GTC,
                    None,
                )
                .unwrap();

            info!(
                "Trade [sym: {}, qty: {}, price: {}] successful, recipe: {:?}",
                symbol, qty, price, recipe
            );

            recipe
        });
    }

    fn sell_order(
        runner: &Runtime,
        account: Arc<Account>,
        symbol: ExchangeSymbol,
        qty: f64,
        price: f64,
    ) {
        let mut symbol: String = symbol.into();
        symbol.make_ascii_uppercase();
        runner.spawn_blocking(move || {
            info!("calling Sell order on symbol: {}; qty: {}; price: {}", &symbol, qty, price);
            let recipe = account
                .custom_order::<String, f64>(
                    symbol.clone().into(),
                    f64::try_from(qty).unwrap(),
                    f64::try_from(price).unwrap(),
                    None,
                    OrderSide::Sell,
                    OrderType::Limit,
                    TimeInForce::GTC,
                    None,
                )
                .unwrap();

            info!(
                "Trade [sym: {}, qty: {}, price: {}] successful, recipe: {:?}",
                symbol, qty, price, recipe
            );

            recipe
        });
    }
}

impl ExchangeClient for BinanceClient {
    fn symbol_exists(&self, symbol: &ExchangeSymbol) -> bool {
        self.market
            .get_depth(Into::<String>::into(symbol).to_ascii_uppercase())
            .is_ok()
    }

    fn get_balance(&self, asset: &String) -> Option<ExchangeBalance> {
        let mut asset: String = asset.into();
        asset.make_ascii_uppercase();
        let rv = self.account
            .get_balance(asset.clone())
            .ok()
            .map(|v| Into::<ExchangeBalance>::into(v));

        info!("Fetching balance for {}: {:?}", asset, rv);
        rv
    }

    fn buy_order(&self, symbol: &ExchangeSymbol, qty: f64, price: f64) {
        let runtime = if let Some(runtime) = &self.runtime {
            runtime.clone()
        } else {
            panic!("No runtime set for BinanceClient, cannot execute buy order");
        };
        Self::buy_order(
            &runtime, self.account.clone(),
            symbol.clone(), qty, price
        );
    }

    fn sell_order(&self, symbol: &ExchangeSymbol, qty: f64, price: f64) {
        let runtime = if let Some(runtime) = &self.runtime {
            runtime.clone()
        } else {
            panic!("No runtime set for BinanceClient, cannot execute sell order");
        };
        Self::sell_order(
            &runtime, self.account.clone(),
            symbol.clone(), qty, price
        );
    }
}
