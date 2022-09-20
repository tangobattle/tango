use std::io::{Read, Write};

use serde::Deserialize;

use crate::{i18n, input};

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub enum GraphicsBackend {
    #[cfg(feature = "glutin")]
    Glutin,
    #[cfg(feature = "wgpu")]
    Wgpu,
}

impl Default for GraphicsBackend {
    #[allow(unreachable_code)]
    fn default() -> Self {
        #[cfg(feature = "glutin")]
        return Self::Glutin;
        #[cfg(feature = "wgpu")]
        return Self::Wgpu;
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub enum AudioBackend {
    #[cfg(feature = "sdl2-audio")]
    Sdl2,
    #[cfg(feature = "cpal")]
    Cpal,
}

impl Default for AudioBackend {
    #[allow(unreachable_code)]
    fn default() -> Self {
        #[cfg(feature = "sdl2-audio")]
        return Self::Sdl2;
        #[cfg(feature = "cpal")]
        return Self::Cpal;
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub enum Theme {
    System,
    Light,
    Dark,
}

impl Default for Theme {
    fn default() -> Self {
        Self::System
    }
}

fn serialize_language_identifier<S>(v: &unic_langid::LanguageIdentifier, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&v.to_string())
}

fn deserialize_language_identifier<'de, D>(deserializer: D) -> Result<unic_langid::LanguageIdentifier, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let buf = String::deserialize(deserializer)?;
    buf.parse().map_err(serde::de::Error::custom)
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub nickname: Option<String>,
    pub theme: Theme,
    pub show_debug: bool,
    #[serde(
        serialize_with = "serialize_language_identifier",
        deserialize_with = "deserialize_language_identifier"
    )]
    pub language: unic_langid::LanguageIdentifier,
    pub max_queue_length: u32,
    pub video_filter: String,
    pub max_scale: u32,
    pub input_mapping: input::Mapping,
    pub matchmaking_endpoint: String,
    pub replaycollector_endpoint: String,
    pub patch_repo: String,
    pub enable_patch_autoupdate: bool,
    pub input_delay: u32,
    pub default_match_type: u8,
    pub data_path: std::path::PathBuf,
    pub full_screen: bool,
    pub streamer_mode: bool,
    pub show_own_setup: bool,
    pub graphics_backend: GraphicsBackend,
    pub audio_backend: AudioBackend,
    pub volume: i32,
    pub ui_scale_percent: u32,
    pub allow_prerelease_upgrades: bool,
    pub enable_updater: bool,
    pub integer_scaling: bool,
    pub always_show_status_bar: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            nickname: None,
            theme: Theme::System,
            show_debug: Default::default(),
            language: i18n::FALLBACK_LANG.parse().unwrap(),
            max_queue_length: 1200,
            video_filter: "".to_string(),
            max_scale: 0,
            input_mapping: Default::default(),
            matchmaking_endpoint: "".to_string(),
            replaycollector_endpoint: "https://replaycollector.tango.n1gp.net".to_string(),
            patch_repo: "".to_string(),
            enable_patch_autoupdate: true,
            input_delay: 2,
            default_match_type: 1,
            data_path: "".into(),
            full_screen: false,
            streamer_mode: false,
            show_own_setup: false,
            graphics_backend: Default::default(),
            audio_backend: Default::default(),
            volume: 0x100,
            ui_scale_percent: 100,
            allow_prerelease_upgrades: !env!("CARGO_PKG_VERSION")
                .parse::<semver::Version>()
                .unwrap()
                .pre
                .is_empty(),
            enable_updater: true,
            integer_scaling: false,
            always_show_status_bar: false,
        }
    }
}

fn get_project_dirs() -> Option<directories_next::ProjectDirs> {
    directories_next::ProjectDirs::from("net.n1gp", "", "Tango")
}

fn get_config_path() -> Result<std::path::PathBuf, anyhow::Error> {
    Ok(get_project_dirs()
        .ok_or_else(|| anyhow::anyhow!("could not get tango project directory"))?
        .config_dir()
        .join("config.json"))
}

pub fn get_updater_path() -> Result<std::path::PathBuf, anyhow::Error> {
    Ok(get_project_dirs()
        .ok_or_else(|| anyhow::anyhow!("could not get tango project directory"))?
        .cache_dir()
        .join("updater"))
}

const DATA_DIR_NAME: &str = "Tango";

impl Config {
    pub fn system_defaults() -> Result<Self, anyhow::Error> {
        let user_dirs =
            directories_next::UserDirs::new().ok_or_else(|| anyhow::anyhow!("could not get user directories"))?;

        let tango_data_dir = user_dirs
            .document_dir()
            .ok_or_else(|| anyhow::anyhow!("could not get tango data directory"))?
            .join(DATA_DIR_NAME);

        Ok(Self {
            language: sys_locale::get_locale()
                .unwrap_or(i18n::FALLBACK_LANG.to_string())
                .parse()?,
            data_path: tango_data_dir,
            ..Default::default()
        })
    }

    pub fn create() -> Result<Self, anyhow::Error> {
        let config_path = get_config_path()?;
        let config = Self::system_defaults()?;
        std::fs::create_dir_all(config_path.parent().unwrap())?;
        std::fs::write(&config_path, serde_json::to_string(&config)?)?;
        Ok(config)
    }

    pub fn load_or_create() -> Result<Self, anyhow::Error> {
        let config_path = get_config_path()?;
        match std::fs::File::open(&config_path) {
            Ok(mut file) => {
                let mut contents = String::new();
                file.read_to_string(&mut contents)?;
                match serde_json::from_str(&contents) {
                    Ok(config) => Ok(config),
                    Err(err) => {
                        log::error!("error loading config, creating new config: {}", err);
                        Self::create()
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Self::create(),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self) -> Result<(), anyhow::Error> {
        let contents = serde_json::to_string(self)?;
        let mut file = std::fs::File::create(get_config_path()?)?;
        file.write_all(contents.as_bytes())?;
        Ok(())
    }

    pub fn saves_path(&self) -> std::path::PathBuf {
        self.data_path.join("saves")
    }

    pub fn roms_path(&self) -> std::path::PathBuf {
        self.data_path.join("roms")
    }

    pub fn replays_path(&self) -> std::path::PathBuf {
        self.data_path.join("replays")
    }

    pub fn patches_path(&self) -> std::path::PathBuf {
        self.data_path.join("patches")
    }

    pub fn logs_path(&self) -> std::path::PathBuf {
        self.data_path.join("logs")
    }

    pub fn crashstates_path(&self) -> std::path::PathBuf {
        self.data_path.join("crashstates")
    }

    pub fn ensure_dirs(&self) -> Result<(), anyhow::Error> {
        std::fs::create_dir_all(&self.saves_path())?;
        std::fs::create_dir_all(&self.roms_path())?;
        std::fs::create_dir_all(&self.replays_path())?;
        std::fs::create_dir_all(&self.patches_path())?;
        std::fs::create_dir_all(&self.logs_path())?;
        std::fs::create_dir_all(&self.crashstates_path())?;
        Ok(())
    }
}

pub const DEFAULT_MATCHMAKING_ENDPOINT: &str = "wss://matchmaking.tango.n1gp.net";
pub const DEFAULT_PATCH_REPO: &str = "https://patches.tango.n1gp.net";
