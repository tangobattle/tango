//! Minimal read-side config. For now tango-ng reads the existing Tango
//! `config.json` (read-only, best-effort) to inherit `data_path` and
//! `language`, so a user's ROMs and saves show up with zero setup.
//! tango-ng grows its own config file when the settings pane lands.

pub struct Config {
    pub data_path: std::path::PathBuf,
    pub language: unic_langid::LanguageIdentifier,
    pub nickname: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_path: directories_next::UserDirs::new()
                .and_then(|d| d.document_dir().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("Tango"),
            language: crate::game::FALLBACK_LANG,
            nickname: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let mut config = Self::default();
        let Some(project_dirs) = directories_next::ProjectDirs::from("net", "n1gp", "tango") else {
            return config;
        };
        let path = project_dirs.config_dir().join("config.json");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return config;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            log::warn!("{}: unparseable, using defaults", path.display());
            return config;
        };
        if let Some(data_path) = v.get("data_path").and_then(|x| x.as_str()) {
            if !data_path.is_empty() {
                config.data_path = data_path.into();
            }
        }
        if let Some(lang) = v.get("language").and_then(|x| x.as_str()) {
            if let Ok(lang) = lang.parse() {
                config.language = lang;
            }
        }
        if let Some(nickname) = v.get("nickname").and_then(|x| x.as_str()) {
            if !nickname.is_empty() {
                config.nickname = Some(nickname.to_string());
            }
        }
        config
    }

    pub fn roms_path(&self) -> std::path::PathBuf {
        self.data_path.join("roms")
    }

    pub fn saves_path(&self) -> std::path::PathBuf {
        self.data_path.join("saves")
    }
}
