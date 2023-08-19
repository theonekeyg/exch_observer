use std::{
    cmp::PartialEq,
    net::TcpStream,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex
    },
    collections::HashMap,
};
use exch_observer_types::{
    ExchangeKind, AskBidValues, PriceUpdateEvent
};
use exch_observer_config::{
    ObserverConfig, WsConfig
};
use tungstenite::{
    connect, handshake::client::Response, protocol::WebSocket, stream::MaybeTlsStream, Message,
};
use ringbuf::{
    HeapRb, SharedRb,
    producer::Producer,
    consumer::Consumer
};
use log::error;
use tokio::{task::JoinHandle, runtime::{Runtime}};

struct WsObserverThreadData {
    /// Producer of the SPSC queue
    pub rx: Producer<PriceUpdateEvent, Arc<HeapRb<PriceUpdateEvent>>>,
    /// URL to the ws server
    pub ws_uri: String,
    /// Exchange kind
    pub exchange: ExchangeKind,
}

impl WsObserverThreadData {
    pub fn new(
        rx: Producer<PriceUpdateEvent, Arc<HeapRb<PriceUpdateEvent>>>,
        ws_uri: String,
        exchange: ExchangeKind,
    ) -> Self {
        Self {
            rx: rx,
            ws_uri: ws_uri,
            exchange: exchange,
        }
    }
}

/// This structure represents remote exch_observer server with WS interface enabled.
/// It is used to connect to remove WS server and listen to updates on it.
/// Push the updates into the local SPSC queue.
pub struct WsRemoteObserver {
    /// Internal map that stores websocket connections to the remote server.
    ws_sockets: HashMap<ExchangeKind, WebSocket<MaybeTlsStream<TcpStream>>>,

    /// Internal map that stores necessary info for each exchange
    thread_data_vec: Vec<WsObserverThreadData>,

    /// Must be used by the used of WsRemoteObserver to get consumer of the queues.
    /// It fills from the config in the constructor and must be consumed by the higher
    /// application, for example, by using `HashMap::drain`.
    pub rx_map: HashMap<ExchangeKind, Consumer<PriceUpdateEvent, Arc<HeapRb<PriceUpdateEvent>>>>,

    /// Configs for individual observers
    pub obs_config: ObserverConfig,

    /// Websocket config
    pub ws_config: WsConfig,

    jobs: Vec<JoinHandle<()>>,

    pub runtime: Arc<Runtime>,
}

impl WsRemoteObserver {
    pub fn new(runtime: Arc<Runtime>, obs_config: ObserverConfig, ws_config: WsConfig) -> Self {

        let mut thread_data_vec = Vec::new();
        let mut rx_map = HashMap::new();

        // Create SPSC queues for each exchange that is enabled in the config.
        // Add binance
        if let Some(config) = &obs_config.binance {
            if config.enable {
                let (tx, rx) = HeapRb::<PriceUpdateEvent>::new(1000).split();
                let thread_data = WsObserverThreadData::new(
                    tx,
                    format!("ws://{}:{}", ws_config.host, config.ws_port),
                    ExchangeKind::Binance,
                );

                thread_data_vec.push(thread_data);

                rx_map.insert(ExchangeKind::Binance, rx);
            }
        }

        // Add huobi
        if let Some(config) = &obs_config.huobi {
            if config.enable {
                let (tx, rx) = HeapRb::<PriceUpdateEvent>::new(1000).split();
                let thread_data = WsObserverThreadData::new(
                    tx,
                    format!("ws://{}:{}", ws_config.host, config.ws_port),
                    ExchangeKind::Huobi,
                );

                thread_data_vec.push(thread_data);
                rx_map.insert(ExchangeKind::Huobi, rx);
            }
        }

        // Add kraken
        if let Some(config) = &obs_config.kraken {
            if config.enable {
                let (tx, rx) = HeapRb::<PriceUpdateEvent>::new(1000).split();
                let thread_data = WsObserverThreadData::new(
                    tx,
                    format!("ws://{}:{}", ws_config.host, config.ws_port),
                    ExchangeKind::Kraken
                );

                thread_data_vec.push(thread_data);
                rx_map.insert(ExchangeKind::Kraken, rx);
            }
        }

        Self {
            ws_sockets: HashMap::new(),
            thread_data_vec: thread_data_vec,
            rx_map: rx_map,
            obs_config: obs_config,
            ws_config: ws_config,
            jobs: vec![],
            runtime: runtime
        }
    }

    fn spawn_ws_handler(runtime: Arc<Runtime>, mut thread_data: WsObserverThreadData) -> JoinHandle<()> {
        runtime.spawn_blocking(move || {
            let (mut ws_stream, _) = connect(&thread_data.ws_uri)
                .expect(
                    format!("Failed to connect to ws server: {}", thread_data.ws_uri).as_str()
                );

            loop {
                let res = ws_stream.read().unwrap();

                match &res {
                    Message::Text(text) => {
                        // Deserialize the message and push it into the queue
                        // let event: PriceUpdateEvent = serde_json::from_str(text);
                        if let Ok(event) = serde_json::from_str::<PriceUpdateEvent>(text) {
                            thread_data
                                .rx
                                .push(event)
                                .expect("Failed to push event into the queue");
                        } else if let Ok(mut event) = serde_json::from_str::<Vec<PriceUpdateEvent>>(text) {
                            for e in event.drain(0..) {
                                thread_data
                                    .rx
                                    .push(e.clone())
                                    .expect("Failed to push event into the queue");
                            }
                        } else {
                            error!("Failed to deserialize the message: {}", text);
                        }
                    },
                    _ => {}
                }

                println!("Received a message from the server! {:?}", res);
            }
        })
    }

    // Start the main WS client application loop.
    pub fn start(&mut self) {
        for thread_data in self.thread_data_vec.drain(0..) {
            self.jobs.push(Self::spawn_ws_handler(self.runtime.clone(), thread_data));
        }
    }

    pub fn stop(&mut self) {
        for job in &mut self.jobs {
            println!("Aborting task");
            job.abort();
        }
    }
}

#[cfg(test)]
pub mod test {

    pub use super::*;

    use tokio::runtime::Builder as RuntimeBuilder;
    use exch_observer_config::BinanceConfig;

    #[test]
    fn test_basic_run() {
        let runtime = Arc::new(RuntimeBuilder::new_multi_thread().build().unwrap());
        let obs_config = ObserverConfig {
            binance: Some(BinanceConfig::new_with_csv_symbols("".to_string())),
            huobi: None,
            kraken: None,
        };
        let ws_config = WsConfig::default();

        let mut ws_remote_observer = WsRemoteObserver::new(runtime, obs_config, ws_config);
        ws_remote_observer.start();

        // sleep for 5 seconds
        std::thread::sleep(std::time::Duration::from_secs(5));
        ws_remote_observer.stop();
        panic!("Test panic");
    }
}
