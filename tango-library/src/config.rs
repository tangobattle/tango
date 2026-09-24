//! The persisted settings every host shares: the model, the derived
//! content paths, and its load/save. Host-only preferences are the
//! host's, `flatten`ed around this struct in the same file.
//!
//! *Where* it is stored is the frontend's business — natively that is
//! the platform config dir, in a browser an OPFS key — so the paths come
//! in as arguments rather than being resolved here.

use crate::storage::{self, Storage};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MATCHMAKING_ENDPOINT: &str = "wss://matchmaking.tango.n1gp.net";
pub const DEFAULT_PATCH_REPO: &str = "https://patches.tango.n1gp.net";

fn default_matchmaking_endpoint() -> String {
    DEFAULT_MATCHMAKING_ENDPOINT.to_string()
}

fn default_patch_repo() -> String {
    DEFAULT_PATCH_REPO.to_string()
}

/// The settings every host shares: where the library lives, which
/// services it talks to, and what the user last picked in it. A host's
/// own preferences (window, theme, audio…) live in a host-side struct
/// that `flatten`s this one into the same JSON object.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub data_path: std::path::PathBuf,
    pub matchmaking_endpoint: String,
    pub patch_repo: String,

    pub last_game: Option<(String, u8)>,
    /// Last selected game *family* (region-specific gamedb family string,
    /// e.g. `"bn3"`). The family drives the picker; the concrete game is
    /// re-derived from the chosen save. Persisted separately from
    /// `last_game` so a family selected with no owned ROM still restores.
    #[serde(default)]
    pub last_family: Option<String>,
    /// Per-family memory of the save each family was last used with.
    /// Key: the gamedb family string, as
    /// [`last_family`](Self::last_family) holds; value: the save's
    /// data-relative path. Written on every save pick, read to restore
    /// the selection at startup and when the user switches back to a
    /// family.
    ///
    /// Keyed by family, not by game, because the family is what the
    /// picker offers — the concrete game is re-derived from whichever
    /// save is chosen, so remembering one save per family is
    /// remembering the version too.
    #[serde(default)]
    pub last_save_per_family: std::collections::BTreeMap<String, String>,
    /// Per-save memory of the patch each save was last used with — the
    /// patch is an *overlay* on a loadout (game + save), dynamically
    /// selectable and remembered per save. Key: the save's data-relative
    /// path (same convention as `last_save_per_family` values). Value:
    /// `Some((patch_name, version))`, or `None` for "this save was last
    /// used unpatched" — distinct from a missing entry (save never
    /// selected), which lets the current patch carry over to brand-new
    /// saves. Saves created from a patch's template are seeded with that
    /// patch, encoding the intrinsic save↔patch association where one
    /// exists.
    #[serde(default)]
    pub last_patch_per_save: std::collections::BTreeMap<String, Option<(String, semver::Version)>>,
    /// Per-family memory of the link-battle mode last picked. Key: the
    /// gamedb family string (`"bn6"`), the same thing
    /// [`last_family`](Self::last_family) holds; value: `(mode,
    /// subtype)` in the encoding of the game's own `match_types` table.
    /// Written whenever the user picks one, read when the lobby's game
    /// changes — so coming back to a family offers the mode it was last
    /// played in rather than the built-in default.
    ///
    /// Keyed by family rather than by game, because the two versions of
    /// a family (Gregar and Falzar, say) are the same game to a player
    /// choosing between Single and Triple. An entry the game no longer
    /// admits — a patch shrank its table — is ignored, not repaired.
    #[serde(default)]
    pub last_match_type_per_family: std::collections::BTreeMap<String, (u8, u8)>,
    /// Names of patches the user has favorited — they sort to the top
    /// of pickers and get a star glyph next to their label.
    #[serde(default)]
    pub favorite_patches: std::collections::BTreeSet<String>,
    /// Where recomputable derived data goes, when the frontend has a
    /// platform cache directory to point at. Not persisted — it is a
    /// property of the host, not of the user's settings — so
    /// [`Config::cache_path`] falls back under `data_path` without it.
    #[serde(skip)]
    pub cache_dir: Option<std::path::PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        // A relative fallback keeps `Default` usable with no host
        // lookup; the frontend passes the real root to
        // `with_data_path` / `load_or_create`.
        let data_path = std::path::PathBuf::from("./tango-data");
        Self {
            data_path,
            matchmaking_endpoint: default_matchmaking_endpoint(),
            patch_repo: default_patch_repo(),
            last_game: None,
            last_family: None,
            last_save_per_family: std::collections::BTreeMap::new(),
            last_patch_per_save: std::collections::BTreeMap::new(),
            last_match_type_per_family: std::collections::BTreeMap::new(),
            favorite_patches: std::collections::BTreeSet::new(),
            cache_dir: None,
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
    /// The configured patch repo, or the default when the setting is
    /// blank (which is how the settings field spells "use the default").
    pub fn patch_repo_url(&self) -> String {
        if self.patch_repo.is_empty() {
            DEFAULT_PATCH_REPO.to_string()
        } else {
            self.patch_repo.clone()
        }
    }
    pub fn patches_path(&self) -> std::path::PathBuf {
        self.data_path.join("patches")
    }
    pub fn replays_path(&self) -> std::path::PathBuf {
        self.data_path.join("replays")
    }
    /// Where derived data the app can always recompute (replay match
    /// stats, …) lives — safe to delete wholesale. The frontend sets
    /// [`Self::cache_dir`] to the platform cache directory when it has
    /// one; otherwise this falls back under the data path.
    pub fn cache_path(&self) -> std::path::PathBuf {
        self.cache_dir.clone().unwrap_or_else(|| self.data_path.join("cache"))
    }

    /// Convert an absolute path under `data_path` to the
    /// forward-slash-separated relative string used as a value in
    /// `last_save_per_family` and keyed by in `last_patch_per_save`.
    /// Returns `None` if the path is
    /// outside `data_path` (shouldn't normally happen since saves
    /// live under `saves_path()`).
    pub fn data_relative_string(&self, path: &std::path::Path) -> Option<String> {
        let rel = path.strip_prefix(&self.data_path).ok()?;
        Some(
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    /// Inverse of `data_relative_string`. Joins a forward-slash
    /// relative path onto `data_path` and returns an absolute
    /// `PathBuf` using the local OS separator.
    pub fn data_relative_to_absolute(&self, rel: &str) -> std::path::PathBuf {
        let mut p = self.data_path.clone();
        for seg in rel.split('/') {
            if !seg.is_empty() {
                p.push(seg);
            }
        }
        p
    }

    /// Defaults rooted at `data_path`. Resolving that root is the
    /// frontend's job — the platform documents directory natively, an
    /// OPFS root in a browser.
    pub fn with_data_path(data_path: std::path::PathBuf) -> Self {
        Self {
            data_path,
            ..Default::default()
        }
    }

    /// Read the config at `path`, falling back to defaults (and
    /// creating the file) when it isn't there.
    pub fn load_or_create(storage: &dyn Storage, path: &std::path::Path, data_path: &std::path::Path) -> Self {
        load_json_or_create(storage, path, || Self::with_data_path(data_path.to_path_buf()))
    }

    pub fn save(&self, storage: &dyn Storage, path: &std::path::Path) -> std::io::Result<()> {
        save_json(storage, path, self)
    }
}

/// Read a JSON settings file at `path`, falling back to `defaults` (and
/// creating the file) when it isn't there. Generic so a frontend that
/// wraps [`Config`] in its own settings struct keeps the same recovery
/// rules.
pub fn load_json_or_create<T>(storage: &dyn Storage, path: &std::path::Path, defaults: impl Fn() -> T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    match storage::read_opt(storage, path) {
        Ok(Some(raw)) => match serde_json::from_slice::<T>(&raw) {
            Ok(c) => return c,
            Err(e) => {
                // Don't compound a parse failure by overwriting the user's
                // settings with defaults — park the unparseable file next
                // door so it can be recovered or reported.
                let backup = path.with_extension("json.bad");
                match storage.rename(path, &backup) {
                    Ok(()) => log::warn!("config parse failed ({e}); moved the bad file to {}", backup.display()),
                    Err(rename_err) => {
                        log::warn!(
                            "config parse failed ({e}) and backing the file up failed too ({rename_err}); \
                             using defaults without persisting"
                        );
                        return defaults();
                    }
                }
            }
        },
        Ok(None) => {}
        Err(e) => {
            // The file exists but couldn't be read (permissions, invalid
            // UTF-8, transient I/O) — it may be perfectly good on the next
            // launch, so don't overwrite it with defaults.
            log::warn!("config read failed, using defaults without persisting: {e}");
            return defaults();
        }
    }
    let c = defaults();
    let _ = save_json(storage, path, &c);
    c
}

/// Write-then-rename, so an interrupted save can't leave a truncated
/// settings file behind.
pub fn save_json<T: serde::Serialize>(storage: &dyn Storage, path: &std::path::Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        storage.create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(value).map_err(|e| std::io::Error::other(format!("serialize failed: {e}")))?;
    storage::write_atomic(storage, path, s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config written back when this struct also held the desktop's
    /// preferences still loads: keys another host owns are skipped, not
    /// rejected, so the browser's `/config.json` from before the split
    /// keeps its library half.
    #[test]
    fn a_config_with_host_keys_still_loads() {
        let config: Config = serde_json::from_value(serde_json::json!({
            "nickname": "someone",
            "theme": "Light",
            "frame_delay": 5,
            "data_path": "/tango",
            "patch_repo": "https://example.invalid/patches",
            "last_family": "bn6",
            "favorite_patches": ["exe6_pvp"],
        }))
        .unwrap();

        assert_eq!(config.data_path, std::path::Path::new("/tango"));
        assert_eq!(config.patch_repo_url(), "https://example.invalid/patches");
        assert_eq!(config.last_family.as_deref(), Some("bn6"));
        assert!(config.favorite_patches.contains("exe6_pvp"));
        assert_eq!(config.matchmaking_endpoint, DEFAULT_MATCHMAKING_ENDPOINT);
    }
}
