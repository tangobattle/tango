use byteorder::ByteOrder;
use bytes::{Buf, BufMut};
use num_traits::FromPrimitive;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
enum Command {
    Dispatch,
    Subscribe,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SubscribeArgs {
    guild_id: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
enum Event {
    Ready,
    ActivityJoin,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Payload {
    nonce: String,
    cmd: Command,
    args: Option<serde_json::Value>,
    evt: Event,
    data: Option<serde_json::Value>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Close {
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
        let close = serde_json::from_slice::<Close>(&raw)?;
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            format!("{}: {}", close.code, close.message),
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
                                let close = serde_json::from_slice::<Close>(&raw)?;
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::ConnectionAborted,
                                    format!("{}: {}", close.code, close.message),
                                ));
                            }

                            Opcode::Frame => {
                                let payload = serde_json::from_slice::<Payload>(&raw)?;
                                let (nonce, tx) = if let Some((nonce, tx)) = inner.current_request.take() {
                                    (nonce, tx)
                                } else {
                                    return Err::<(), std::io::Error>(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!("no current request"),
                                    ));
                                };
                                if payload.nonce != nonce {
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
        let (rpc_tx, rpc_rx) = tokio::sync::oneshot::channel();
        {
            let mut inner = self.inner.lock().await;
            let inner = inner
                .as_mut()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "not connected"))?;
            if inner.current_request.is_some() {
                return Err(std::io::Error::new(std::io::ErrorKind::Other, "rpc in progress"));
            }
            inner.current_request = Some((payload.nonce.clone(), rpc_tx));
            inner
                .sender
                .send_packet(Opcode::Frame, &serde_json::to_vec(payload)?)
                .await?;
        }
        Ok(rpc_rx
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, e))?)
    }
}
