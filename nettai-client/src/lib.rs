pub mod protocol;

use base64::Engine;
use futures_util::{FutureExt, SinkExt, StreamExt};
use prost::Message;
use tungstenite::client::IntoClientRequest;

struct Sender(
    tokio::sync::Mutex<
        futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            tungstenite::Message,
        >,
    >,
);

impl Sender {
    async fn send(&self, message: tungstenite::Message) -> Result<(), tungstenite::Error> {
        self.0.lock().await.send(message).await
    }

    async fn send_binary(&self, buf: Vec<u8>) -> Result<(), tungstenite::Error> {
        self.0.lock().await.send(tungstenite::Message::Binary(buf)).await
    }

    async fn send_message(&self, msg: &impl prost::Message) -> Result<(), tungstenite::Error> {
        self.send_binary(msg.encode_to_vec()).await
    }
}

struct Receiver(
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    >,
);

impl Receiver {
    async fn recv(&mut self) -> Option<Result<tungstenite::Message, tungstenite::Error>> {
        self.0.next().await
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("tungstenite: {0}")]
    Tungstenite(#[from] tungstenite::Error),

    #[error("prost: {0}")]
    ProstDecode(#[from] prost::DecodeError),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("timeout: {0}")]
    Timeout(#[from] tokio::time::error::Elapsed),
}

#[derive(Default)]
struct Pending {
    offer_sdp: Option<String>,
    answer_sdp: Option<String>,
}

struct Session {
    user_id: Vec<u8>,
    ticket: Vec<u8>,
    tx: std::sync::Arc<Sender>,
    rx: tokio::sync::Mutex<Receiver>,
    requests: tokio::sync::Mutex<std::collections::HashMap<Vec<u8>, Pending>>,
}

impl Session {
    async fn new(addr: &str, ticket: Vec<u8>) -> Result<Self, Error> {
        let mut req = addr.into_client_request()?;
        req.headers_mut().append(
            "X-Nettai-Version",
            tungstenite::http::HeaderValue::from_str(&env!("CARGO_PKG_VERSION")).unwrap(),
        );
        if !ticket.is_empty() {
            req.headers_mut().append(
                "Authorization",
                tungstenite::http::HeaderValue::from_str(&format!(
                    "Nettai-Ticket {}",
                    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&ticket)
                ))
                .unwrap(),
            );
        }

        let (stream, _) = tokio_tungstenite::connect_async(req).await?;

        let (tx, rx) = stream.split();

        let tx = std::sync::Arc::new(Sender(tokio::sync::Mutex::new(tx)));
        let mut rx = Receiver(rx);

        // Receive the Hello message.
        let hello = match rx
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stream closed"))??
        {
            tungstenite::Message::Binary(msg) => protocol::Packet::decode(&mut bytes::Bytes::from(msg))?
                .which
                .and_then(|which| match which {
                    protocol::packet::Which::Hello(hello) => Some(hello),
                    _ => None,
                })
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "unexpected packet"))?,
            _ => {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unexpected packet").into());
            }
        };

        Ok(Self {
            user_id: hello.user_id,
            ticket: hello.ticket,
            tx,
            rx: tokio::sync::Mutex::new(rx),
            requests: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    async fn run_loop(&self) -> Result<(), Error> {
        let mut rx = self.rx.lock().await;
        loop {
            tokio::select! {
                msg = tokio::time::timeout(std::time::Duration::from_secs(60), rx.recv()) => {
                    let msg = if let Some(msg) = msg? {
                        msg
                    } else {
                        return Ok::<_, Error>(());
                    }?;

                    match msg {
                        tungstenite::Message::Binary(buf) => {
                            match protocol::Packet::decode(&mut bytes::Bytes::from(buf))?
                                .which
                                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "unexpected packet"))?
                            {
                                protocol::packet::Which::Offer(offer) => {
                                    let mut requests = self.requests.lock().await;
                                    let mut pending = requests.entry(offer.user_id.clone()).or_default();
                                    pending.offer_sdp = Some(offer.sdp);
                                }
                                protocol::packet::Which::Answer(answer) => {
                                    let mut requests = self.requests.lock().await;
                                    let mut pending = requests.entry(answer.user_id.clone()).or_default();
                                }
                                msg => {
                                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unexpected packet: {:?}", msg)).into());
                                }
                            }

                        },
                        tungstenite::Message::Ping(_) => {
                            // Note that upon receiving a ping message, tungstenite cues a pong reply automatically.
                            // When you call either read_message, write_message or write_pending next it will try to send that pong out if the underlying connection can take more data.
                            // This means you should not respond to ping frames manually.
                        },
                        _ => todo!(),
                    }
                }
            }
        }
    }
}

enum MaybeSession {
    Session(std::sync::Arc<Session>),
    AwaitingSession(std::sync::Arc<tokio::sync::Notify>),
}

impl MaybeSession {
    fn set(&mut self, session: std::sync::Arc<Session>) {
        let notify = if let MaybeSession::AwaitingSession(notify) = &self {
            Some(notify.clone())
        } else {
            None
        };
        *self = MaybeSession::Session(session.clone());
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
    }
}

pub struct Client {
    session: std::sync::Arc<tokio::sync::Mutex<MaybeSession>>,
    _drop_guard: tokio_util::sync::DropGuard,
}

impl Client {
    pub async fn new(addr: &str, mut ticket: Vec<u8>) -> Result<Self, Error> {
        let session = std::sync::Arc::new(tokio::sync::Mutex::new(MaybeSession::AwaitingSession(
            std::sync::Arc::new(tokio::sync::Notify::new()),
        )));
        let cancellation_token = tokio_util::sync::CancellationToken::new();

        tokio::spawn({
            let addr = addr.to_string();
            let session = session.clone();
            let cancellation_token = cancellation_token.clone();

            async move {
                loop {
                    tokio::select! {
                        r = async {
                            let sess = backoff::future::retry(backoff::ExponentialBackoff::default(), || async {
                                Ok(tokio::time::timeout(std::time::Duration::from_secs(60), Session::new(&addr, ticket.clone())).await?)
                            })
                            .await??;
                            ticket = sess.ticket.clone();
                            let sess = std::sync::Arc::new(sess);
                            session.lock().await.set(sess.clone());
                            sess.run_loop().await?;
                            Ok::<_, Error>(())
                        } => {
                            if let Err(e) = r {
                                // Log the error.
                                log::error!("error in client session: {}", e);
                            }
                        }

                        _ = cancellation_token.cancelled() => {
                            *session.lock().await = MaybeSession::AwaitingSession(std::sync::Arc::new(tokio::sync::Notify::new()));
                            return;
                        }
                    }
                    *session.lock().await =
                        MaybeSession::AwaitingSession(std::sync::Arc::new(tokio::sync::Notify::new()));
                }
            }
        });

        Ok(Self {
            session,
            _drop_guard: cancellation_token.drop_guard(),
        })
    }

