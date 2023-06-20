use clap::Parser;
use dotenvy::dotenv;
use exch_observer_dbg::TradeUtilsCli;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::init();
    let cli = TradeUtilsCli::parse();
    cli.start().unwrap();

    Ok(())
}
