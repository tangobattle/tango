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

    pub fn report_running(&self) -> std::io::Result<()> {
        let mut inner = self.0.lock();
        inner.writer.write(b"running\n")?;
        inner.writer.flush()?;
        Ok(())
    }

    pub fn report_waiting(&self) -> std::io::Result<()> {
        let mut inner = self.0.lock();
        inner.writer.write(b"waiting\n")?;
        inner.writer.flush()?;
        Ok(())
    }

    pub fn report_connecting(&self) -> std::io::Result<()> {
        let mut inner = self.0.lock();
        inner.writer.write(b"connecting\n")?;
        inner.writer.flush()?;
        Ok(())
    }

    pub fn report_done(&self) -> std::io::Result<()> {
        let mut inner = self.0.lock();
        inner.writer.write(b"done\n")?;
        inner.writer.flush()?;
        Ok(())
    }
}

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