    async fn wait_for_session(&self) -> std::sync::Arc<Session> {
        loop {
            let notify = {
                match &*self.session.lock().await {
                    MaybeSession::Session(session) => {
                        return session.clone();
                    }
                    MaybeSession::AwaitingSession(notify) => notify.clone(),
                }
            };
            notify.notified().await
        }
    }

    pub async fn user_id(&self) -> Vec<u8> {
        self.wait_for_session().await.user_id.clone()
    }

    pub async fn connect(&self, target_user_id: &[u8]) -> Connecting {
        // There are two states we can be in:
        // - There is no offer from the remote, in which case we must offer ourselves.
        // - There is an offer from the remote, in which case we must answer.
        //
        // However, in the first case, we may encounter glare: that is, the remote offered but at the time of our connection we did not get their offer.
        // In this case, the server will send us their offer and we must rollback to accept it and answer.
        let target_user_id = target_user_id.to_vec();
        Connecting {
            target_user_id,
            fut: Box::pin(async {
                //
                Ok(())
            }),
        }
    }
}

pub struct Connecting {
    target_user_id: Vec<u8>,
    fut: futures_util::future::BoxFuture<'static, Result<(), Error>>,
}

impl Connecting {
    pub fn target_user_id(&self) -> &[u8] {
        &self.target_user_id
    }
}

impl std::future::Future for Connecting {
    type Output = Result<(), Error>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        self.fut.poll_unpin(cx)
    }
}
