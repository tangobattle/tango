pub mod protocol;

use futures_util::{SinkExt, StreamExt};

struct Sender(
    tokio::sync::Mutex<
        futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            tokio_tungstenite::tungstenite::Message,
        >,
    >,
);

impl Sender {
    async fn send(
        &self,
        message: tokio_tungstenite::tungstenite::Message,
    ) -> Result<(), tokio_tungstenite::tungstenite::Error> {
        self.0.lock().await.send(message).await
    }

    async fn send_binary(&self, buf: Vec<u8>) -> Result<(), tokio_tungstenite::tungstenite::Error> {
        self.0
            .lock()
            .await
            .send(tokio_tungstenite::tungstenite::Message::Binary(buf))
            .await
    }

    async fn send_message(&self, msg: &impl prost::Message) -> Result<(), tokio_tungstenite::tungstenite::Error> {
        self.send_binary(msg.encode_to_vec()).await
    }
}

struct Receiver(
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    >,
);

impl Receiver {
    async fn recv(
        &mut self,
    ) -> Option<Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>> {
        self.0.next().await
    }
}

pub struct Client {}
