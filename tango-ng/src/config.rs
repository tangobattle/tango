//! On-disk user config. Slim version of `tango/src/config.rs` — keeps
//! only the fields tango-ng actually uses (no graphics/audio backends,
//! no input mappings, etc.) and lives in its own ProjectDirs path so
//! it doesn't collide with the main app's config.

use serde::{Deserialize, Serialize};
use std::io::Write;

const DATA_DIR_NAME: &str = "Tango";

const QUALIFIER: &str = "net";
const ORGANIZATION: &str = "n1gp";
const APPLICATION: &str = "tango-ng";

pub const DEFAULT_MATCHMAKING_ENDPOINT: &str = "wss://matchmaking.tango.n1gp.net";
pub const DEFAULT_PATCH_REPO: &str = "https://github.com/tangobattle/patches";

fn default_matchmaking_endpoint() -> String {
    DEFAULT_MATCHMAKING_ENDPOINT.to_string()
}

fn default_patch_repo() -> String {
    DEFAULT_PATCH_REPO.to_string()
}

fn default_language() -> unic_langid::LanguageIdentifier {
    crate::i18n::FALLBACK_LANG
}

fn ser_language<S: serde::Serializer>(
    lang: &unic_langid::LanguageIdentifier,
    s: S,
) -> Result<S::Ok, S::Error> {
    s.serialize_str(&lang.to_string())
}

fn de_language<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<unic_langid::LanguageIdentifier, D::Error> {
    let s = String::deserialize(d)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
}

impl std::fmt::Display for ThemeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub nickname: Option<String>,
    #[serde(serialize_with = "ser_language", deserialize_with = "de_language")]
    pub language: unic_langid::LanguageIdentifier,
    pub streamer_mode: bool,
    pub theme: ThemeMode,
    pub data_path: std::path::PathBuf,
    pub matchmaking_endpoint: String,
    pub patch_repo: String,

    pub last_game: Option<(String, u8)>,
    pub last_save: Option<std::path::PathBuf>,
    pub last_patch: Option<String>,
    pub last_patch_version: Option<semver::Version>,
}

impl Default for Config {
    fn default() -> Self {
        // Fall back to ./tango-data if the user dirs lookup fails so the
        // app still runs in degraded form rather than panicking.
        let data_path = directories_next::UserDirs::new()
            .and_then(|u| u.document_dir().map(|d| d.join(DATA_DIR_NAME)))
            .unwrap_or_else(|| std::path::PathBuf::from("./tango-data"));
        Self {
            nickname: None,
            language: default_language(),
            streamer_mode: false,
            theme: ThemeMode::default(),
            data_path,
            matchmaking_endpoint: default_matchmaking_endpoint(),
            patch_repo: default_patch_repo(),
            last_game: None,
            last_save: None,
            last_patch: None,
            last_patch_version: None,
        }
    }
}

impl Config {
    pub fn roms_path(&self) -> std::path::PathBuf {
        self.data_path.join("roms")
    }
    pub fn saves_path(&self) -> std::path::PathBuf {
        self.data_path.join("saves")
    }
    pub fn patches_path(&self) -> std::path::PathBuf {
        self.data_path.join("patches")
    }
    pub fn replays_path(&self) -> std::path::PathBuf {
        self.data_path.join("replays")
    }

    pub fn load_or_create() -> Self {
        match config_path() {
            Some(p) => match std::fs::read_to_string(&p) {
                Ok(s) => match serde_json::from_str(&s) {
                    Ok(c) => return c,
                    Err(e) => log::warn!("config parse failed, using defaults: {e}"),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => log::warn!("config read failed, using defaults: {e}"),
            },
            None => log::warn!("could not resolve config dir, using defaults"),
        }
        let c = Self::default();
        let _ = c.save();
        c
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(p) = config_path() else {
            return Err(std::io::Error::other("no config dir"));
        };
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::other(format!("serialize failed: {e}")))?;
        let mut f = std::fs::File::create(&p)?;
        f.write_all(s.as_bytes())?;
        Ok(())
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    directories_next::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|d| d.config_dir().join("config.json"))
}
