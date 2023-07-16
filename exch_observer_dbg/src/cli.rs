use clap::{Parser, Subcommand};
use csv::Writer as CsvWriter;
use exch_clients::BinanceClient;
use exch_observer_config::ExchObserverConfig;
use exch_observer_types::{
    ExchangeBalance, ExchangeClient, ExchangeObserverKind, ExchangeSymbol, PairedExchangeSymbol,
};
use exch_observer_utils::get_current_timestamp;
use log::{debug, info};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::Path,
    rc::Rc,
    str::FromStr,
    sync::Mutex,
};
use tokio::runtime::Builder as RuntimeBuilder;

use crate::{
    scanner::SymbolScanner,
    symbols_parser::{BinanceSymbolsParser, HuobiSymbolsParser, KrakenSymbolsParser},
    types::{ArbitrageExchangeSymbolRow, SymbolsParser},
    utils::dump_balances,
};

#[derive(Debug, Subcommand)]
pub enum TradeUtilsCliCommand {
    DiffBalances {
        #[arg(
            short,
            long,
            default_value = "/home/keyg/.arbitrage-swap/binance-balances.json"
        )]
        old: String,
        #[arg(short, long, default_value = "false")]
        rewrite: bool,
    },

    ScanUninitializedSymbols {
        #[arg(short, long, default_value = "/home/keyg/.exch_observer/default.toml")]
        config: String,
        #[arg(short, long, default_value = "all")]
        network: String,
    },

    ScanUpdateTimes {
        #[arg(short, long, default_value = "/home/keyg/.exch_observer/default.toml")]
        config: String,
        #[arg(short, long, default_value = "all")]
        network: String,
    },

    FetchSymbols {
        /// Used to parse allowed symbols
        #[arg(short, long, default_value = "/home/keyg/.exch_observer/default.toml")]
        config: String,

        #[arg(short, long, default_value = "binance")]
        network: String,

        #[arg(short, long, default_value = "symbols.csv")]
        output: String,

        #[arg(long, default_value = "false")]
        all: bool,
    },
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct TradeUtilsCli {
    #[command(subcommand)]
    command: TradeUtilsCliCommand,
}

trait MyCustomHelperTrait: FnMut(ExchangeSymbol, f64, u64) + Sized {}

