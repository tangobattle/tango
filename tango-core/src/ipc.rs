use crate::game;

#[derive(serde::Serialize, serde::Deserialize, typescript_type_def::TypeDef)]
pub struct Args {
    pub rom_path: String,
    pub save_path: String,
    pub session_id: String,
    pub input_delay: u32,
    pub match_type: u16,
    pub replay_prefix: String,
    pub matchmaking_connect_addr: String,
    pub ice_servers: Vec<String>,
    pub keymapping: Keymapping,
}

#[derive(serde::Serialize, serde::Deserialize, typescript_type_def::TypeDef)]
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

#[derive(serde::Serialize, serde::Deserialize, typescript_type_def::TypeDef)]
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
