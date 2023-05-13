/**
 * The `websockets.rs` file contains the code for the required WebSocket API clients.
 * Currently only Huobi WebSocket API is implemented.
 **/
use libflate::gzip::Decoder;

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    net::TcpStream,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use tungstenite::{
    connect, error::Result as WsResult, handshake::client::Response, protocol::WebSocket,
    stream::MaybeTlsStream, Message,
};

/// Our internal struct to represent KLine data tick
#[derive(Serialize, Deserialize, Debug)]
pub struct KLine {
    pub open: f64,

    pub sym: String,

    pub close: f64,

    pub low: f64,

    pub high: f64,

    pub vol: f64,

    pub count: f64,
}

/// Our internal struct to represent Book data tick
#[derive(Serialize, Deserialize, Debug)]
pub struct BookTick {
    pub sym: String,
    pub best_bid: f64,
    pub best_ask: f64,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum WebsocketEvent {
    KLineEvent(KLine),
    BookTickerEvent(BookTick),
}

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

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum HuobiWebsocketEvent {
    KLineEvent(HuobiKlineEvent),
    BookTickerEvent(HuobiBookTickerEvent),
    PingEvent(HuobiPingEvent),
    StatusEvent(HuobiStatusEvent),
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
    handler: Box<dyn FnMut(WebsocketEvent) -> Result<(), Box<dyn std::error::Error>> + 'a>,
}

impl<'a> HuobiWebsocket<'a> {
    /// Creates new HuobiWebsocket with provided function to handle events
    pub fn new<Callback>(handler: Callback) -> Self
    where
        Callback: FnMut(WebsocketEvent) -> Result<(), Box<dyn std::error::Error>> + 'a,
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
            socket.0.write_message(Message::Text(format!(
                "{{\"sub\": \"{}\", \"id\": \"id{}\"}}",
                subscription,
                HUOBI_UNIQUE_ID.fetch_add(1, Ordering::Relaxed)
            )))?;
        }

        Ok(())
    }

    fn create_connection(
        &self,
        url: &str,
    ) -> WsResult<(WebSocket<MaybeTlsStream<TcpStream>>, Response)> {
        connect(url)
    }

    /// Main event loop for Huobi WebSocket API
    pub fn event_loop(&mut self, running: &AtomicBool) -> WsResult<()> {
        // Only start the server if running flag is true
        while running.load(Ordering::Relaxed) {
            if let Some(ref mut socket) = self.socket {
                let msg = socket.0.read_message()?;
                match msg {
                    Message::Binary(bin) => {
                        // Huobi WebSocket API only sends messages in binary encrypted gzip format,
                        // so we need to decompress the data before we can process it
                        let mut decoder = Decoder::new(&bin[..])?;
                        let mut text = String::new();
                        decoder.read_to_string(&mut text).unwrap();
                        let event: HuobiWebsocketEvent = serde_json::from_str(&text).unwrap();

                        let ws_event = match event {
                            HuobiWebsocketEvent::KLineEvent(event) => {
                                WebsocketEvent::KLineEvent(event.into())
                            }
                            HuobiWebsocketEvent::PingEvent(event) => {
                                // debug!("Received ping event: {}", event.ping);
                                socket.0.write_message(Message::Text(format!(
                                    "{{\"pong\":{}}}",
                                    event.ping
                                )))?;
                                continue;
                            }
                            HuobiWebsocketEvent::StatusEvent(_event) => {
                                // debug!("Received status event: {:?}", event);
                                continue;
                            }
                            HuobiWebsocketEvent::BookTickerEvent(event) => {
                                WebsocketEvent::BookTickerEvent(event.into())
                            }
                        };

                        (self.handler)(ws_event).unwrap();
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
