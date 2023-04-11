use std::{
    sync::{Arc, RwLock},
};
use clap::Parser;
use dotenvy::dotenv;
use exch_observer_rpc::{
};
use exch_observer::ObserverRunner;
use exch_cli::{ExchCli, ExchCliCommand};

fn main() {
    dotenv().ok();
    env_logger::init();
    let cli = ExchCli::parse();
    cli.start();
    println!("{:?}", cli);
}
