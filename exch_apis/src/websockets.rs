use libflate::gzip::Decoder;

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

#[derive(Serialize, Deserialize, Debug)]
pub struct KLine {
    pub open: f64,

    pub close: f64,

    pub low: f64,

    pub high: f64,

    pub vol: f64,

    pub count: f64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BookTick {
    pub best_bid: f64,
    pub best_ask: f64,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum WebsocketEvent {
    KLineEvent(KLine),
    BookTickerEvent(BookTick)
}

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

impl Into<KLine> for HuobiKLine {
    fn into(self) -> KLine {
        KLine {
            open: self.open,
            close: self.close,
            low: self.low,
            high: self.high,
            vol: self.vol,
            count: self.count,
        }
    }
}

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

impl Into<BookTick> for HuobiBookTick {
    fn into(self) -> BookTick {
        BookTick {
            best_bid: self.bid,
            best_ask: self.ask,
        }
    }
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

// {"ch":"market.btcusdt.ticker","ts":1683877598657,"tick":{"open":27516.87,"high":27623.31,"low":26120.0,"close":26274.0,"amount":6489.818597098022,"vol":1.750934985679804E8,"count":141080,"bid":26275.21,"bidSize":0.2709,"ask":26275.22,"askSize":0.86,"lastPrice":26274.0,"lastSize":9.47E-4}}
#[derive(Serialize, Deserialize, Debug)]
struct HuobiBookTickerEvent {
    #[serde(rename = "ch")]
    pub channel: String,
    #[serde(rename = "ts")]
    pub system_time: u64,
    #[serde(rename = "tick")]
    pub tick: HuobiBookTick,
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

#[allow(dead_code)]
enum HuobiConnectionKind {
    Default,
    MultiStream,
    Custom(String),
}

const HUOBI_WS_URL: &str = "wss://api.huobi.pro/ws";
static HUOBI_UNIQUE_ID: AtomicU64 = AtomicU64::new(1);

pub struct HuobiWebsocket<'a> {
    pub socket: Option<(WebSocket<MaybeTlsStream<TcpStream>>, Response)>,
    handler: Box<dyn FnMut(WebsocketEvent) -> Result<(), Box<dyn std::error::Error>> + 'a>,
}

impl<'a> HuobiWebsocket<'a> {
    pub fn new<Callback>(handler: Callback) -> Self
    where
        Callback: FnMut(WebsocketEvent) -> Result<(), Box<dyn std::error::Error>> + 'a,
    {
        Self {
            socket: None,
            handler: Box::new(handler),
        }
    }

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

    pub fn event_loop(&mut self, running: &AtomicBool) -> WsResult<()> {
        while running.load(Ordering::Relaxed) {
            if let Some(ref mut socket) = self.socket {
                let msg = socket.0.read_message()?;
                match msg {
                    Message::Binary(bin) => {
                        let mut decoder = Decoder::new(&bin[..])?;
                        let mut text = String::new();
                        decoder.read_to_string(&mut text).unwrap();
                        let event: HuobiWebsocketEvent = serde_json::from_str(&text).unwrap();

                        let ws_event = match event {
                            HuobiWebsocketEvent::KLineEvent(event) => {
                                WebsocketEvent::KLineEvent(event.tick.into())
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
                                WebsocketEvent::BookTickerEvent(event.tick.into())
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
            let kline: KLine = kline_event.tick.into();
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
