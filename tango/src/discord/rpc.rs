use byteorder::ByteOrder;
use bytes::{Buf, BufMut};
use num_traits::FromPrimitive;
use rand::RngCore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub mod activity;

trait AsyncReadWrite
where
    Self: tokio::io::AsyncRead + tokio::io::AsyncWrite,
{
}

impl<T> AsyncReadWrite for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite {}

#[cfg(unix)]
async fn open() -> std::io::Result<Box<dyn AsyncReadWrite + Send + std::marker::Unpin>> {
    let tmpdir = if let Some(tmpdir) = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .flat_map(|key| std::env::var_os(key))
        .next()
    {
        std::path::PathBuf::from(tmpdir)
    } else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no temp dir"));
    };

    for i in 0..10 {
        if let Ok(stream) = tokio::net::UnixStream::connect(&tmpdir.join(format!("discord-ipc-{}", i))).await {
            return Ok(Box::new(stream));
        }
    }

    return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "could not connect"));
}

#[cfg(windows)]
async fn open() -> std::io::Result<Box<dyn AsyncReadWrite + Send + std::marker::Unpin>> {
    for i in 0..10 {
        if let Ok(pipe) =
            tokio::net::windows::named_pipe::ClientOptions::new().open(format!(r"\\?\pipe\discord-ipc-{}", i))
        {
            return Ok(Box::new(pipe));
        }
    }
    return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "could not connect"));
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
enum Command {
    Dispatch,
    Subscribe,
    SetActivity,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum Event {
    Ready,
    ActivityJoin,
    Error,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct Payload {
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
    cmd: Command,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    evt: Option<Event>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Error {
    code: u32,
    message: String,
}

#[derive(PartialEq, Eq, num_derive::FromPrimitive, Debug)]
enum Opcode {
    Handshake = 0,
    Frame = 1,
    Close = 2,
    Ping = 4,
    Pong = 5,
}

struct Sender {
    buf: bytes::BytesMut,
    w: tokio::io::WriteHalf<Box<dyn AsyncReadWrite + Send + std::marker::Unpin>>,
}

impl Sender {
    fn new(w: tokio::io::WriteHalf<Box<dyn AsyncReadWrite + Send + std::marker::Unpin>>) -> Self {
        Self {
            buf: bytes::BytesMut::new(),
            w,
        }
    }

    async fn send_packet(&mut self, opcode: Opcode, body: &[u8]) -> std::io::Result<()> {
        // ON WINDOWS, A NAMED PIPE CAN BE OPENED IN MESSAGE MODE, DESPITE THE INTERFACE CLEARLY BEING A STREAM INTERFACE.
        // WE'LL TRY FLUSH THE BUFFER WE HAVE AND FUCKING HOPE THAT IT'S OK WHEN WE SEND THE NEXT ONE AND THAT IT'S ATOMIC.
        // I HATE WINDOWS
        self.w.write_all_buf(&mut self.buf).await?;

        self.buf.put_u32_le(opcode as u32);
        self.buf.put_u32_le(body.len() as u32);
        self.buf.put_slice(body);
        self.w.write_all_buf(&mut self.buf).await?;
        Ok(())
    }
}

struct Receiver {
    buf: bytes::BytesMut,
    r: tokio::io::ReadHalf<Box<dyn AsyncReadWrite + Send + std::marker::Unpin>>,
}

impl Receiver {
    fn new(r: tokio::io::ReadHalf<Box<dyn AsyncReadWrite + Send + std::marker::Unpin>>) -> Self {
        Self {
            buf: bytes::BytesMut::new(),
            r,
        }
    }

    async fn receive_packet(&mut self) -> std::io::Result<(u32, Vec<u8>)> {
        while self.buf.len() < 4 {
            self.r.read_buf(&mut self.buf).await?;
        }
        let opcode = byteorder::LittleEndian::read_u32(&self.buf[0..4]);
        self.buf.advance(4);

        while self.buf.len() < 4 {
            self.r.read_buf(&mut self.buf).await?;
        }
        let size = byteorder::LittleEndian::read_u32(&self.buf[0..4]) as usize;
        self.buf.advance(4);

        while self.buf.len() < size {
            self.r.read_buf(&mut self.buf).await?;
        }
        let raw = self.buf[..size].to_vec();
        self.buf.advance(size);

        Ok((opcode, raw))
    }
}

async fn connect(client_id: u64) -> std::io::Result<(Receiver, Sender)> {
    let (r, w) = tokio::io::split(open().await?);
    let mut receiver = Receiver::new(r);
    let mut sender = Sender::new(w);

    sender
        .send_packet(
            Opcode::Handshake,
            serde_json::json!({
                "v": 1,
                "client_id": format!("{}", client_id),
            })
            .to_string()
            .as_bytes(),
        )
        .await?;

    let (opcode, raw) = receiver.receive_packet().await?;
    if opcode == Opcode::Close as u32 {
        let error = serde_json::from_slice::<Error>(&raw)?;
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            format!("{}: {}", error.code, error.message),
        ));
    }

    Ok((receiver, sender))
}

struct Inner {
    current_request: Option<(String, tokio::sync::oneshot::Sender<Payload>)>,
    sender: Sender,
}

pub struct Client {
    inner: std::sync::Arc<tokio::sync::Mutex<Option<Inner>>>,
}

fn generate_nonce() -> String {
    let mut rng = rand::thread_rng();
    let mut nonce = [0u8; 16];
    rng.fill_bytes(&mut nonce);
    serde_hex::SerHex::<serde_hex::Strict>::into_hex(&nonce).unwrap()
}

impl Client {
    pub async fn connect(client_id: u64) -> std::io::Result<Self> {
        let (mut receiver, sender) = connect(client_id).await?;

        let inner = std::sync::Arc::new(tokio::sync::Mutex::new(Some(Inner {
            current_request: None,
            sender,
        })));
        tokio::task::spawn({
            let inner = inner.clone();
            async move {
                let receiver = &mut receiver;
                let inner2 = inner.clone();
                if let Err(e) = (move || async move {
                    loop {
                        let (opcode, raw) = receiver.receive_packet().await?;
                        let opcode = Opcode::from_u32(opcode).ok_or_else(|| {
                            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid opcode: {}", opcode))
                        })?;
                        let mut inner = inner2.lock().await;
                        let inner = inner
                            .as_mut()
                            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"))?;
                        match opcode {
                            Opcode::Close => {
                                let error = serde_json::from_slice::<Error>(&raw)?;
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::ConnectionAborted,
                                    format!("{}: {}", error.code, error.message),
                                ));
                            }

                            Opcode::Frame => {
                                let mut payload = serde_json::from_slice::<Payload>(&raw)?;

                                if payload.cmd == Command::Dispatch {
                                    // This is an event that we've subscribed to.
                                    continue;
                                }

                                let incoming_nonce = if let Some(nonce) = payload.nonce.take() {
                                    nonce
                                } else {
                                    return Err::<(), std::io::Error>(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!("no nonce received"),
                                    ));
                                };

                                let (nonce, tx) = if let Some((nonce, tx)) = inner.current_request.take() {
                                    (nonce, tx)
                                } else {
                                    return Err::<(), std::io::Error>(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!("no current request"),
                                    ));
                                };
                                if incoming_nonce != nonce {
                                    return Err::<(), std::io::Error>(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!("unexpected nonce: {:?}", payload.nonce),
                                    ));
                                }

                                let _ = tx.send(payload);
                            }

                            Opcode::Ping => {
                                inner.sender.send_packet(Opcode::Pong, &raw).await?;
                                continue;
                            }

                            Opcode::Pong => {}

                            opcode => {
                                return Err::<(), std::io::Error>(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    format!("unexpected opcode: {:?}", opcode),
                                ));
                            }
                        }
                    }
                })()
                .await
                {
                    log::warn!("discord rpc disconnected with error: {:?}", e);
                }
                *inner.lock().await = None;
            }
        });
        Ok(Client { inner })
    }

    async fn do_request(&self, payload: &Payload) -> std::io::Result<Payload> {
        let nonce = if let Some(nonce) = payload.nonce.as_ref() {
            nonce
        } else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "expected nonce"));
        };
        let (rpc_tx, rpc_rx) = tokio::sync::oneshot::channel();
        {
            let mut inner = self.inner.lock().await;
            let inner = inner
                .as_mut()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"))?;
            if inner.current_request.is_some() {
                return Err(std::io::Error::new(std::io::ErrorKind::Other, "rpc in progress"));
            }
            inner.current_request = Some((nonce.clone(), rpc_tx));
            inner
                .sender
                .send_packet(Opcode::Frame, &serde_json::to_vec(payload)?)
                .await?;
        }
        let payload = rpc_rx
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, e))?;
        if payload.evt == Some(Event::Error) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                if let Some(data) = payload.data {
                    let error = serde_json::from_value::<Error>(data)?;
                    format!("{}: {}", error.code, error.message)
                } else {
                    "received error event with no details".to_string()
                },
            ));
        }
        Ok(payload)
    }

    pub async fn subscribe(&self, evt: Event) -> std::io::Result<()> {
        self.do_request(&Payload {
            nonce: Some(generate_nonce()),
            cmd: Command::Subscribe,
            args: None,
            evt: Some(evt),
            data: None,
        })
        .await?;
        Ok(())
    }

    pub async fn set_activity(&self, activity: &activity::Activity) -> std::io::Result<()> {
        self.do_request(&Payload {
            nonce: Some(generate_nonce()),
            cmd: Command::SetActivity,
            args: Some(serde_json::json!({
                "pid": std::process::id(),
                "activity": activity,
            })),
            evt: None,
            data: None,
        })
        .await?;
        Ok(())
    }
}
