use futures::{SinkExt, StreamExt};
use showdown::message::{Kind, QueryResponse, Room};
use showdown::{RoomId, SendMessage, Stream};
use std::borrow::Cow;
use std::error::Error;
use std::net::Ipv4Addr;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

async fn mock_connection() -> Result<(WebSocketStream<TcpStream>, Stream), Box<dyn Error>> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let port = listener.local_addr()?.port();
    let uri = format!("ws://127.0.0.1:{}", port).parse()?;
    let (socket, stream) = tokio::join!(
        async move { tokio_tungstenite::accept_async(listener.accept().await?.0).await },
        Stream::connect_to_url(&uri),
    );
    Ok((socket?, stream?))
}

#[tokio::test]
async fn parsing_chat_messages() -> Result<(), Box<dyn Error>> {
    let (mut socket, mut stream) = mock_connection().await?;
    socket
        .send(Message::Text("|c:|1634571729|+xfix|Hello|world".into()))
        .await?;
    let message = stream.next().await.unwrap()?;
    let chat = match message.kind() {
        Kind::Chat(chat) => chat,
        _ => unreachable!(),
    };
    assert_eq!(message.room().0, RoomId::LOBBY.0);
    #[cfg(feature = "time")]
    assert_eq!(
        chat.timestamp(),
        time::macros::datetime!(2021-10-18 15:42:09 UTC),
    );
    assert_eq!(chat.user(), "+xfix");
    assert_eq!(chat.message(), "Hello|world");
    Ok(())
}

#[tokio::test]
async fn reply_test() -> Result<(), Box<dyn Error>> {
    let (mut socket, mut stream) = mock_connection().await?;
    socket
        .send(Message::Text("|c:|0|+xfix|Hi there".into()))
        .await?;
    let message = stream.next().await.unwrap()?;
    assert!(matches!(message.kind(), Kind::Chat(_)));
    stream
        .send(SendMessage::chat_message(message.room(), "Hi there"))
        .await?;
    assert_eq!(
        socket.next().await.transpose()?,
        Some(Message::Text("lobby| Hi there".into())),
    );
    Ok(())
}

#[tokio::test]
async fn test_global_command() -> Result<(), Box<dyn Error>> {
    let (mut socket, mut stream) = mock_connection().await?;
    stream
        .send(SendMessage::global_command("hey there"))
        .await?;
    assert_eq!(
        socket.next().await.transpose()?,
        Some(Message::Text("|/hey there".into())),
    );
    Ok(())
}

#[tokio::test]
async fn parsing_roomlist() -> Result<(), Box<dyn Error>> {
    let (mut socket, mut stream) = mock_connection().await?;
    socket
        .send(Message::Text(
            r#"|queryresponse|rooms|{
                "official": [
                    {
                        "title": "a\"b",
                        "desc": "\n",
                        "userCount": 2
                    }
                ],
                "pspl": [],
                "chat": [
                    {
                        "title": "Nice room",
                        "desc": "No need to own that one",
                        "userCount": 1
                    }
                ],
                "userCount": 42,
                "battleCount": 24
            }"#
            .into(),
        ))
        .await?;
    match stream.next().await.unwrap()?.kind() {
        Kind::QueryResponse(QueryResponse::Rooms(rooms_list)) => {
            let mut iter = rooms_list.iter();
            match iter.next() {
                Some(Room {
                    title: Cow::Owned(title),
                    desc: Cow::Owned(desc),
                    ..
                }) => {
                    assert_eq!(title, "a\"b");
                    assert_eq!(desc, "\n");
                }
                _ => unreachable!(),
            }
            match iter.next() {
                Some(Room {
                    title: Cow::Borrowed("Nice room"),
                    desc: Cow::Borrowed("No need to own that one"),
                    ..
                }) => {}
                _ => unreachable!(),
            }
            assert!(iter.next().is_none());
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[tokio::test]
async fn parsing_close_message() -> Result<(), Box<dyn Error>> {
    let (mut socket, mut stream) = mock_connection().await?;
    socket
        .close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "Normal closure".into(),
        }))
        .await?;
    drop(socket);
    assert!(stream.next().await.is_none());
    Ok(())
}
