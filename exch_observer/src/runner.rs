use log::info;
use rust_decimal::Decimal;
use std::{
    str::FromStr,
    sync::{Arc, RwLock},
};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

use exch_observer_config::ExchObserverConfig;
use exch_observer_rpc::ObserverRpcRunner;
use exch_observer_types::ArbitrageExchangeSymbol;
use exch_observer_ws::ObserverWsRunner;
use exch_subobservers::CombinedObserver;

/// Main runner for the observer binary, contains the main observer and the
/// configured services in the config
pub struct ObserverRunner {
    pub main_observer: Arc<RwLock<CombinedObserver<ArbitrageExchangeSymbol>>>,
    pub config: ExchObserverConfig,
    pub runtime: Arc<Runtime>,
}

impl ObserverRunner {
    pub fn new(config: ExchObserverConfig) -> Self {
        let observer = Arc::new(RwLock::new(CombinedObserver::new(config.observer.clone())));

        // Create and set runtime for the observer
        let async_runtime = Arc::new(
            RuntimeBuilder::new_multi_thread()
                .worker_threads(config.num_threads.unwrap_or(4))
                .enable_io()
                .build()
                .unwrap(),
        );

        observer.write().unwrap().set_runtime(async_runtime.clone());

        Self {
            main_observer: observer,
            config: config,
            runtime: async_runtime,
        }
    }

    /// Returns an Arc to the tokio's runtime ObserverRunner is using
    pub fn get_async_runner(&self) -> &'_ Arc<Runtime> {
        return &self.runtime;
    }

    /// Launches the observer and other services (i.e. RPC) that might interact with the observer
    pub fn launch(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        {
            // Create the observers, load their symbols and launch them
            let mut observer = self.main_observer.write().unwrap();
            observer.create_observers().unwrap();
            // observer.load_symbols(|record| {
            //     let base_sym = record.get(0).unwrap();
            //     let quote_sym = record.get(1).unwrap();
            //     Some(ExchangeSymbol::from(base_sym, quote_sym))
            // });

            observer.load_symbols(|record| {
                let base = record.get(0).unwrap();
                let quote = record.get(1).unwrap();
                let pair_name = record.get(2).unwrap();

                let min_price = Decimal::from_str(record.get(3).unwrap()).unwrap();
                let base_precision = u8::from_str(record.get(4).unwrap()).unwrap();
                let qty_step_size = Decimal::from_str(record.get(5).unwrap()).unwrap();
                let price_tick_size = Decimal::from_str(record.get(6).unwrap()).unwrap();
                let min_notional = Decimal::from_str(record.get(7).unwrap()).unwrap();
                let min_qty = Decimal::from_str(record.get(8).unwrap()).unwrap();

                Some(ArbitrageExchangeSymbol::new(
                    base,
                    quote,
                    pair_name,
                    min_price,
                    base_precision,
                    qty_step_size,
                    price_tick_size,
                    min_notional,
                    min_qty,
                ))
            });
            observer.launch().unwrap();
        }

        // Start RPC service if configured
        let rpc_handle = if let Some(ref rpc_config) = self.config.rpc {
            let mut rpc_observer = ObserverRpcRunner::new(&self.main_observer, rpc_config.clone());
            Some(self.runtime.spawn(async move {
                rpc_observer.run().await;
            }))
        } else {
            None
        };

        let ws_handle = if let Some(ref ws_config) = self.config.ws {
            let mut ws_observer = ObserverWsRunner::new(
                &self.main_observer,
                self.runtime.clone(),
                ws_config.clone(),
                self.config.observer.clone(),
            );

            Some(self.runtime.spawn(async move {
                ws_observer.run().await;
            }))
        } else {
            None
        };

        // Start WS service if configured

        // Block main thread until all services are done
        self.runtime.block_on(async {
            if let Some(ws_handle) = ws_handle {
                info!("Awaiting ws handle");
                ws_handle.await.unwrap();
            }

            if let Some(rpc_handle) = rpc_handle {
                info!("Awaiting rpc handle");
                rpc_handle.await.unwrap();
            }
        });

        Ok(())
    }
}
