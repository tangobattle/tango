const DATA_DIR_NAME: &str = "Tango";

pub struct Paths {
    pub data: std::path::PathBuf,
}

impl Paths {
    pub fn system_default() -> anyhow::Result<Self> {
        let user_dirs = directories_next::UserDirs::new()
            .ok_or_else(|| anyhow::anyhow!("could not get user directories"))?;
        let data = user_dirs
            .document_dir()
            .ok_or_else(|| anyhow::anyhow!("could not get document directory"))?
            .join(DATA_DIR_NAME);
        Ok(Self { data })
    }

    pub fn roms(&self) -> std::path::PathBuf {
        self.data.join("roms")
    }

    pub fn saves(&self) -> std::path::PathBuf {
        self.data.join("saves")
    }

    pub fn patches(&self) -> std::path::PathBuf {
        self.data.join("patches")
    }

    pub fn replays(&self) -> std::path::PathBuf {
        self.data.join("replays")
    }
}
