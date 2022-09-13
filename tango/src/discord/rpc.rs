use byteorder::ByteOrder;
use bytes::{Buf, BufMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(unix)]
async fn open() -> std::io::Result<(
    Box<dyn tokio::io::AsyncRead + Send + std::marker::Unpin>,
    Box<dyn tokio::io::AsyncWrite + Send + std::marker::Unpin>,
)> {
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
        if let Ok(mut stream) = tokio::net::UnixStream::connect(&tmpdir.join(format!("discord-ipc-{}", i))).await {
            let (r, w) = stream.into_split();
            return Ok((Box::new(r), Box::new(w)));
        }
    }

    return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "could not connect"));
}

#[cfg(windows)]
fn open() -> std::io::Result<Box<dyn ReadWrite + Send>> {
    use std::os::windows::fs::OpenOptionsExt;
    (0..10)
        .flat_map(|i| {
            std::fs::OpenOptions::new()
                .access_mode(0x3)
                .open(format!(r"\\?\pipe\discord-ipc-{}", i))
                .ok()
        })
        .next()
        .map(|s| Box::new(s) as Box<dyn ReadWrite + Send>)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "could not connect"))
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
    w: Box<dyn tokio::io::AsyncWrite + Send + std::marker::Unpin>,
}

impl Sender {
    fn new(w: Box<dyn tokio::io::AsyncWrite + Send + std::marker::Unpin>) -> Self {
        Self {
            buf: bytes::BytesMut::new(),
            w,
        }
    }

    async fn send_packet(&mut self, opcode: Opcode, body: &[u8]) -> std::io::Result<()> {
        self.buf.put_u32_le(opcode as u32);
        self.buf.put_u32_le(body.len() as u32);
        self.buf.put_slice(body);
        self.w.write_all_buf(&mut self.buf).await?;
        Ok(())
    }
}

struct Receiver {
    buf: bytes::BytesMut,
    r: Box<dyn tokio::io::AsyncRead + Send + std::marker::Unpin>,
}

impl Receiver {
    fn new(r: Box<dyn tokio::io::AsyncRead + Send + std::marker::Unpin>) -> Self {
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
    let (r, w) = open().await?;
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

pub struct Client {}

impl Client {
    pub async fn connect(client_id: u64) -> std::io::Result<Self> {
        let (mut receiver, sender) = connect(client_id).await?;
        Ok(Client {})
    }
}
