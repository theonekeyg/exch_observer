use exch_observer_types::ExchangeObserver;
use exch_observer_config::ObserverConfig;

#[derive(Debug)]
pub struct ObserverRunner<O: ExchangeObserver> {
    pub observers: Vec<O>,
    pub config: ObserverConfig
}
