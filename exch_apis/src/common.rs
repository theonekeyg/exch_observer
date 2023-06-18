use serde::{Deserialize, Serialize};
use thiserror::Error;

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

/// Structure to provide multiple websocket connections at once,
/// not relevant for Huobi because Huobi WebSocket API uses single channel
/// for multiple symbols
pub enum ConnectionKind {
    Default,
    MultiStream,
    Custom(String),
}

#[derive(Error, Debug)]
pub enum WebSocketError {
    #[error("WebSocket connection was closed by the server")]
    Disconnected,

    #[error("Error when creating socket connection to the server: {0}")]
    SocketError(String),

    #[error("Error when writing to created socket connection: {0}")]
    WriteError(String),

    #[error("Error when reading from the socket connection: {0}")]
    ReadError(String),
}

pub type Result<T> = std::result::Result<T, WebSocketError>;
