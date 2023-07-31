mod observer_rpc {
    include!("myproto/exch_observer_rpc.rs");
}
use observer_rpc::{
    exch_observer_client::ExchObserverClient,
    exch_observer_server::{ExchObserver, ExchObserverServer},
    GetPriceRequest, GetPriceResponse, GetSymbolsRequest, GetSymbolsResponse,
};

use exch_observer_config::RpcConfig;
use exch_observer_types::{ExchangeKind, ExchangeSymbol, ExchangeValues};
use exch_subobservers::CombinedObserver;
use log::{debug, info};
use std::{
    marker::Sync,
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, RwLock},
};
use tonic::{transport::Server, Request, Response, Status};

/// Struct representing the observer RPC service
pub struct GrpcObserver {
    observer: Arc<RwLock<CombinedObserver<ExchangeSymbol>>>,
}

impl GrpcObserver {
    pub fn new(observer: Arc<RwLock<CombinedObserver<ExchangeSymbol>>>) -> Self {
        Self {
            observer: observer.clone(),
        }
    }

    pub fn setup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let observer = self.observer.read().unwrap();

        if !observer.is_running {
            let mut observer = self.observer.write().unwrap();
            observer.launch().unwrap();
        }

        Ok(())
    }
}

unsafe impl Send for GrpcObserver {}
unsafe impl Sync for GrpcObserver {}

#[tonic::async_trait]
impl ExchObserver for GrpcObserver {
    async fn get_price(
        &self,
        request: Request<GetPriceRequest>,
    ) -> Result<Response<GetPriceResponse>, Status> {
        let observer = self.observer.read().unwrap();
        let request = request.into_inner();
        let symbol = ExchangeSymbol::from(&request.base, &request.quote);
        let exchange = ExchangeKind::from_str(&request.exchange).unwrap();

        debug!(
            "Received price request for symbol {} on exchange {}",
            symbol, request.exchange
        );

        // Get price from the observer
        let tmp_binding = observer.get_price(exchange, &symbol).unwrap();
        let _price = tmp_binding.lock().unwrap();

        // Format the price from the table for the RPC response
        let price = _price.showable_price();
        let timestamp = _price.update_timestamp();

        Ok(Response::new(GetPriceResponse {
            base: request.base,
            quote: request.quote,
            price: price,
            timestamp: timestamp,
        }))
    }

    async fn get_symbols(
        &self,
        request: Request<GetSymbolsRequest>,
    ) -> Result<Response<GetSymbolsResponse>, Status> {
        let observer = self.observer.read().unwrap();
        let request = request.into_inner();
        let exchange = ExchangeKind::from_str(&request.exchange).unwrap();

        debug!("Received symbols request for exchange {}", request.exchange);

        let symbols = observer
            .get_watching_symbols(exchange)
            .unwrap_or(&vec![])
            .iter()
            .map(|s| s.to_string())
            .collect();

        Ok(Response::new(GetSymbolsResponse { symbols: symbols }))
    }
}

/// Struct representing server for RPC service in `exch_observer`
pub struct ObserverRpcRunner {
    rpc: Arc<GrpcObserver>,
    config: RpcConfig,
}

unsafe impl Send for ObserverRpcRunner {}
unsafe impl Sync for ObserverRpcRunner {}

impl ObserverRpcRunner {
    pub fn new(
        observer: &Arc<RwLock<CombinedObserver<ExchangeSymbol>>>,
        config: RpcConfig,
    ) -> Self {
        let rpc = GrpcObserver::new(observer.clone());

        Self {
            rpc: Arc::new(rpc),
            config: config,
        }
    }

    pub async fn run(&mut self) {
        let addr: SocketAddr = format!(
            "{}:{}",
            self.config.host.as_ref().unwrap(),
            self.config.port.as_ref().unwrap()
        )
        .parse()
        .unwrap();
        info!("Starting RPC service on {}", addr);

        Server::builder()
            .add_service(ExchObserverServer::from_arc(self.rpc.clone()))
            .serve(addr)
            .await
            .unwrap();
    }
}

/// Client for the observer RPC service
pub struct ObserverRpcClient {
    inner: ExchObserverClient<tonic::transport::Channel>,
}

impl ObserverRpcClient {
    pub async fn new(config: RpcConfig) -> Self {
        let addr = format!(
            "http://{}:{}",
            config.host.as_ref().unwrap(),
            config.port.unwrap()
        );
        let client = ExchObserverClient::connect(addr).await.unwrap();

        Self { inner: client }
    }

    /// Fetch the price of a symbol on an observer
    ///
    /// # Arguments
    /// * `exchange` - The exchange to query
    /// * `base` - The base symbol
    /// * `quote` - The quote symbol
    pub async fn get_price(&mut self, exchange: &str, base: &str, quote: &str) -> f64 {
        debug!(
            "Fetching price for symbol {}{} on exchange {}",
            base, quote, exchange
        );
        let res = self
            .inner
            .get_price(GetPriceRequest {
                exchange: exchange.to_string(),
                base: base.to_string(),
                quote: quote.to_string(),
            })
            .await
            .unwrap();

        res.into_inner().price
    }

    /// Fetch the price and last update timestmap of a symbol on an observer
    ///
    /// # Arguments
    /// * `exchange` - The exchange to query
    /// * `base` - The base symbol
    /// * `quote` - The quote symbol
    pub async fn get_price_with_timestamp(
        &mut self,
        exchange: &str,
        base: &str,
        quote: &str,
    ) -> (f64, u64) {
        debug!(
            "Fetching price for symbol {}{} on exchange {}",
            base, quote, exchange
        );
        let res = self
            .inner
            .get_price(GetPriceRequest {
                exchange: exchange.to_string(),
                base: base.to_string(),
                quote: quote.to_string(),
            })
            .await
            .unwrap()
            .into_inner();

        (res.price, res.timestamp)
    }
}
