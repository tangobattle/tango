use tokio::io::AsyncWriteExt;

use crate::game;

#[derive(Debug, serde::Serialize, serde::Deserialize, typescript_type_def::TypeDef)]
pub struct Args {
    pub window_title: String,
    pub rom_path: String,
    pub save_path: String,
    pub keymapping: Keymapping,
    pub match_settings: Option<MatchSettings>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, typescript_type_def::TypeDef)]
pub struct MatchSettings {
    pub shadow_save_path: String,
    pub shadow_rom_path: String,
    pub session_id: String,
    pub input_delay: u32,
    pub match_type: u16,
    pub replays_path: String,
    pub replay_metadata: String,
    pub signaling_connect_addr: String,
    pub ice_servers: Vec<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, typescript_type_def::TypeDef)]
pub struct Keymapping {
    up: String,
    down: String,
    left: String,
    right: String,
    a: String,
    b: String,
    l: String,
    r: String,
    select: String,
    start: String,
}

impl TryInto<game::Keymapping> for Keymapping {
    type Error = serde_plain::Error;

    fn try_into(self) -> Result<game::Keymapping, Self::Error> {
        Ok(game::Keymapping {
            up: serde_plain::from_str(&self.up)?,
            down: serde_plain::from_str(&self.down)?,
            left: serde_plain::from_str(&self.left)?,
            right: serde_plain::from_str(&self.right)?,
            a: serde_plain::from_str(&self.a)?,
            b: serde_plain::from_str(&self.b)?,
            l: serde_plain::from_str(&self.l)?,
            r: serde_plain::from_str(&self.r)?,
            select: serde_plain::from_str(&self.select)?,
            start: serde_plain::from_str(&self.start)?,
        })
    }
}

impl Args {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, typescript_type_def::TypeDef)]
pub enum Notification {
    State(State),
}

#[derive(Debug, serde::Serialize, serde::Deserialize, typescript_type_def::TypeDef)]
pub enum State {
    Running,
    Waiting,
    Connecting,
}

#[derive(Clone)]
pub struct Client {
    writer:
        std::sync::Arc<tokio::sync::Mutex<std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>>>,
}

impl Client {
    pub fn new_from_stdout() -> Self {
        Client {
            writer: std::sync::Arc::new(tokio::sync::Mutex::new(Box::pin(tokio::io::stdout()))),
        }
    }

    pub async fn send_notification(&self, n: Notification) -> std::io::Result<()> {
        let mut writer = self.writer.lock().await;
        writer.write_all(&serde_json::to_vec(&n)?).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        Ok(())
    }
}
