use prost::Message;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone)]
pub struct Client {
    writer: std::sync::Arc<
        tokio::sync::Mutex<std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send + 'static>>>,
    >,
    reader: std::sync::Arc<
        tokio::sync::Mutex<std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send + 'static>>>,
    >,
}

impl Client {
    pub fn new_from_stdio() -> Self {
        Client {
            writer: std::sync::Arc::new(tokio::sync::Mutex::new(Box::pin(tokio::io::stdout()))),
            reader: std::sync::Arc::new(tokio::sync::Mutex::new(Box::pin(tokio::io::stdin()))),
        }
    }

    pub async fn send(&self, req: tango_protos::ipc::FromCoreMessage) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().await;
        let buf = req.encode_to_vec();
        writer.write_u32_le(buf.len() as u32).await?;
        writer.flush().await?;
        writer.write_all(&buf).await?;
        writer.flush().await?;
        Ok(())
    }

    pub async fn receive(&self) -> anyhow::Result<tango_protos::ipc::ToCoreMessage> {
        let mut reader = self.reader.lock().await;
        let size = reader.read_u32_le().await? as usize;
        let mut buf = vec![0u8; size];
        reader.read_exact(&mut buf).await?;
        let resp = tango_protos::ipc::ToCoreMessage::decode(bytes::Bytes::from(buf))?;
        Ok(resp)
    }
}
