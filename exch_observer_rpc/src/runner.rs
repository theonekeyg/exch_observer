mod observer_rpc {
    include!("myproto/exch_observer_rpc.rs");
}
use observer_rpc::{
    exch_observer_server::{ExchObserver, ExchObserverServer},
    GetPriceRequest, GetPriceResponse,
};

use std::{
    sync::{Arc, RwLock},
    net::SocketAddr,
    marker::Sync
};
use exch_observer::ObserverRunner;
use exch_observer_config::RpcConfig;
use exch_observer_types::{ExchangeSymbol, ExchangeObserverKind};
use tonic::{
    transport::Server,
    Request, Response, Status
};

pub struct GrpcObserver {
    observer: Arc<RwLock<ObserverRunner>>,
}

impl GrpcObserver {
    pub fn new(observer: Arc<RwLock<ObserverRunner>>) -> Self {
        Self { observer: observer.clone() }
    }

    pub fn setup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let observer = self.observer.read().unwrap();

        if !observer.is_running {
            let mut observer = self.observer.write().unwrap();
            observer.launch();
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

impl ObserverRpcRunner {
    pub fn new(observer: &Arc<RwLock<ObserverRunner>>, config: RpcConfig) -> Self {
        Self {
            rpc: Arc::new(GrpcObserver::new(observer.clone())),
            config: config,
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let addr: SocketAddr = format!(
            "{}:{}", self.config.host.get_or_insert("[::1]".into()), self.config.port
        ).parse()?;

        Server::builder()
            .add_service(ExchObserverServer::from_arc(self.rpc.clone()))
            .serve(addr)
            .await?;

        Ok(())
    }
}
