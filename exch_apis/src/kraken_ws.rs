/**
 * `kraken_ws.rs` file containts functions and structs useful
 * to connect to kraken Websocket API.
 **/

use std::{
    io::Read,
    net::TcpStream
};
use tungstenite::{
    connect, handshake::client::Response, protocol::WebSocket,
    stream::MaybeTlsStream, Message,
};

use crate::common::{
    KLine, BookTick, WebsocketEvent, WebSocketError, Result as WsResult
};

/// Kraken base URL for websocket API
const KRAKEN_WS_URL: &str = "wss://ws.kraken.com";

pub struct KrakenWebsocket<'a> {
    pub socket: Option<(WebSocket<MaybeTlsStream<TcpStream>>, Response)>,
    handler: Box<dyn FnMut(WebsocketEvent) -> WsResult<()> + 'a>,
}

