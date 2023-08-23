use serde::{Deserialize, Serialize};
/**
 * `kraken_ws.rs` file containts functions and structs useful
 * to connect to kraken Websocket API.
 **/
use std::{
    cmp::PartialEq,
    net::TcpStream,
    sync::atomic::{AtomicBool, Ordering},
};
use tungstenite::{
    connect, handshake::client::Response, protocol::WebSocket, stream::MaybeTlsStream, Message,
};

use crate::common::{BookTick, KLine, Result as WsResult, WebSocketError, WebsocketEvent};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenAskBidData {
    price: String,
    whole_lot_volume: i64,
    lot_volume: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenBookTickData {
    #[serde(rename = "a")]
    ask: KrakenAskBidData,
    #[serde(rename = "b")]
    bid: KrakenAskBidData,
    #[serde(rename = "c")]
    close: Vec<String>,
    #[serde(rename = "v")]
    volume: Vec<String>,
    #[serde(rename = "p")]
    volume_weighted_average_price: Vec<String>,
    #[serde(rename = "t")]
    number_of_trades: Vec<i32>,
    #[serde(rename = "l")]
    low: Vec<String>,
    #[serde(rename = "h")]
    high: Vec<String>,
    #[serde(rename = "o")]
    open: Vec<String>,
}

// [
//   340,
//   {
//     "a":["26870.00000",0,"0.95697355"],"b":["26869.90000",1,"1.18808196"],"c":["26869.90000","0.00185950"],"v":["69.72579216","2050.18198936"],"p":["26879.26763","26901.26063"],"t":[2462,27770],"l":["26834.20000","26635.30000"],"h":["26918.90000","27174.80000"],"o":["26887.40000","26851.20000"]
//     },
//     "ticker","XBT/USD"
// ]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenBookTickerEvent {
    channel_id: i64,
    data: KrakenBookTickData,
    channel: String,
    symbol: String,
}

impl Into<BookTick> for KrakenBookTickerEvent {
    fn into(self) -> BookTick {
        let best_ask = self.data.ask.price.parse::<f64>().unwrap();
        let best_bid = self.data.bid.price.parse::<f64>().unwrap();

        BookTick {
            sym: self.symbol,
            best_ask: best_ask,
            best_bid: best_bid,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenOHLCData {
    time: String,
    end_time: String,
    open: String,
    high: String,
    low: String,
    close: String,
    vwap: String,
    volume: String,
    count: i32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenOHLCEvent {
    channel_id: i64,
    data: KrakenOHLCData,
    channel: String,
    symbol: String,
}

impl Into<KLine> for KrakenOHLCEvent {
    fn into(self) -> KLine {
        KLine {
            sym: self.symbol,
            open: self.data.open.parse::<f64>().unwrap(),
            close: self.data.close.parse::<f64>().unwrap(),
            low: self.data.low.parse::<f64>().unwrap(),
            high: self.data.high.parse::<f64>().unwrap(),
            vol: self.data.volume.parse::<f64>().unwrap(),
            count: self.data.count as f64,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenSystemStatusEvent {
    #[serde(rename = "connectionID")]
    connection_id: u64,
    event: String,
    status: String,
    version: String,
}

// {"event":"heartbeat"}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenHeartbeatEvent {
    event: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenSubscription {
    depth: Option<i64>,
    interval: Option<i64>,
    maxratecount: Option<i64>,
    name: String,
    token: Option<String>,
}

impl KrakenSubscription {
    #[allow(dead_code)]
    pub fn with_name(name: String) -> Self {
        Self {
            depth: None,
            interval: None,
            maxratecount: None,
            name: name,
            token: None,
        }
    }
}

// {"channelID":564,"channelName":"ticker","event":"subscriptionStatus","pair":"ETH/USD","status":"subscribed","subscription":{"name":"ticker"}}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenSubscriptionStatusEvent {
    #[serde(rename = "channelID")]
    channel_id: i64,
    #[serde(rename = "channelName")]
    channel_name: String,
    event: String,
    pair: String,
    status: String,
    subscription: KrakenSubscription,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenSpreadData {
    bid: String,
    ask: String,
    timestamp: String,
    #[serde(rename = "bidVolume")]
    bid_volume: String,
    #[serde(rename = "askVolume")]
    ask_volume: String,
}

// [53,["1.34948","1.35017","1684769251.100692","1447.38977961","188.82000000"],"spread","USD/CAD"]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct KrakenSpreadEvent {
    channel_id: i64,
    data: KrakenSpreadData,
    channel: String,
    symbol: String,
}

impl Into<BookTick> for KrakenSpreadEvent {
    fn into(self) -> BookTick {
        let best_bid = self.data.bid.parse::<f64>().unwrap();
        let best_ask = self.data.ask.parse::<f64>().unwrap();

        BookTick {
            sym: self.symbol,
            best_ask: best_ask,
            best_bid: best_bid,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(untagged)]
enum KrakenWebsocketEvent {
    SystemStatusEvent(KrakenSystemStatusEvent),
    SpreadEvent(KrakenSpreadEvent),
    BookTickerEvent(KrakenBookTickerEvent),
    SubscriptionStatusEvent(KrakenSubscriptionStatusEvent),
    OHLCEvent(KrakenOHLCEvent),
    HeartbeatEvent(KrakenHeartbeatEvent),
}

/// Kraken base URL for websocket API
const KRAKEN_WS_URL: &str = "wss://ws.kraken.com";

pub struct KrakenWebsocket<'a> {
    pub socket: Option<(WebSocket<MaybeTlsStream<TcpStream>>, Response)>,
    handler: Box<dyn FnMut(WebsocketEvent) -> WsResult<()> + 'a>,
}

impl<'a> KrakenWebsocket<'a> {
    /// Creates new KrakenWebsocket with provided function to handle events
    pub fn new<Callback>(handler: Callback) -> Self
    where
        Callback: FnMut(WebsocketEvent) -> WsResult<()> + 'a,
    {
        Self {
            socket: None,
            handler: Box::new(handler),
        }
    }

    pub fn connect_multiple_streams(&mut self, subscriptions: Vec<String>) -> WsResult<()> {
        if self.socket.is_none() {
            self.socket = Some(self.create_connection(KRAKEN_WS_URL)?);
        }

        self.connect_multiple_ws(subscriptions)
    }

    /// Connects to Kraken Websocket API with provided single pair
    #[allow(dead_code)]
    fn connect_ws(&mut self, subscription: &str) -> WsResult<()> {
        if let Some(ref mut socket) = self.socket {
            socket
                .0
                .send(Message::Text(format!(
                    "{{
                \"event\":\"subscribe\",
                \"pair\":[\"{}\"],
                \"subscription\":{{
                    \"name\":\"spread\"
                }}
            }}",
                    subscription
                )))
                .map_err(|e| WebSocketError::WriteError(e.to_string()))?;
        }

        Ok(())
    }

    fn connect_multiple_ws(&mut self, subscriptions: Vec<String>) -> WsResult<()> {
        if let Some(ref mut socket) = self.socket {
            socket
                .0
                .send(Message::Text(String::from(format!(
                    "{{
                \"event\": \"subscribe\",
                \"pair\": {},
                \"subscription\": {{
                    \"name\": \"spread\"
                }}
            }}",
                    serde_json::to_string(&subscriptions).unwrap()
                ))))
                .unwrap();
        }

