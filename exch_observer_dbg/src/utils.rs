use exch_observer_types::ExchangeBalance;
use std::{collections::HashMap, fs, path::Path};

pub fn dump_balances<P: AsRef<Path>>(balances: &HashMap<String, ExchangeBalance>, path: P) {
    let balances_str = serde_json::to_string(balances).unwrap();
    fs::write(path, &balances_str).unwrap();
}
