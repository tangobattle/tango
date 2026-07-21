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

/// The matchmaking endpoint for this page load.
#[allow(dead_code)] // netplay (M3)
pub fn matchmaking_endpoint() -> String {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok())
        .and_then(|p| p.get("matchmaking_endpoint"))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_MATCHMAKING.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub nick: String,
    /// The UI language (BCP 47). `None` = follow the browser.
    pub language: Option<String>,
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
    pub mapping: Mapping,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // Empty until the player names themselves.
            nick: String::new(),
            language: None,
            present_delay: 2,
            volume: 1.0,
            integer_scaling: true,
            last_game: None,
            last_saves: HashMap::new(),
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