        Ok(())
    }

    fn create_connection(
        &self,
        url: &str,
    ) -> WsResult<(WebSocket<MaybeTlsStream<TcpStream>>, Response)> {
        connect(url).map_err(|e| WebSocketError::SocketError(e.to_string()))
    }

    /// Main event loop for Kraken WebSocket API
    pub fn event_loop(&mut self, running: &AtomicBool) -> WsResult<()> {
        // Only start the connection if running flag is true
        while running.load(Ordering::Relaxed) {
            if let Some(ref mut socket) = self.socket {
                let msg = socket
                    .0
                    .read()
                    .map_err(|e| WebSocketError::ReadError(e.to_string()))?;
                match msg {
                    Message::Text(text) => {
                        let event: KrakenWebsocketEvent = serde_json::from_str(&text).unwrap();

                        let ws_event = match event {
                            KrakenWebsocketEvent::BookTickerEvent(e) => {
                                Some(WebsocketEvent::BookTickerEvent(e.into()))
                            }
                            KrakenWebsocketEvent::OHLCEvent(e) => {
                                Some(WebsocketEvent::KLineEvent(e.into()))
                            }
                            KrakenWebsocketEvent::SpreadEvent(e) => {
                                Some(WebsocketEvent::BookTickerEvent(e.into()))
                            }
                            _ => None, // Ignore other events
                        };

                        if let Some(ws_event) = ws_event {
                            (self.handler)(ws_event).unwrap();
                        }
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
fn test_book_ticker_event_deserialization_from_json() {
    let json = r#"[340,{"a":["26870.00000",0,"0.95697355"],"b":["26869.90000",1,"1.18808196"],"c":["26869.90000","0.00185950"],"v":["69.72579216","2050.18198936"],"p":["26879.26763","26901.26063"],"t":[2462,27770],"l":["26834.20000","26635.30000"],"h":["26918.90000","27174.80000"],"o":["26887.40000","26851.20000"]},"ticker","XBT/USD"]"#;
    let event: KrakenWebsocketEvent = serde_json::from_str(json).unwrap();

    let expected = KrakenWebsocketEvent::BookTickerEvent(KrakenBookTickerEvent {
        channel_id: 340,
        data: KrakenBookTickData {
            ask: KrakenAskBidData {
                price: "26870.00000".to_string(),
                whole_lot_volume: 0,
                lot_volume: "0.95697355".to_string(),
            },
            bid: KrakenAskBidData {
                price: "26869.90000".to_string(),
                whole_lot_volume: 1,
                lot_volume: "1.18808196".to_string(),
            },
            close: vec!["26869.90000".to_string(), "0.00185950".to_string()],
            volume: vec!["69.72579216".to_string(), "2050.18198936".to_string()],
            volume_weighted_average_price: vec![
                "26879.26763".to_string(),
                "26901.26063".to_string(),
            ],
            number_of_trades: vec![2462, 27770],
            low: vec!["26834.20000".to_string(), "26635.30000".to_string()],
            high: vec!["26918.90000".to_string(), "27174.80000".to_string()],
            open: vec!["26887.40000".to_string(), "26851.20000".to_string()],
        },
        channel: "ticker".to_string(),
        symbol: "XBT/USD".to_string(),
    });

    assert_eq!(event, expected);
}

// [
//   42,
//   [
//     "1542057314.748456",
//     "1542057360.435743",
//     "3586.70000",
//     "3586.70000",
//     "3586.60000",
//     "3586.60000",
//     "3586.68894",
//     "0.03373000",
//     2
//   ],
//   "ohlc-5",
//   "XBT/USD"
// ]
#[test]
fn test_ohlc_deserialization_from_json() {
    let json = r#"[42,["1542057314.748456","1542057360.435743","3586.70000","3586.70000","3586.60000","3586.60000","3586.68894","0.03373000",2],"ohlc-5","XBT/USD"]"#;
    let event: KrakenWebsocketEvent = serde_json::from_str(json).unwrap();

    let expected = KrakenWebsocketEvent::OHLCEvent(KrakenOHLCEvent {
        channel_id: 42,
        data: KrakenOHLCData {
            time: "1542057314.748456".to_string(),
            end_time: "1542057360.435743".to_string(),
            open: "3586.70000".to_string(),
            high: "3586.70000".to_string(),
            low: "3586.60000".to_string(),
            close: "3586.60000".to_string(),
            vwap: "3586.68894".to_string(),
            volume: "0.03373000".to_string(),
            count: 2,
        },
        channel: "ohlc-5".to_string(),
        symbol: "XBT/USD".to_string(),
    });

    assert_eq!(event, expected);
}

// {"connectionID":15775968476884074414,"event":"systemStatus","status":"online","version":"1.9.1"}
#[test]
fn test_system_status_from_json() {
    let json = r#"{"connectionID":15775968476884074414,"event":"systemStatus","status":"online","version":"1.9.1"}"#;
    let event: KrakenWebsocketEvent = serde_json::from_str(json).unwrap();

    let expected = KrakenWebsocketEvent::SystemStatusEvent(KrakenSystemStatusEvent {
        connection_id: 15775968476884074414,
        event: "systemStatus".to_string(),
        status: "online".to_string(),
        version: "1.9.1".to_string(),
    });

    assert_eq!(event, expected);
}

// {"event":"heartbeat"}
#[test]
fn test_heartbeat_from_json() {
    let json = r#"{"event":"heartbeat"}"#;
    let event: KrakenWebsocketEvent = serde_json::from_str(json).unwrap();

    let expected = KrakenWebsocketEvent::HeartbeatEvent(KrakenHeartbeatEvent {
        event: "heartbeat".to_string(),
    });

    assert_eq!(event, expected);
}

// {"channelID":564,"channelName":"ticker","event":"subscriptionStatus","pair":"ETH/USD","status":"subscribed","subscription":{"name":"ticker"}}
#[test]
fn test_subscription_status_from_json() {
    let json = r#"{"channelID":564,"channelName":"ticker","event":"subscriptionStatus","pair":"ETH/USD","status":"subscribed","subscription":{"name":"ticker"}}"#;
    let event: KrakenWebsocketEvent = serde_json::from_str(json).unwrap();

    let expected = KrakenWebsocketEvent::SubscriptionStatusEvent(KrakenSubscriptionStatusEvent {
        channel_id: 564,
        channel_name: "ticker".to_string(),
        event: "subscriptionStatus".to_string(),
        pair: "ETH/USD".to_string(),
        status: "subscribed".to_string(),
        subscription: KrakenSubscription::with_name("ticker".to_string()),
    });

    assert_eq!(event, expected);
}

// [53,["1.34948","1.35017","1684769251.100692","1447.38977961","188.82000000"],"spread","USD/CAD"]
#[test]
fn test_spread_from_json() {
    let json = r#"[53,["1.34948","1.35017","1684769251.100692","1447.38977961","188.82000000"],"spread","USD/CAD"]"#;
    let event: KrakenWebsocketEvent = serde_json::from_str(json).unwrap();

    let expected = KrakenWebsocketEvent::SpreadEvent(KrakenSpreadEvent {
        channel_id: 53,
        data: KrakenSpreadData {
            bid: "1.34948".to_string(),
            ask: "1.35017".to_string(),
            timestamp: "1684769251.100692".to_string(),
            bid_volume: "1447.38977961".to_string(),
            ask_volume: "188.82000000".to_string(),
        },
        channel: "spread".to_string(),
        symbol: "USD/CAD".to_string(),
    });

    assert_eq!(event, expected);
}
