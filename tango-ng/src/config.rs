//! tango-ng's own config, stored as `tango-ng.json` next to tango's
//! `config.json`. On first run it seeds the portable fields (data path,
//! language, nickname, theme, volume) from the existing tango config so
//! the app works with zero setup; after that the two files are
//! independent (tango-ng never writes tango's config).

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, serde::Serialize, serde::Deserialize)]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub data_path: std::path::PathBuf,
    #[serde(with = "lang_serde")]
    pub language: unic_langid::LanguageIdentifier,
    pub nickname: Option<String>,
    pub theme: ThemeMode,
    pub volume: f32,
    pub fractional_scaling: bool,
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
            theme: ThemeMode::default(),
            volume: 1.0,
            fractional_scaling: false,
        }
    }
}

mod lang_serde {
    use serde::Deserialize as _;

    pub fn serialize<S: serde::Serializer>(
        lang: &unic_langid::LanguageIdentifier,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.serialize_str(&lang.to_string())
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<unic_langid::LanguageIdentifier, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    directories_next::ProjectDirs::from("net", "n1gp", "tango").map(|d| d.config_dir().join("tango-ng.json"))
}

impl Config {
    pub fn load() -> Self {
        if let Some(path) = config_path() {
            match std::fs::read_to_string(&path) {
                Ok(raw) => match serde_json::from_str(&raw) {
                    Ok(config) => return config,
                    Err(e) => log::warn!("{}: {e}, reseeding from tango config", path.display()),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => log::warn!("{}: {e}", path.display()),
            }
        }
        let config = Self::seed_from_tango();
        config.save();
        config
    }

    /// Best-effort import of the portable fields from tango's config.json.
    fn seed_from_tango() -> Self {
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
        if let Some(theme) = v.get("theme").and_then(|x| x.as_str()) {
            config.theme = match theme {
                "Light" => ThemeMode::Light,
                _ => ThemeMode::Dark,
            };
        }
        if let Some(volume) = v.get("volume").and_then(|x| x.as_f64()) {
            config.volume = (volume as f32).clamp(0.0, 1.0);
        }
        log::info!("seeded tango-ng config from {}", path.display());
        config
    }

    /// Atomic write-then-rename save. Failures are logged, not fatal.
    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        let Ok(raw) = serde_json::to_string_pretty(self) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, raw).and_then(|()| std::fs::rename(&tmp, &path)) {
            log::warn!("{}: save failed: {e}", path.display());
        }
    }

    pub fn roms_path(&self) -> std::path::PathBuf {
        self.data_path.join("roms")
    }

    pub fn saves_path(&self) -> std::path::PathBuf {
        self.data_path.join("saves")
    }

    pub fn replays_path(&self) -> std::path::PathBuf {
        self.data_path.join("replays")
    }

    pub fn patches_path(&self) -> std::path::PathBuf {
        self.data_path.join("patches")
    }
}
