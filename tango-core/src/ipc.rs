#[derive(serde::Serialize, serde::Deserialize)]
pub struct Args {
    pub rom_path: String,
    pub save_path: String,
    pub session_id: String,
    pub input_delay: u32,
    pub match_type: u16,
    pub replay_prefix: String,
    pub matchmaking_connect_addr: String,
    pub ice_servers: Vec<String>,
    pub keymapping: crate::game::Keymapping,
}

impl Args {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum Notification {
    Running,
    Waiting,
    Connecting,
    Done,
}

#[derive(Clone)]
pub struct Client(std::sync::Arc<parking_lot::Mutex<Inner>>);

struct Inner {
    writer: Box<dyn std::io::Write>,
}

impl Client {
    pub fn new_from_stdout() -> Self {
        Client(std::sync::Arc::new(parking_lot::Mutex::new(Inner {
            writer: Box::new(std::io::stdout()),
        })))
    }

    pub fn send_notification(&self, n: Notification) -> std::io::Result<()> {
        let mut inner = self.0.lock();
        serde_json::to_writer(&mut inner.writer, &n)?;
        inner.writer.write_all(b"\n")?;
        inner.writer.flush()?;
        Ok(())
    }
}
