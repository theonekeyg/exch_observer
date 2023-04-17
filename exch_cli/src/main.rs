use clap::Parser;
use dotenvy::dotenv;

use exch_cli::ExchCli;

fn main() {
    dotenv().ok();
    env_logger::init();
    let cli = ExchCli::parse();
    cli.start();
    println!("{:?}", cli);
}
