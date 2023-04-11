use std::{
    sync::{Arc, RwLock},
};
use exch_observer_config::ObserverConfig;
use exch_observer_rpc::ObserverRpcRunner;
use exch_observer::ObserverRunner;
use clap::{
    Parser, Subcommand
};

#[derive(Debug, Subcommand)]
pub enum ExchCliCommand {
    Launch {
        #[arg(short, long, default_value = "/home/keyg/.exch_observer/default.toml")]
        config: String
    },
    FetchSymbol {
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
            ExchCliCommand::FetchSymbol { network, base, quote } => {
                println!("{} {} {}", network, base, quote);
            }
        }
    }

    pub fn launch(&self, config: String) {
        let config = ObserverConfig::parse_config(config).unwrap();
        println!("{:?}", config);
        let rpc_config = config.rpc.clone().unwrap_or_default();
        let mut obs = Arc::new(RwLock::new(ObserverRunner::new(config)));
        obs.write().unwrap().launch();
        let mut rpc_runner = ObserverRpcRunner::new(&obs, rpc_config);
        rpc_runner.run();
    }
}
