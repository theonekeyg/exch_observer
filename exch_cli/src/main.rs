use std::{
    sync::{Arc, RwLock},
};
use clap::Parser;
use dotenvy::dotenv;
use exch_observer::ObserverRunner;
use exch_cli::{ExchCli, ExchCliCommand};
use exch_clients::BinanceClient;
use binance::{
    account::Account,
    api::Binance
};

fn main() {
    dotenv().ok();
    env_logger::init();
    let cli = ExchCli::parse();
    cli.start();
    println!("{:?}", cli);
}
