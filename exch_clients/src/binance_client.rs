use std::{
    sync::Arc
};
use tokio::runtime::{Runtime, Builder as RuntimeBuilder};
use exch_observer_types::{
    ExchangeObserver, ExchangeObserverKind, ExchangeSymbol,
    ExchangeBalance, ExchangeClient
};
use binance::{
    account::{Account, OrderSide, OrderType, TimeInForce},
    api::Binance,
    market::Market,
    model::{Balance as BinanceBalance, Transaction},
};
use log::{info, debug, warn};

pub struct BinanceClient {
    pub account: Arc<Account>,
    pub market: Arc<Market>,
    pub async_runner: Runtime,
}

impl BinanceClient {
    pub fn new(api_key: Option<String>, secret_key: Option<String>, n_threads: usize) -> Self {
        Self {
            account: Arc::new(Account::new(api_key.clone(), secret_key.clone())),
            market: Arc::new(Market::new(api_key, secret_key)),
            async_runner: RuntimeBuilder::new_multi_thread()
                .worker_threads(n_threads)
                .build()
                .unwrap(),
        }
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
        Self::buy_order(
            &self.async_runner, self.account.clone(),
            symbol.clone(), qty, price
        );
    }

    fn sell_order(&self, symbol: &ExchangeSymbol, qty: f64, price: f64) {
        Self::sell_order(
            &self.async_runner, self.account.clone(),
            symbol.clone(), qty, price
        );
    }
}