impl TradeUtilsCli {
    pub fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        match &self.command {
            TradeUtilsCliCommand::DiffBalances { old, rewrite } => {
                info!("DiffBalances cmd with old = {}, rewrite: {}", old, rewrite);
                self.diff_balances(old, *rewrite);
            }
            TradeUtilsCliCommand::ScanUninitializedSymbols { config, network } => {
                info!(
                    "ScanUninitializedSymbols cmd with config = {}, network = {}",
                    config, network
                );
                self.scan_uninitialized_symbols(config, network);
            }
            TradeUtilsCliCommand::ScanUpdateTimes { config, network } => {
                self.scan_update_times(config, network);
            }
            TradeUtilsCliCommand::FetchSymbols {
                config,
                network,
                output,
                all,
            } => {
                self.fetch_symbols(config, network, output, *all);
            }
        };
        Ok(())
    }

    /// Reads old balances from the provided path, fetches new balances from
    /// Binance api and compares them. If rewrite is true, new balances will be
    /// written to the provided path.
    fn diff_balances<P: AsRef<Path>>(&self, old_balances_p: P, rewrite: bool) {
        let old_balances: HashMap<String, ExchangeBalance> =
            serde_json::from_slice(&fs::read(&old_balances_p).unwrap()).unwrap();

        let binance_client: BinanceClient<ExchangeSymbol> = BinanceClient::new(
            env::var("BINANCE_API_KEY").ok(),
            env::var("BINANCE_API_SECRET").ok(),
        );
        let new_balances = binance_client.get_balances().unwrap();

        for (k, new_balance) in new_balances.iter() {
            if !old_balances.contains_key(k) {
                debug!("{} not found in new balances", k);
            }

            let old_balance = if let Some(balance) = old_balances.get(k) {
                balance
            } else {
                debug!("{} not found in old balances", k);
                continue;
            };

            if new_balance.free != old_balance.free {
                let pnl = new_balance.free - old_balance.free;
                println!("{} pnl: {}", k, pnl);
            }
        }

        if rewrite {
            info!(
                "Rewriting new balances to {}",
                old_balances_p.as_ref().to_str().unwrap()
            );
            dump_balances(&new_balances, old_balances_p);
        }
    }

    fn scan_update_times<P: AsRef<Path>>(&self, config_path: P, _network: &String) {
        // TODO: make use of _network argument
        let runtime = RuntimeBuilder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let config = ExchObserverConfig::parse_config(config_path).unwrap();
        let obs_config = &config.observer;

        let mut scanner = runtime.block_on(SymbolScanner::new(config.clone()));

        let callback_gen =
            move |kind: ExchangeObserverKind,
                  updates: Rc<Mutex<Vec<(String, ExchangeSymbol, f64, u64)>>>| {
                let network = kind.to_str();

                return move |symbol, price, timestamp| {
                    let timestamp = get_current_timestamp().unwrap() - timestamp;
                    updates
                        .lock()
                        .unwrap()
                        .push((network.into(), symbol, price, timestamp));
                };
            };

        let updates_vec = Rc::new(Mutex::new(Vec::new()));

        if let Some(conf) = &obs_config.binance {
            if conf.enable {
                // Generate a new callback with captures values
                let callback = callback_gen(ExchangeObserverKind::Binance, updates_vec.clone());
                runtime.block_on(scanner.run_with_obs(ExchangeObserverKind::Binance, callback));

                let mut updates = updates_vec.lock().unwrap();
                updates.sort_by(|a, b| a.3.cmp(&b.3));

                for (network, symbol, price, timestamp) in updates.iter() {
                    println!("{}, {}, {}, {}", network, symbol, price, timestamp);
                }
                updates.clear();
            }
        }

        if let Some(conf) = &obs_config.huobi {
            if conf.enable {
                let callback = callback_gen(ExchangeObserverKind::Huobi, updates_vec.clone());
                runtime.block_on(scanner.run_with_obs(ExchangeObserverKind::Huobi, callback));

                let mut updates = updates_vec.lock().unwrap();
                updates.sort_by(|a, b| a.3.cmp(&b.3));

                for (network, symbol, price, timestamp) in updates.iter() {
                    println!("{}, {}, {}, {}", network, symbol, price, timestamp);
                }
                updates.clear();
            }
        }

        if let Some(conf) = &obs_config.kraken {
            if conf.enable {
                let callback = callback_gen(ExchangeObserverKind::Kraken, updates_vec.clone());
                runtime.block_on(scanner.run_with_obs(ExchangeObserverKind::Kraken, callback));

                let mut updates = updates_vec.lock().unwrap();
                updates.sort_by(|a, b| a.3.cmp(&b.3));

                for (network, symbol, price, timestamp) in updates.iter() {
                    println!("{}, {}, {}, {}", network, symbol, price, timestamp);
                }
                updates.clear();
            }
        }
    }

    /// Scans all symbols from the config to find unintialized prices in the
    /// observer and prints them.
    fn scan_uninitialized_symbols<P: AsRef<Path>>(&self, config_path: P, _network: &String) {
        // TODO: make use of _network argument
        let runtime = RuntimeBuilder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let config = ExchObserverConfig::parse_config(config_path).unwrap();
        let obs_config = &config.observer;

        let mut scanner = runtime.block_on(SymbolScanner::new(config.clone()));

        // Closure to count uninitialized prices.
        //
        // It had to implemented this way, since we later capture `uninitialized` and `total` in
        // the callback, we couldn't store in the same closure, since we need references to them
        // outside of the closure lifetime scope. Implementeations with plain references that I had
        // come up with either didn't work as they should, or drown in lifetime errors.
        let callback_gen = move |uninitialized: Rc<Mutex<usize>>, total: Rc<Mutex<usize>>| {
            return move |_symbol, price, _timestamp| {
                if price == 0.0 {
                    let mut _uininitialized = uninitialized.lock().unwrap();
                    *_uininitialized += 1;
                }
                let mut _total = total.lock().unwrap();
                *_total += 1;
            };
        };

        let total_locked: Rc<Mutex<usize>> = Rc::new(Mutex::new(0));
        let uninitialized_locked: Rc<Mutex<usize>> = Rc::new(Mutex::new(0));

        if let Some(conf) = &obs_config.binance {
            if conf.enable {
                // Generate a new callback with captures values
                let callback = callback_gen(uninitialized_locked.clone(), total_locked.clone());
                runtime.block_on(scanner.run_with_obs(ExchangeObserverKind::Binance, callback));

                // Get the values after the scan and print them
                let mut uninitialized = uninitialized_locked.lock().unwrap();
                let mut total = total_locked.lock().unwrap();
                println!("[Binance] {}/{} is uninitialized", uninitialized, total);

                // Reset the values for future scans
                *total = 0;
                *uninitialized = 0;
            }
        }

        if let Some(conf) = &obs_config.huobi {
            if conf.enable {
                let callback = callback_gen(uninitialized_locked.clone(), total_locked.clone());
                runtime.block_on(scanner.run_with_obs(ExchangeObserverKind::Huobi, callback));

                let mut uninitialized = uninitialized_locked.lock().unwrap();
                let mut total = total_locked.lock().unwrap();
                println!("[Huobi] {}/{} is uninitialized", uninitialized, total);

                // Reset the values for future scans
                *total = 0;
                *uninitialized = 0;
            }
        }

        if let Some(conf) = &obs_config.kraken {
            if conf.enable {
                let callback = callback_gen(uninitialized_locked.clone(), total_locked.clone());
                runtime.block_on(scanner.run_with_obs(ExchangeObserverKind::Kraken, callback));

                let uninitialized = uninitialized_locked.lock().unwrap();
                let total = total_locked.lock().unwrap();
                println!("[Kraken] {}/{} is uninitialized", uninitialized, total);
            }
        }
    }

    fn fetch_symbols<P: AsRef<Path>>(
        &self,
        config_path: P,
        network: &String,
        output: &String,
        all: bool,
    ) {
        // Get correct parser for the network
        let network = ExchangeObserverKind::from_str(network).unwrap();
        let parser: Box<dyn SymbolsParser> = match network {
            ExchangeObserverKind::Binance => Box::new(BinanceSymbolsParser::new(None, None)),
            ExchangeObserverKind::Kraken => Box::new(KrakenSymbolsParser::new(None, None)),
            ExchangeObserverKind::Huobi => Box::new(HuobiSymbolsParser::new(None, None)),
            _ => panic!("Not implemented yet"),
        };

        // Fetch and log symbols
        let symbols = parser.fetch_symbols();
        info!("Fetched {} symbols", symbols.len());

        // Create output file writer and write symbols to it
        let mut writer = CsvWriter::from_path(output).unwrap();
        let mut write_counter: usize = 0;

        // Write all symbols if `--all` flag was provided
        if all {
            for symbol in symbols.iter() {
                writer
                    .serialize(Into::<ArbitrageExchangeSymbolRow>::into(symbol.clone()))
                    .unwrap();
                write_counter += 1;
            }
        } else {
            let config = ExchObserverConfig::parse_config(config_path).unwrap();
            let obs_config = &config.observer;

            // Get allowed symbols from config
            let allowed_symbols = match network {
                ExchangeObserverKind::Binance => obs_config
                    .binance
                    .as_ref()
                    .unwrap()
                    .allowed_symbols
                    .as_ref()
                    .unwrap(),
                ExchangeObserverKind::Kraken => obs_config
                    .kraken
                    .as_ref()
                    .unwrap()
                    .allowed_symbols
                    .as_ref()
                    .unwrap(),
                ExchangeObserverKind::Huobi => obs_config
                    .huobi
                    .as_ref()
                    .unwrap()
                    .allowed_symbols
                    .as_ref()
                    .unwrap(),
                _ => panic!("Not implemented yet"),
            };

            // Convert allowed symbols to a hashset for faster lookup
            let allowed_symbols_hs = allowed_symbols.iter().collect::<HashSet<&String>>();

            // Filter out symbols that are not allowed
            for symbol in symbols.iter().filter(|s| {
                allowed_symbols_hs.contains(&s.base().to_string())
                    && allowed_symbols_hs.contains(&s.quote().to_string())
            }) {
                writer
                    .serialize(Into::<ArbitrageExchangeSymbolRow>::into(symbol.clone()))
                    .unwrap();
                write_counter += 1;
            }
        }

        // Flush and print final message to the user
        writer.flush().unwrap();
        println!("Written {} symbols to {}", write_counter, output);
    }
}
