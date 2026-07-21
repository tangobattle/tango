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

/// The accent color, the desktop's MegaMan-cast picker (dark-palette
/// values — the web build is dark-only). Selection gold and success
/// green stay constant regardless, like the desktop.
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

    /// The dark-palette accent color (`ui/theme.rs`'s values).
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Accent::TangoGreen => (0x4c, 0xaf, 0x50),
            Accent::MegaManBlue => (0x4d, 0xa6, 0xff),
            Accent::ProtoManRed => (0xef, 0x40, 0x56),
            Accent::RollPink => (0xff, 0x6e, 0xa8),
            Accent::GutsManYellow => (0xe6, 0xb4, 0x22),
            Accent::BassPurple => (0xae, 0x6f, 0xf5),
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
