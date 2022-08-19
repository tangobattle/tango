use byteorder::ByteOrder;
use bytes::{Buf, BufMut};
use prost::Message;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub mod protos {
    include!(concat!(env!("OUT_DIR"), "/tango.ipc.rs"));
}

pub struct Sender {
    writer: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send + 'static>>,
    buf: bytes::BytesMut,
}

impl Sender {
    pub fn new_from_stdout() -> Self {
        Sender {
            writer: Box::pin(tokio::io::stdout()),
            buf: bytes::BytesMut::new(),
        }
    }

    pub async fn send(&mut self, req: protos::FromCoreMessage) -> anyhow::Result<()> {
        let buf = req.encode_to_vec();
        self.buf.put_u32_le(buf.len() as u32);
        self.buf.put_slice(&buf[..]);
        self.writer.write_all_buf(&mut self.buf).await?;
        self.writer.flush().await?;
        Ok(())
    }
}

pub struct Receiver {
    reader: std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send + 'static>>,
    buf: bytes::BytesMut,
}

impl Receiver {
    pub fn new_from_stdin() -> Self {
        Receiver {
            reader: Box::pin(tokio::io::stdin()),
            buf: bytes::BytesMut::new(),
        }
    }

    pub async fn receive(&mut self) -> anyhow::Result<protos::ToCoreMessage> {
        while self.buf.len() < 4 {
            self.reader.read_buf(&mut self.buf).await?;
        }
        let size = byteorder::LittleEndian::read_u32(&self.buf[0..4]) as usize;

        while self.buf.len() < 4 + size {
            self.reader.read_buf(&mut self.buf).await?;
        }
        let resp = protos::ToCoreMessage::decode(&self.buf[4..4 + size])?;

        self.buf.advance(4 + size);

        Ok(resp)
    }
}
