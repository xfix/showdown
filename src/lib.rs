#![deny(clippy::future_not_send)]

//! Pokémon Showdown client.
//!
//! # Stability
//!
//! This crate is not stable, not even close. The APIs of this crate are
//! heavily experimented on, and there isn't going to be depreciation period for
//! removed features. Don't use this crate if you aren't prepared for constant
//! breakage.

pub mod message;

use self::message::Message;
#[cfg(feature = "__tls")]
use futures_util::future::TryFutureExt;
use futures_util::ready;
use futures_util::sink::Sink;
use futures_util::stream::Stream as FuturesStream;
#[cfg(feature = "__tls")]
use serde::Deserialize;
use std::fmt::{self, Display, Formatter};
use std::pin::Pin;
use std::result::Result as StdResult;
use std::task::{Context, Poll};
use thiserror::Error;
#[cfg(feature = "time")]
pub use time;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::{Error as WsError, Message as OwnedMessage};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
pub use url;
use url::Url;

type SocketStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Message stream.
///
/// # Examples
///
#[cfg_attr(feature = "__tls", doc = "```no_run")]
#[cfg_attr(not(feature = "__tls"), doc = "```compile_fail")]
/// use futures::{SinkExt, StreamExt};
/// use showdown::message::{Kind, UpdateUser};
/// use showdown::{Result, RoomId, Stream};
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     let mut stream = Stream::connect("showdown").await?;
///     let message = stream.next().await.unwrap()?;
///     match message.kind() {
///         Kind::UpdateUser(UpdateUser {
///             username,
///             named: false,
///             ..
///         }) => {
///             assert!(username.starts_with(" Guest "));
///         }
///         _ => panic!(),
///     }
///     Ok(())
/// }
/// ```
pub struct Stream {
    stream: SocketStream,
}

impl Stream {
    /// Connects to a named Showdown server.
    ///
    /// Returns a structure implementing [`Sink`](Sink) and
    /// [`Stream`](FuturesStream) traits which can be used to send and
    /// receives messages respectively.
    ///
    /// It's possible to use [`StreamExt::split`](futures_util::stream::StreamExt::split)
    /// to split returned structure into separate `Sink` and `Stream` objects.
    ///
    /// Requires `native-tls`, `native-tls-vendored` or `rustls-tls` feature.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use showdown::{Result, Stream};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     assert!(Stream::connect("showdown").await.is_ok());
    ///     assert!(Stream::connect("fakestofservers").await.is_err());
    /// }
    /// ```
    #[cfg(feature = "__tls")]
    pub async fn connect(name: &str) -> Result<Self> {
        Self::connect_to_url(&fetch_server_url(name).await?).await
    }

    /// Connects to an URL.
    ///
    /// This URL is provided by
    #[cfg_attr(feature = "__tls", doc = " [`fetch_server_url`]")]
    #[cfg_attr(not(feature = "__tls"), doc = "`fetch_server_url`")]
    /// function.
    ///
    /// # Examples
    ///
    #[cfg_attr(feature = "__tls", doc = "```no_run")]
    #[cfg_attr(not(feature = "__tls"), doc = "```compile_fail")]
    /// use showdown::{fetch_server_url, Result, Stream};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<()> {
    ///     let url = fetch_server_url("smogtours").await?;
    ///     assert_eq!(url.as_str(), "ws://sim3.psim.us:8002/showdown/websocket");
    ///     Stream::connect_to_url(&url).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect_to_url(url: &Url) -> Result<Self> {
        let stream = Error::from_ws(tokio_tungstenite::connect_async(url).await)?.0;
        Ok(Self { stream })
    }
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream").finish()
    }
}

impl Sink<SendMessage> for Stream {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        Pin::new(&mut self.stream)
            .poll_ready(cx)
            .map(Error::from_ws)
    }

    fn start_send(mut self: Pin<&mut Self>, item: SendMessage) -> Result<()> {
        Error::from_ws(Pin::new(&mut self.stream).start_send(OwnedMessage::Text(item.0)))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        Pin::new(&mut self.stream)
            .poll_flush(cx)
            .map(Error::from_ws)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        Pin::new(&mut self.stream)
            .poll_close(cx)
            .map(Error::from_ws)
    }
}

