use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{Arc, RwLock}
};
use log::info;
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};
use exch_observer_types::{
    ExchangeObserver, ExchangeObserverKind, ExchangeSymbol, ExchangeClient
};
use exch_observer_config::ObserverConfig;
use exch_subobservers::CombinedObserver;
use exch_observer_rpc::ObserverRpcRunner;
use exch_clients::BinanceClient;

pub struct ObserverRunner {
    pub main_observer: Arc<RwLock<CombinedObserver>>,
    pub config: ObserverConfig,
    pub runtime: Arc<Runtime>
}

impl ObserverRunner {
    pub fn new(config: ObserverConfig) -> Self {
        let async_runtime = Arc::new(
            RuntimeBuilder::new_multi_thread()
                .worker_threads(config.num_threads.unwrap_or(4))
                .build()
                .unwrap()
        );
        let observer = Arc::new(
            RwLock::new(
                CombinedObserver::new(config.clone(), async_runtime.clone())
            )
        );

        Self {
            main_observer: observer,
            config: config,
            runtime: async_runtime
        }
    }

    pub fn get_async_runner(&self) -> &'_ Arc<Runtime> {
        return &self.runtime;
    }

    pub fn launch(&mut self) -> Result<(), Box<dyn std::error::Error>>{
        let mut observer_binding = self.main_observer.write().unwrap();
        let observer_handle = observer_binding.launch();

        // let mut rpc_observer = ObserverRpcRunner::new(&self.main_observer, self.config.rpc.clone().unwrap());
        // let rpc_handle = Some(self.runtime.spawn(async move { rpc_observer.run(); }));
        let rpc_handle = if let Some(ref rpc_config) = self.config.rpc {
            let mut rpc_observer = ObserverRpcRunner::new(&self.main_observer, rpc_config.clone());
            Some(self.runtime.spawn(async move { rpc_observer.run().await; }))
        } else {
            None
        };

        self.runtime.block_on(async {
            info!("Awaiting observer handle");
            observer_handle.await.unwrap();
            if let Some(rpc_handle) = rpc_handle {
                info!("Awaiting rpc handle");
                rpc_handle.await.unwrap();
            }
        });

        Ok(())
    }
}
