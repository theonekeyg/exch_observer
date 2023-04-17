mod observer_rpc {
    include!("myproto/exch_observer_rpc.rs");
}
use observer_rpc::{
    exch_observer_server::{ExchObserver, ExchObserverServer},
    exch_observer_client::ExchObserverClient,
    GetPriceRequest, GetPriceResponse,
};

use std::{
    sync::{Arc, RwLock},
    net::SocketAddr,
    marker::Sync
};
use exch_subobservers::CombinedObserver;
use exch_observer_config::RpcConfig;
use exch_observer_types::{ExchangeSymbol, ExchangeObserverKind};
use tonic::{
    transport::Server,
    Request, Response, Status
};
use log::{info, debug};


pub struct GrpcObserver {
    observer: Arc<RwLock<CombinedObserver>>,
}

impl GrpcObserver {
    pub fn new(observer: Arc<RwLock<CombinedObserver>>) -> Self {
        Self { observer: observer.clone() }
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
        info!(
            "processing request for symbol {}{}",
            request.get_ref().base, request.get_ref().quote
        );

        let observer = self.observer.read().unwrap();
        let request = request.into_inner();
        let symbol = ExchangeSymbol::from(&request.base, &request.quote);
        let exchange = match request.exchange.as_str() {
            "binance" => ExchangeObserverKind::Binance,
            "bitfinex" => ExchangeObserverKind::Bitfinex,
            "bitmex" => ExchangeObserverKind::Bitmex,
            "bittrex" => ExchangeObserverKind::Bittrex,
            "coinbase" => ExchangeObserverKind::Coinbase,
            "derbit" => ExchangeObserverKind::Deribit,
            "ftx" => ExchangeObserverKind::Ftx,
            "huobi" => ExchangeObserverKind::Huobi,
            "kraken" => ExchangeObserverKind::Kraken,
            "okex" => ExchangeObserverKind::Okex,
            "poloniex" => ExchangeObserverKind::Poloniex,
            "uniswap" => ExchangeObserverKind::Uniswap,
            _ => ExchangeObserverKind::Unknown,
        };

        info!(
            "Received price request for symbol {} on exchange {}",
            symbol, request.exchange
        );
        let price = observer.get_price(exchange, &symbol).unwrap_or(0.0);

        Ok(Response::new(GetPriceResponse {
            base: request.base,
            quote: request.quote,
            price: price
        }))
    }
}

pub struct ObserverRpcRunner {
    rpc: Arc<GrpcObserver>,
    config: RpcConfig,
}

unsafe impl Send for ObserverRpcRunner {}
unsafe impl Sync for ObserverRpcRunner {}

impl ObserverRpcRunner {
    pub fn new(observer: &Arc<RwLock<CombinedObserver>>, config: RpcConfig) -> Self {
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

        Self {
            inner: client
        }
    }

    pub async fn get_price(&mut self, exchange: &str, base: &str, quote: &str) -> f64 {
        debug!("Fetching price for symbol {}{} on exchange {}", base, quote, exchange);
        let res = self.inner.get_price(GetPriceRequest {
            exchange: exchange.to_string(),
            base: base.to_string(),
            quote: quote.to_string()
        }).await.unwrap();

        res.into_inner().price
    }
}
