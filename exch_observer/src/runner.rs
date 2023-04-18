use log::info;
use std::sync::{Arc, RwLock};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

use exch_observer_config::ExchObserverConfig;
use exch_observer_rpc::ObserverRpcRunner;
use exch_observer_types::ExchangeSymbol;
use exch_subobservers::CombinedObserver;

pub struct ObserverRunner {
    pub main_observer: Arc<RwLock<CombinedObserver<ExchangeSymbol>>>,
    pub config: ExchObserverConfig,
    pub runtime: Arc<Runtime>,
}

impl ObserverRunner {
    pub fn new(config: ExchObserverConfig) -> Self {
        let observer = Arc::new(RwLock::new(CombinedObserver::new(config.observer.clone())));

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

    pub fn get_async_runner(&self) -> &'_ Arc<Runtime> {
        return &self.runtime;
    }

    pub fn launch(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut observer = self.main_observer.write().unwrap();
            observer.create_observers().unwrap();
            observer.load_symbols(|record| {
                let base_sym = record.get(1).unwrap();
                let quote_sym = record.get(2).unwrap();
                Some(ExchangeSymbol::from(base_sym, quote_sym))
            });
            observer.launch().unwrap();
        }
        // let observer_handle = observer_binding.launch();

        // let mut rpc_observer = ObserverRpcRunner::new(&self.main_observer, self.config.rpc.clone().unwrap());
        // let rpc_handle = Some(self.runtime.spawn(async move { rpc_observer.run(); }));
        let rpc_handle = if let Some(ref rpc_config) = self.config.rpc {
            let mut rpc_observer = ObserverRpcRunner::new(&self.main_observer, rpc_config.clone());
            Some(self.runtime.spawn(async move {
                rpc_observer.run().await;
            }))
        } else {
            None
        };

        self.runtime.block_on(async {
            // info!("Awaiting observer handle");
            // observer_handle.await.unwrap();
            if let Some(rpc_handle) = rpc_handle {
                info!("Awaiting rpc handle");
                rpc_handle.await.unwrap();
            }
        });

        Ok(())
    }
}
