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

pub const DEFAULT_MATCHMAKING_ENDPOINT: &str = "wss://matchmaking.tango.n1gp.net";
pub const DEFAULT_PATCH_REPO: &str = "https://patches.tango.n1gp.net";

/// Whether matchmaking connections may/must go through the TURN
/// relay (ported from tango's config.rs). `Auto` lets ICE pick the
/// best route (direct when possible, relay as fallback); `Always`
/// forces every candidate through the relay (`ice_transport_policy =
/// Relay`); `Never` strips the TURN servers from the ICE config
/// entirely, so only direct routes are attempted.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, serde::Serialize, serde::Deserialize)]
pub enum RelayMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl RelayMode {
    /// The `use_relay` argument `tango_signaling::connect` expects.
    pub fn use_relay(self) -> Option<bool> {
        match self {
            RelayMode::Auto => None,
            RelayMode::Always => Some(true),
            RelayMode::Never => Some(false),
        }
    }
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
    /// Hide identifying save details for streaming: masks the link-code
    /// input and swaps the save viewer's data tabs behind a Cover tab
    /// (tango's `streamer_mode`).
    pub streamer_mode: bool,
    /// Open the opponent's setup drawer automatically at match start
    /// (tango's `show_opponent_setup`). Stored now; consumed when the
    /// in-session setup drawers land.
    pub show_opponent_setup: bool,
    /// Start (and keep) the window fullscreen.
    pub full_screen: bool,
    /// PvP presentation delay in frames: how far the display core trails
    /// the netcode frontier. Purely local (never negotiated with the
    /// peer); clamped to tango-pvp's supported [2, 10] range at use.
    pub frame_delay: u32,
    /// Skip the game's battle BGM during PvP matches. Local-only, like
    /// the volume — the peer is unaffected and the recorded replay keeps
    /// its music.
    pub disable_bgm_in_pvp: bool,
    /// Matchmaking (signaling) server websocket endpoint.
    pub matchmaking_endpoint: String,
    /// TURN relay policy for matchmaking connections. Sampled at
    /// connect time (see [`RelayMode::use_relay`]).
    pub relay_mode: RelayMode,
    /// Patch repository synced by the Patches tab's Update button and
    /// the background autoupdater. Empty = the default repo (see
    /// [`Config::patch_repo_url`]).
    pub patch_repo: String,
    /// Re-sync the patch repo in the background every 15 minutes
    /// (tango's `enable_patch_autoupdate`).
    pub enable_patch_autoupdate: bool,
    /// Keyboard + gamepad bindings for the emulator sessions, edited
    /// by the Input settings pane. Not seeded from tango's config —
    /// tango stores physical scancodes, which don't map onto the
    /// logical key names tango-ng's Slint frontend sees.
    pub input_mapping: crate::input::Mapping,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_path: directories_next::UserDirs::new()
                .and_then(|d| d.document_dir().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("Tango"),
            language: crate::i18n::FALLBACK_LANG,
            nickname: None,
            theme: ThemeMode::default(),
            volume: 1.0,
            fractional_scaling: false,
            streamer_mode: false,
            show_opponent_setup: false,
            full_screen: false,
            frame_delay: 2,
            disable_bgm_in_pvp: false,
            matchmaking_endpoint: DEFAULT_MATCHMAKING_ENDPOINT.to_string(),
            relay_mode: RelayMode::default(),
            patch_repo: DEFAULT_PATCH_REPO.to_string(),
            enable_patch_autoupdate: true,
            input_mapping: crate::input::Mapping::default(),
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

/// The per-user config directory (ProjectDirs net/n1gp/tango) — shared
/// with tango, so the two frontends read the same on-disk identity
/// (see [`crate::identity`]).
pub fn config_dir() -> Option<std::path::PathBuf> {
    directories_next::ProjectDirs::from("net", "n1gp", "tango").map(|d| d.config_dir().to_path_buf())
}

fn config_path() -> Option<std::path::PathBuf> {
    config_dir().map(|d| d.join("tango-ng.json"))
}

impl Config {
    pub fn load() -> Self {
        let mut config = Self::load_from_disk();
        // Test-only override for verification drivers (`--ui-shot` in a
        // given language): forces the UI language for this process
        // without touching the config file. Note anything that later
        // calls `save()` will persist it — don't set it interactively.
        if let Ok(lang) = std::env::var("TANGO_NG_LANG") {
            match lang.parse() {
                Ok(lang) => config.language = lang,
                Err(e) => log::warn!("TANGO_NG_LANG={lang:?}: {e:?}, ignoring"),
            }
        }
        config
    }

    fn load_from_disk() -> Self {
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
        let Some(dir) = config_dir() else {
            return config;
        };
        let path = dir.join("config.json");
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
        if let Some(streamer) = v.get("streamer_mode").and_then(|x| x.as_bool()) {
            config.streamer_mode = streamer;
        }
        if let Some(show) = v.get("show_opponent_setup").and_then(|x| x.as_bool()) {
            config.show_opponent_setup = show;
        }
        if let Some(fs) = v.get("full_screen").and_then(|x| x.as_bool()) {
            config.full_screen = fs;
        }
        if let Some(endpoint) = v.get("matchmaking_endpoint").and_then(|x| x.as_str()) {
            if !endpoint.is_empty() {
                config.matchmaking_endpoint = endpoint.to_string();
            }
        }
        if let Some(repo) = v.get("patch_repo").and_then(|x| x.as_str()) {
            if !repo.is_empty() {
                config.patch_repo = repo.to_string();
            }
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

    /// The effective patch repo URL — the configured one, or the
    /// default when the config holds an empty string (tango's
    /// Autoupdater treats empty the same way).
    pub fn patch_repo_url(&self) -> String {
        if self.patch_repo.is_empty() {
            DEFAULT_PATCH_REPO.to_string()
        } else {
            self.patch_repo.clone()
        }
    }
}