impl FuturesStream for Stream {
    type Item = Result<Message>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(loop {
            match Error::from_ws(ready!(Pin::new(&mut self.stream).poll_next(cx)).transpose())? {
                Some(OwnedMessage::Text(raw)) => break Some(Ok(Message { raw })),
                Some(OwnedMessage::Close(Some(CloseFrame {
                    code: CloseCode::Normal,
                    ..
                }))) => {}
                Some(message) => break Some(Err(Error(ErrorInner::UnrecognizedMessage(message)))),
                None => break None,
            }
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendMessage(String);

impl SendMessage {
    /// Creates a global command.
    ///
    /// # Example
    ///
    #[cfg_attr(feature = "__tls", doc = "```no_run")]
    #[cfg_attr(not(feature = "__tls"), doc = "```compile_fail")]
    /// use futures::{SinkExt, StreamExt};
    /// use showdown::message::{Kind, QueryResponse};
    /// use showdown::{Result, RoomId, SendMessage, Stream};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<()> {
    ///     let mut stream = Stream::connect("showdown").await?;
    ///     stream.send(SendMessage::global_command("cmd rooms")).await?;
    ///     while let Some(received) = stream.next().await {
    ///         if let Kind::QueryResponse(QueryResponse::Rooms(rooms)) = received?.kind() {
    ///             assert!(rooms
    ///                 .official
    ///                 .iter()
    ///                 .any(|room| room.title == "Tournaments"));
    ///             return Ok(());
    ///         }
    ///     }
    ///     panic!("Server didn't provide a list of rooms")
    /// }
    /// ```
    pub fn global_command(command: impl Display) -> Self {
        SendMessage(format!("|/{}", command))
    }

    /// Creates a chat room message.
    pub fn chat_message(room_id: RoomId<'_>, message: impl Display) -> Self {
        Self::prefixed(room_id, ' ', message)
    }

    /// Creates a command that executes in a chat room.
    ///
    /// # Examples
    ///
    #[cfg_attr(feature = "__tls", doc = "```no_run")]
    #[cfg_attr(not(feature = "__tls"), doc = "```compile_fail")]
    /// use futures::{SinkExt, StreamExt};
    /// use showdown::message::{Kind, QueryResponse};
    /// use showdown::{Result, RoomId, SendMessage, Stream};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<()> {
    ///     let mut stream = Stream::connect("showdown").await?;
    ///     stream.send(SendMessage::global_command("join lobby")).await?;
    ///     stream.send(SendMessage::chat_command(RoomId::LOBBY, "roomdesc")).await;
    ///     while let Some(message) = stream.next().await {
    ///         if let Kind::Html(html) = message?.kind() {
    ///             assert!(html.contains("Relax here amidst the chaos."));
    ///             return Ok(());
    ///         }
    ///     }
    ///     panic!("Server didn't provide a room description")
    /// }
    /// ```
    pub fn chat_command(room_id: RoomId<'_>, command: impl Display) -> Self {
        Self::prefixed(room_id, '/', command)
    }

    pub fn broadcast_command(room_id: RoomId<'_>, command: impl Display) -> Self {
        Self::prefixed(room_id, '!', command)
    }

    fn prefixed(room_id: RoomId<'_>, prefix: char, message: impl Display) -> Self {
        SendMessage(format!("{}|{}{}", room_id.0, prefix, message))
    }
}

/// Requires `native-tls`, `native-tls-vendored` or `rustls-tls` feature.
#[cfg(feature = "__tls")]
pub async fn fetch_server_url(name: &str) -> Result<Url> {
    let owned_url;
    let url = if name == "showdown" {
        "wss://sim3.psim.us/showdown/websocket"
    } else {
        let server_info_url = format!("https://pokemonshowdown.com/servers/{}.json", name);
        let Server { host, port } = reqwest::get(&server_info_url)
            .and_then(|r| r.json())
            .await
            .map_err(|e| Error(ErrorInner::Reqwest(e)))?;
        let protocol = if port == 443 { "wss" } else { "ws" };
        // Concatenation is fine, as it's also done by the official Showdown client
        owned_url = format!("{}://{}:{}/showdown/websocket", protocol, host, port);
        &owned_url
    };
    Url::parse(url).map_err(|e| Error(ErrorInner::Url(e)))
}

#[cfg(feature = "__tls")]
#[derive(Deserialize)]
struct Server {
    host: String,
    port: u16,
}

#[derive(Copy, Clone, Debug)]
pub struct RoomId<'a>(pub &'a str);

impl RoomId<'_> {
    pub const LOBBY: RoomId<'static> = RoomId("lobby");
}

pub type Result<T> = StdResult<T, Error>;

/// A specialized `Result` type for Showdown client operations.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct Error(ErrorInner);

impl Error {
    fn from_ws<T>(r: StdResult<T, tokio_tungstenite::tungstenite::Error>) -> Result<T> {
        r.map_err(|e| Error(ErrorInner::WebSocket(e)))
    }
}

#[derive(Debug, Error)]
enum ErrorInner {
    #[error("Websocket error")]
    WebSocket(#[source] WsError),
    #[cfg(feature = "__tls")]
    #[error("HTTPS request error")]
    Reqwest(#[source] reqwest::Error),
    #[cfg(feature = "__tls")]
    #[error("Couldn't get a valid server URL")]
    Url(#[source] url::ParseError),
    #[cfg(feature = "__tls")]
    #[error("Couldn't parse login assertion")]
    Json(#[source] serde_json::Error),
    #[error("Unrecognized message: {0:?}")]
    UnrecognizedMessage(OwnedMessage),
}
