use std::{
    sync::{Arc, RwLock},
};
use exch_observer_config::ObserverConfig;
use exch_observer_rpc::{ObserverRpcRunner, ObserverRpcClient};
use exch_observer::ObserverRunner;
use clap::{
    Parser, Subcommand
};
use log::{info, debug};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

#[derive(Debug, Subcommand)]
pub enum ExchCliCommand {
    Launch {
        #[arg(short, long, default_value = "/home/keyg/.exch_observer/default.toml")]
        config: String
    },
    FetchSymbol {
        #[arg(short, long, default_value = "/home/keyg/.exch_observer/default.toml")]
        config: String,
        #[arg(short, long, default_value="binance")]
        network: String,
        #[arg(short, long)]
        base: String,
        #[arg(short, long)]
        quote: String,
    },
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct ExchCli {
    #[command(subcommand)]
    command: ExchCliCommand,
}

impl ExchCli {
    pub fn start(&self) {
        match &self.command {
            ExchCliCommand::Launch { config } => {
                self.launch(config.to_string());
            },
            ExchCliCommand::FetchSymbol { config, network, base, quote } => {
                self.fetch_symbol(config.to_string(), network.to_string(), base.to_string(), quote.to_string());
            }
        }
    }

    pub fn fetch_symbol(&self, config: String, network: String, base: String, quote: String) {
        let config = ObserverConfig::parse_config(config).unwrap();
        let rpc_config = config.rpc.clone().unwrap_or_default();
        let runtime = RuntimeBuilder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        debug!("Creating an RPC client");
        let mut client = runtime.block_on(ObserverRpcClient::new(rpc_config));

        debug!("Fetching price");
        let price = runtime.block_on(client.get_price(&network, &base, &quote));
        println!("price: {}", price);
    }

    pub fn launch(&self, config: String) {
        let config = ObserverConfig::parse_config(config).unwrap();
        println!("{:?}", config);
        let mut obs = ObserverRunner::new(config);
        obs.launch().unwrap();
        /*
        let rpc_config = config.rpc.clone().unwrap_or_default();
        let mut obs = Arc::new(RwLock::new(ObserverRunner::new(config)));
        obs.write().unwrap().launch();
        let runtime = obs.read().unwrap().get_async_runner().clone();
        let mut rpc_runner = ObserverRpcRunner::new(&obs, rpc_config, runtime);
        rpc_runner.run();
        */
    }
}
