/**
 * The `websockets.rs` file contains the code for the required WebSocket API clients.
 * Currently only Huobi WebSocket API is implemented.
 **/
use libflate::gzip::Decoder;

use log::{error, trace, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    net::TcpStream,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use tungstenite::{
    connect, handshake::client::Response, protocol::WebSocket, stream::MaybeTlsStream, Message,
};

use crate::common::{BookTick, KLine, Result as WsResult, WebSocketError, WebsocketEvent};

/// Internal Huobi KLine ABI
#[derive(Serialize, Deserialize, Debug)]
struct HuobiKLine {
    pub id: u64,
    pub open: f64,
    pub close: f64,
    pub low: f64,
    pub high: f64,
    pub amount: f64,
    pub vol: f64,
    pub count: f64,
}

/// Internal Huobi BookTick ABI
#[derive(Serialize, Deserialize, Debug)]
struct HuobiBookTick {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub amount: f64,
    pub vol: f64,
    pub count: f64,
    pub bid: f64,
    #[serde(rename = "bidSize")]
    pub bid_size: f64,
    pub ask: f64,
    #[serde(rename = "askSize")]
    pub ask_size: f64,
    #[serde(rename = "lastPrice")]
    pub last_price: f64,
    #[serde(rename = "lastSize")]
    pub last_size: f64,
}

#[derive(Serialize, Deserialize, Debug)]
struct HuobiKlineEvent {
    #[serde(rename = "ch")]
    pub channel: String,
    #[serde(rename = "ts")]
    pub system_time: u64,
    #[serde(rename = "tick")]
    pub tick: HuobiKLine,
}

lazy_static! {
    /// Regex to extract symbol and interval from Huobi KLine channel
    static ref KLINE_SYMBOL_REGEX: Regex =
        Regex::new(r#"market\.(?P<symbol>\w+)\.kline\.(?P<interval>\w+)"#).unwrap();
}

impl Into<KLine> for HuobiKlineEvent {
    fn into(self) -> KLine {
        let caps = KLINE_SYMBOL_REGEX.captures(&self.channel).unwrap();
        let symbol = caps.name("symbol").unwrap().as_str();
        KLine {
            sym: symbol.to_string(),
            open: self.tick.open,
            close: self.tick.close,
            low: self.tick.low,
            high: self.tick.high,
            vol: self.tick.vol,
            count: self.tick.count,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct HuobiBookTickerEvent {
    #[serde(rename = "ch")]
    pub channel: String,
    #[serde(rename = "ts")]
    pub system_time: u64,
    #[serde(rename = "tick")]
    pub tick: HuobiBookTick,
}

lazy_static! {
    /// Regex to extract symbol from Huobi BookTick channel
    static ref BOOK_TICKER_SYMBOL_REGEX: Regex =
        Regex::new(r#"market\.(?P<symbol>\w+)\.ticker"#).unwrap();
}

impl Into<BookTick> for HuobiBookTickerEvent {
    fn into(self) -> BookTick {
        let caps = BOOK_TICKER_SYMBOL_REGEX.captures(&self.channel).unwrap();
        let symbol = caps.name("symbol").unwrap().as_str();
        BookTick {
            sym: symbol.to_string(),
            best_bid: self.tick.bid,
            best_ask: self.tick.ask,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct HuobiPingEvent {
    #[serde(rename = "ping")]
    pub ping: u64,
}

// {"id":"id10","status":"ok","subbed":"market.btcusdt.kline.1min","ts":1682461825574}
#[derive(Serialize, Deserialize, Debug)]
struct HuobiStatusEvent {
    pub id: String,
    pub status: String,
    pub subbed: String,
    pub ts: u64,
}

// {"status":"error","ts":1684650798105,"id":"id488","err-code":"bad-request","err-msg":"invalid symbol lmrusdt"}
#[derive(Serialize, Deserialize, Debug)]
struct HuobiErrorEvent {
    pub status: String,
    pub ts: u64,
    pub id: String,
    #[serde(rename = "err-code")]
    pub err_code: String,
    #[serde(rename = "err-msg")]
    pub err_msg: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum HuobiWebsocketEvent {
    KLineEvent(HuobiKlineEvent),
    BookTickerEvent(HuobiBookTickerEvent),
    PingEvent(HuobiPingEvent),
    StatusEvent(HuobiStatusEvent),
    ErrorEvent(HuobiErrorEvent),
}

/// Structure to provide multiple websocket connections at once,
/// not relevant for Huobi because Huobi WebSocket API uses single channel
/// for multiple symbols
#[allow(dead_code)]
enum HuobiConnectionKind {
    Default,
    MultiStream,
    Custom(String),
}

/// Huobi base URL for websocket API
const HUOBI_WS_URL: &str = "wss://api.huobi.pro/ws";
/// Used to generate unique IDs for Huobi subscriptions
static HUOBI_UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

/// Huobi WebSocket client
pub struct HuobiWebsocket<'a> {
    pub socket: Option<(WebSocket<MaybeTlsStream<TcpStream>>, Response)>,
    handler: Box<dyn FnMut(WebsocketEvent) + 'a>,
}

impl<'a> HuobiWebsocket<'a> {
    /// Creates new HuobiWebsocket with provided function to handle events
    pub fn new<Callback>(handler: Callback) -> Self
    where
        Callback: FnMut(WebsocketEvent) + 'a,
    {
        Self {
            socket: None,
            handler: Box::new(handler),
        }
    }

    /// Connects to Huobi WebSocket API with single subscription
    pub fn connect(&mut self, subscription: &str) -> WsResult<()> {
        if self.socket.is_none() {
            self.socket = Some(self.create_connection(HUOBI_WS_URL)?);
        }

        self.connect_ws(subscription)
    }

    pub fn connect_custom(&mut self, url: String, subscription: &str) -> WsResult<()> {
        if self.socket.is_none() {
            self.socket = Some(self.create_connection(url.as_ref())?);
        }

        self.connect_ws(subscription)
    }

    /// Connects to Huobi WebSocket API with multiple subscriptions
    pub fn connect_multiple_streams<S: Into<String>>(
        &mut self,
        subscriptions: Vec<S>,
    ) -> WsResult<()> {
        if self.socket.is_none() {
            self.socket = Some(self.create_connection(HUOBI_WS_URL)?);
        }

        for subscription in subscriptions {
            self.connect_ws(subscription.into().as_ref())?;
        }

        Ok(())
    }

    /// Sends a subscription message to the Huobi websocket stream, which is the
    /// documented way of subscribing to a stream.
    fn connect_ws(&mut self, subscription: &str) -> WsResult<()> {
        if let Some(ref mut socket) = self.socket {
            socket
                .0
                .send(Message::Text(format!(
                    "{{\"sub\": \"{}\", \"id\": \"id{}\"}}",
                    subscription,
                    HUOBI_UNIQUE_ID.fetch_add(1, Ordering::Relaxed)
                )))
                .map_err(|e| WebSocketError::WriteError(e.to_string()))?;
        }

        Ok(())
    }

    fn create_connection(
        &self,
        url: &str,
    ) -> WsResult<(WebSocket<MaybeTlsStream<TcpStream>>, Response)> {
        connect(url).map_err(|e| WebSocketError::SocketError(e.to_string()))
    }

    /// Main event loop for Huobi WebSocket API
    pub fn event_loop(&mut self, running: &AtomicBool) -> WsResult<()> {
        // Only start the connection if running flag is true
        while running.load(Ordering::Relaxed) {
            if let Some(ref mut socket) = self.socket {
                let msg = socket
                    .0
                    .read()
                    .map_err(|e| WebSocketError::ReadError(e.to_string()))?;
                match msg {
                    Message::Binary(bin) => {
                        // Huobi WebSocket API only sends messages in binary encrypted gzip format,
                        // so we need to decompress the data before we can process it
                        let mut decoder = Decoder::new(&bin[..]).unwrap();
                        let mut text = String::new();
                        decoder.read_to_string(&mut text).unwrap();
                        let event = serde_json::from_str::<HuobiWebsocketEvent>(&text);

                        let event = match event {
                            Ok(event) => event,
                            Err(e) => {
                                error!("Received invalid JSON: {}", e);
                                continue;
                            }
                        };

                        let ws_event = match event {
                            HuobiWebsocketEvent::KLineEvent(event) => {
                                WebsocketEvent::KLineEvent(event.into())
                            }
                            HuobiWebsocketEvent::PingEvent(event) => {
                                trace!("Received ping event: {}", event.ping);
                                socket
                                    .0
                                    .send(Message::Text(format!("{{\"pong\":{}}}", event.ping)))
                                    .map_err(|e| WebSocketError::WriteError(e.to_string()))?;
                                continue;
                            }
                            HuobiWebsocketEvent::StatusEvent(event) => {
                                trace!("Received status event: {:?}", event);
                                continue;
                            }
                            HuobiWebsocketEvent::BookTickerEvent(event) => {
                                WebsocketEvent::BookTickerEvent(event.into())
                            }
                            HuobiWebsocketEvent::ErrorEvent(event) => {
                                warn!("Received error event: {:?}", event);
                                continue;
                            }
                        };

                        (self.handler)(ws_event);
                    }
                    _ => {
                        panic!("Received some other message than binary: {:?}", msg);
                    }
                }
            }
        }

        Ok(())
    }
}

#[test]
fn test_huobi_kline() {
    let json = r#"{
        "ch": "market.btcusdt.kline.1min",
        "ts": 1560000000000,
        "tick": {
            "id": 1560000000,
            "open": 10000,
            "close": 11006,
            "low": 10000,
            "high": 11023,
            "amount": 10000,
            "vol": 6141235.513,
            "count": 10000
        }
    }"#;

    let event: HuobiWebsocketEvent = serde_json::from_str(json).unwrap();
    match event {
        HuobiWebsocketEvent::KLineEvent(kline_event) => {
            let kline: KLine = kline_event.into();
            assert_eq!(kline.open, 10000.0);
            assert_eq!(kline.close, 11006.0);
            assert_eq!(kline.low, 10000.0);
            assert_eq!(kline.high, 11023.0);
            assert_eq!(kline.vol, 6141235.513);
            assert_eq!(kline.count, 10000.0);
        }
        _ => {
            panic!("Unexpected event type");
        }
    }
}

#[test]
fn test_huobi_ping() {
    let json = r#"{"ping":1682445630232}"#;
    let event: HuobiWebsocketEvent = serde_json::from_str(json).unwrap();
    match event {
        HuobiWebsocketEvent::PingEvent(ping_event) => {
            assert_eq!(ping_event.ping, 1682445630232);
        }
        _ => {
            panic!("Unexpected event type");
        }
    }
}

#[test]
fn test_huobi_book_ticker() {
    let json = r#"{
        "ch": "market.btcusdt.ticker",
        "ts": 1683877598657,
        "tick": {
            "open": 27516.87,
            "high": 27623.31,
            "low": 26120.0,
            "close": 26274.0,
            "amount": 6489.818597098022,
            "vol": 1.750934985679804E8,
            "count": 141080,
            "bid": 26275.21,
            "bidSize": 0.2709,
            "ask": 26275.22,
            "askSize": 0.86,
            "lastPrice": 26274.0,
            "lastSize": 9.47E-4
        }
    }"#;
    let event: HuobiWebsocketEvent = serde_json::from_str(json).unwrap();

    match event {
        HuobiWebsocketEvent::BookTickerEvent(book_ticker_event) => {
            assert_eq!(book_ticker_event.channel, "market.btcusdt.ticker");
            assert_eq!(book_ticker_event.system_time, 1683877598657);
            assert_eq!(book_ticker_event.tick.bid, 26275.21);
            assert_eq!(book_ticker_event.tick.bid_size, 0.2709);
            assert_eq!(book_ticker_event.tick.ask, 26275.22);
            assert_eq!(book_ticker_event.tick.ask_size, 0.86);
            assert_eq!(book_ticker_event.tick.last_price, 26274.0);
            assert_eq!(book_ticker_event.tick.last_size, 9.47E-4);
        }
        _ => {
            panic!("Unexpected event type");
        }
    }
}

// {"status":"error","ts":1684650798105,"id":"id488","err-code":"bad-request","err-msg":"invalid symbol lmrusdt"}
#[test]
fn test_error_event_from_json() {
    let json = r#"{
        "status":"error",
        "ts":1684650798105,
        "id":"id488",
        "err-code":"bad-request",
        "err-msg":"invalid symbol lmrusdt"
    }"#;
    let event: HuobiWebsocketEvent = serde_json::from_str(json).unwrap();

    match event {
        HuobiWebsocketEvent::ErrorEvent(error_event) => {
            assert_eq!(error_event.status, "error");
            assert_eq!(error_event.ts, 1684650798105);
            assert_eq!(error_event.id, "id488");
            assert_eq!(error_event.err_code, "bad-request");
            assert_eq!(error_event.err_msg, "invalid symbol lmrusdt");
        }
        _ => {
            panic!("Unexpected event type");
        }
    }
}

/*
#[test]
fn test_huobi_blocking_connection() {
    let mut ws = HuobiWebsocket::new(|event| {
        println!("Received event: {:?}", event);
        Ok(())
    });

    ws.connect("market.ethbtc.kline.1min").unwrap();
    ws.event_loop(&AtomicBool::new(true)).unwrap();
}
*/
