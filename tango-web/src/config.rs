//! App configuration, one small JSON blob in localStorage. The dirs the
//! desktop client exposes are gone — ROMs and saves live in OPFS (see
//! `storage`), which has no user-facing paths.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::platform::input::Mapping;

const KEY: &str = "tango-web.config";

/// The matchmaking server every build points at (the same one the
/// desktop client dials); override per page load with
/// `?matchmaking_endpoint=…` (there is no settings knob).
pub const DEFAULT_MATCHMAKING: &str = "wss://matchmaking.tango.n1gp.net";

/// The desktop's default patch repo; the host must allow cross-origin
/// GETs for the web client to sync it.
pub const DEFAULT_PATCH_REPO: &str = "https://patches.tango.n1gp.net";

/// The matchmaking endpoint for this page load: the URL override wins,
/// then the Settings → Netplay value, then the default.
pub fn matchmaking_endpoint() -> String {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok())
        .and_then(|p| p.get("matchmaking_endpoint"))
        .filter(|v| !v.is_empty())
        .or_else(|| Config::load().matchmaking_endpoint.filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| DEFAULT_MATCHMAKING.to_string())
}

/// The relay preference as `tango_signaling`'s `use_relay` argument.
pub fn use_relay_pref() -> Option<bool> {
    Config::load().use_relay.use_relay()
}

/// Dark or light chrome, the desktop's Theme picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Dark, Theme::Light];
}

/// The accent color, the desktop's MegaMan-cast picker. Selection gold
/// and success green stay constant regardless, like the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Accent {
    #[default]
    TangoGreen,
    MegaManBlue,
    ProtoManRed,
    RollPink,
    GutsManYellow,
    BassPurple,
}

impl Accent {
    pub const ALL: [Accent; 6] = [
        Accent::TangoGreen,
        Accent::MegaManBlue,
        Accent::ProtoManRed,
        Accent::RollPink,
        Accent::GutsManYellow,
        Accent::BassPurple,
    ];

    /// The per-theme accent color (`ui/theme.rs`'s dark/light values).
    pub fn rgb(self, theme: Theme) -> (u8, u8, u8) {
        match (self, theme) {
            (Accent::TangoGreen, _) => (0x4c, 0xaf, 0x50),
            (Accent::MegaManBlue, Theme::Dark) => (0x4d, 0xa6, 0xff),
            (Accent::MegaManBlue, Theme::Light) => (0x14, 0x5c, 0xc2),
            (Accent::ProtoManRed, Theme::Dark) => (0xef, 0x40, 0x56),
            (Accent::ProtoManRed, Theme::Light) => (0xb7, 0x1c, 0x30),
            (Accent::RollPink, Theme::Dark) => (0xff, 0x6e, 0xa8),
            (Accent::RollPink, Theme::Light) => (0xc2, 0x2f, 0x6d),
            (Accent::GutsManYellow, Theme::Dark) => (0xe6, 0xb4, 0x22),
            (Accent::GutsManYellow, Theme::Light) => (0x96, 0x71, 0x18),
            (Accent::BassPurple, Theme::Dark) => (0xae, 0x6f, 0xf5),
            (Accent::BassPurple, Theme::Light) => (0x6a, 0x35, 0xb5),
        }
    }
}

/// Whether to route the peer connection through a TURN relay — the
/// desktop's Settings → Netplay picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UseRelay {
    #[default]
    Auto,
    Always,
    Never,
}

impl UseRelay {
    pub const ALL: [UseRelay; 3] = [UseRelay::Auto, UseRelay::Always, UseRelay::Never];

    /// The `use_relay` argument the signaling connect expects.
    pub fn use_relay(self) -> Option<bool> {
        match self {
            UseRelay::Auto => None,
            UseRelay::Always => Some(true),
            UseRelay::Never => Some(false),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub nick: String,
    /// The UI language (BCP 47). `None` = follow the browser.
    pub language: Option<String>,
    /// The patch repo to sync (the desktop's by default).
    pub patch_repo: String,
    /// Each family's last-picked patch: family → (name, version).
    pub last_patches: HashMap<String, (String, String)>,
    /// How many ticks behind the input frontier to present (the input
    /// delay / rollback depth tradeoff), adjustable live in-session.
    pub present_delay: u32,
    /// Master volume, 0.0..=1.0.
    pub volume: f32,
    /// Snap the game image to integer multiples of 240x160.
    pub integer_scaling: bool,
    /// The last-picked game *family* (region-specific family string,
    /// e.g. "bn6"), restored on load — selection is per family like
    /// the desktop's loadout, not per individual game.
    pub last_game: Option<String>,
    /// Each family's last-picked save: family string → either a file
    /// name in the flat `saves/` directory or a `//fresh/<variant>`
    /// sentinel. No entry = the default fresh-save row.
    pub last_saves: HashMap<String, String>,
    /// Favorited patches (by name) — they sort to the top of the
    /// Patches tab, like the desktop's.
    pub patch_favorites: Vec<String>,
    /// Settings → Netplay's matchmaking endpoint; `None`/empty = the
    /// default. A `?matchmaking_endpoint=` URL override beats both.
    pub matchmaking_endpoint: Option<String>,
    /// Whether to force / forbid the TURN relay.
    pub use_relay: UseRelay,
    /// The accent color driving the chrome (CSS custom props).
    pub accent: Accent,
    /// Dark or light chrome.
    pub theme: Theme,
    /// Hide identifying info (masked link-code input; the save view
    /// leads with the Cover tab), the desktop's streamer mode.
    pub streamer_mode: bool,
    /// Silence the game BGM during netplay matches.
    pub mute_bgm_in_pvp: bool,
    /// Show the recorded joypads during replay playback (the transport
    /// bar's toggle, persisted like the desktop's).
    pub show_replay_inputs: bool,
    /// Re-sync the patch repo automatically in the background.
    pub enable_patch_autoupdate: bool,
    pub mapping: Mapping,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // Empty until the player names themselves.
            nick: String::new(),
            language: None,
            patch_repo: DEFAULT_PATCH_REPO.to_string(),
            last_patches: HashMap::new(),
            present_delay: 2,
            volume: 1.0,
            integer_scaling: true,
            last_game: None,
            last_saves: HashMap::new(),
            patch_favorites: Vec::new(),
            matchmaking_endpoint: None,
            use_relay: UseRelay::default(),
            accent: Accent::default(),
            theme: Theme::default(),
            streamer_mode: false,
            mute_bgm_in_pvp: false,
            show_replay_inputs: false,
            enable_patch_autoupdate: true,
            mapping: Mapping::default(),
        }
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

impl Config {
    pub fn load() -> Config {
        local_storage()
            .and_then(|s| s.get_item(KEY).ok()?)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(storage) = local_storage() else {
            return;
        };
        match serde_json::to_string(self) {
            Ok(json) => {
                if storage.set_item(KEY, &json).is_err() {
                    log::error!("failed to write config to localStorage");
                }
            }
            Err(e) => log::error!("failed to serialize config: {e}"),
        }
    }
}
