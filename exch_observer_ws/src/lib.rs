use std::{
    collections::HashMap,
    sync::{Arc, RwLock, mpsc},
    hash::Hash,
    marker::PhantomData,
    fmt::{Debug, Display},
};
use exch_observer_config::{
    WsConfig,
};
use exch_subobservers::{
    CombinedObserver,
};
use exch_observer_types::{
    ExchangeKind, ExchangeSymbol, PriceUpdateEvent, PairedExchangeSymbol
};

/// This struct serves as a main interface to exch_observer WS service.
/// It sends the data about the prices to the connected WS clients.
pub struct WsExchObserver<Symbol>
where
    Symbol: Eq
        + Hash
        + Clone
        + Display
        + Debug
        + Into<String>
        + PairedExchangeSymbol
        + Send
        + Sync
        + 'static,
{
    /// Config for the ws server
    pub config: WsConfig,
    /// Observer that will be used to get the prices
    observer: Arc<RwLock<CombinedObserver<Symbol>>>,

    /// Map of exchange kind to mpsc channel receiver
    /// that will be used to send the prices to the subscribed WS clients
    pub exchange_kind_to_rx: HashMap<ExchangeKind, mpsc::Receiver<PriceUpdateEvent>>,

    marker: PhantomData<Symbol>,
}

impl<Symbol> WsExchObserver<Symbol>
where
    Symbol: Eq
        + Hash
        + Clone
        + Display
        + Debug
        + Into<String>
        + PairedExchangeSymbol
        + Into<String>
        + Send
        + Sync
        + 'static,
    {
    pub fn new(config: WsConfig, observer: Arc<RwLock<CombinedObserver<Symbol>>>) -> Self {
        {
            let mut mut_observer = observer.write().expect("Failed to lock RWLock for writing");
            // let (tx, rx) = mpsc::channel();
        }

        Self {
            config: config,
            observer,
            exchange_kind_to_rx: HashMap::new(),
            marker: PhantomData,
        }
    }
}
