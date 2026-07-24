//! This frontend's config: the library's settings model plus the bits
//! only this frontend has, and the platform locations it all lives in.
//!
//! [`tango_library::config::Config`] holds everything a second frontend
//! would also need (paths, nickname, netplay and patch settings…). The
//! input mapping can't join it: bindings are expressed in *this* UI
//! toolkit's physical key codes and SDL gamepad ids, which a browser
//! build would spell differently. So it lives here, `flatten`ed into the
//! same JSON object — the on-disk format is unchanged — and [`Config`]
//! derefs to the library's, so every `config.<library field>` access
//! reads exactly as before.

use serde::{Deserialize, Serialize};

pub use tango_library::config::{AccentColor, RelayMode, ThemeMode, DATA_DIR_NAME, FILE_NAME};

const QUALIFIER: &str = "net";
const ORGANIZATION: &str = "n1gp";
const APPLICATION: &str = "tango";

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Config {
    #[serde(flatten)]
    pub library: tango_library::config::Config,

    /// User-editable input bindings (keyboard + gamepad). See
    /// [`crate::platform::input::Mapping::default`] for the
    /// out-of-the-box layout. Each mgba key can have multiple bindings.
    pub input_mapping: crate::platform::input::Mapping,
}

impl std::ops::Deref for Config {
    type Target = tango_library::config::Config;
    fn deref(&self) -> &Self::Target {
        &self.library
    }
}

impl std::ops::DerefMut for Config {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.library
    }
}

impl Config {
    pub fn load_or_create() -> Self {
        let storage = crate::library::storage();
        let Some(path) = config_path() else {
            log::warn!("could not resolve config dir, using defaults");
            return Self::default();
        };
        let mut config: Self = match crate::library::storage::read_opt(storage, &path) {
            Ok(Some(raw)) => match serde_json::from_slice(&raw) {
                Ok(c) => c,
                Err(e) => {
                    // Don't compound a parse failure by overwriting the user's
                    // settings with defaults — park the unparseable file next
                    // door so it can be recovered or reported.
                    let backup = path.with_extension("json.bad");
                    match storage.rename(&path, &backup) {
                        Ok(()) => log::warn!("config parse failed ({e}); moved the bad file to {}", backup.display()),
                        Err(rename_err) => {
                            log::warn!(
                                "config parse failed ({e}) and backing the file up failed too ({rename_err}); \
                                 using defaults without persisting"
                            );
                            return Self::defaults();
                        }
                    }
                    let c = Self::defaults();
                    let _ = c.save();
                    c
                }
            },
            Ok(None) => {
                let c = Self::defaults();
                let _ = c.save();
                c
            }
            Err(e) => {
                // The file exists but couldn't be read (permissions, invalid
                // UTF-8, transient I/O) — it may be perfectly good on the next
                // launch, so don't overwrite it with defaults.
                log::warn!("config read failed, using defaults without persisting: {e}");
                return Self::defaults();
            }
        };
        // Host locations aren't persisted (see `cache_dir`), so bind
        // them on every load rather than only on a fresh default.
        config.library.cache_dir = cache_dir();
        config
    }

    /// Defaults with this platform's directories filled in.
    fn defaults() -> Self {
        Self {
            library: tango_library::config::Config {
                cache_dir: cache_dir(),
                ..tango_library::config::Config::with_data_path(default_data_path())
            },
            input_mapping: crate::platform::input::Mapping::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = config_path() else {
            return Err(std::io::Error::other("no config dir"));
        };
        let storage = crate::library::storage();
        if let Some(parent) = path.parent() {
            storage.create_dir_all(parent)?;
        }
        let s =
            serde_json::to_string_pretty(self).map_err(|e| std::io::Error::other(format!("serialize failed: {e}")))?;
        crate::library::storage::write_atomic(storage, &path, s.as_bytes())
    }
}

/// Debounced background writer for [`Config`]. The UI thread queues a
/// snapshot on every change ([`write`](Self::write)); a dedicated thread
/// coalesces bursts down to the newest snapshot and does the disk write,
/// so rapid selection changes cost one write and the render thread never
/// blocks on I/O. All writes happen on the one thread, so an older
/// snapshot can never land after a newer one.
pub struct Writer {
    tx: Option<std::sync::mpsc::Sender<Config>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Writer {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Config>();
        let thread = std::thread::Builder::new()
            .name("config-writer".to_string())
            .spawn(move || {
                while let Ok(mut config) = rx.recv() {
                    // Coalesce a burst into its newest snapshot.
                    while let Ok(newer) = rx.try_recv() {
                        config = newer;
                    }
                    if let Err(e) = config.save() {
                        log::error!("failed to save config: {e}");
                    }
                }
            })
            .expect("spawn config writer");
        Self {
            tx: Some(tx),
            thread: Some(thread),
        }
    }

    pub fn write(&self, config: Config) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(config);
        }
    }

    /// Drain the queue and stop the thread — called before exit so the
    /// final write (window geometry, last selection) is on disk before
    /// the process ends. Idempotent; `write` after `flush` is a no-op.
    pub fn flush(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.flush();
    }
}

/// Build the lookup key used by `Config::last_save_per_game`.
pub fn game_key(game: crate::library::rom::GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    format!("{family}/{variant}")
}

/// The platform config directory Tango stores `config.json` (and the
/// persistent client identity — see [`crate::netplay::identity`]) under.
/// `None` only when the OS user-dirs lookup fails, the same degraded
/// case [`Config::load_or_create`] already tolerates.
pub fn config_dir() -> Option<std::path::PathBuf> {
    directories_next::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION).map(|d| d.config_dir().to_path_buf())
}

/// The platform cache directory (e.g. `~/Library/Caches/net.n1gp.tango`
/// on macOS, `~/.cache/tango` on Linux) for derived data the app can
/// always recompute.
fn cache_dir() -> Option<std::path::PathBuf> {
    directories_next::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION).map(|d| d.cache_dir().to_path_buf())
}

/// Where a fresh install puts its data. Falls back to `./tango-data` if
/// the user-dirs lookup fails, so the app still runs in degraded form
/// rather than panicking.
fn default_data_path() -> std::path::PathBuf {
    directories_next::UserDirs::new()
        .and_then(|u| u.document_dir().map(|d| d.join(DATA_DIR_NAME)))
        .unwrap_or_else(|| std::path::PathBuf::from("./tango-data"))
}

fn config_path() -> Option<std::path::PathBuf> {
    config_dir().map(|d| d.join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_library::config::DEFAULT_MATCHMAKING_ENDPOINT;

    /// Splitting the settings model into a library half and a frontend
    /// half must not change what lands on disk: `flatten` puts both
    /// halves in one flat JSON object, so a config written by an older
    /// build still loads and a config written now is still one object.
    #[test]
    fn the_split_config_is_still_one_flat_json_object() {
        let json = serde_json::to_value(Config::default()).unwrap();
        let obj = json.as_object().expect("config serializes as one object");

        // Library-owned settings sit at the top level, not nested under
        // a "library" key.
        assert!(obj.contains_key("nickname"), "{obj:#?}");
        assert!(obj.contains_key("data_path"), "{obj:#?}");
        assert!(obj.contains_key("patch_repo"), "{obj:#?}");
        // As does the frontend-owned half.
        assert!(obj.contains_key("input_mapping"), "{obj:#?}");
        assert!(
            !obj.contains_key("library"),
            "the split leaked into the format: {obj:#?}"
        );
        // `cache_dir` is a host location, resolved per launch.
        assert!(!obj.contains_key("cache_dir"), "{obj:#?}");
    }

    /// A config file from before the split — one flat object — must still
    /// deserialize, with both halves populated.
    #[test]
    fn a_pre_split_config_still_loads() {
        let raw = r#"{
            "nickname": "someone",
            "streamer_mode": true,
            "patch_repo": "https://example.invalid/patches",
            "frame_delay": 7,
            "input_mapping": { "keyboard": {} }
        }"#;
        let config: Config = serde_json::from_str(raw).unwrap();
        assert_eq!(config.nickname.as_deref(), Some("someone"));
        assert!(config.streamer_mode);
        assert_eq!(config.patch_repo, "https://example.invalid/patches");
        assert_eq!(config.frame_delay, 7);
        // Unset fields fall back to their defaults on both sides.
        assert_eq!(config.matchmaking_endpoint, DEFAULT_MATCHMAKING_ENDPOINT);
        assert!(config.enable_patch_autoupdate);
    }

    /// Round-tripping must preserve both halves.
    #[test]
    fn a_round_trip_preserves_both_halves() {
        let mut config = Config::default();
        config.nickname = Some("player".into());
        config.frame_delay = 4;
        config.input_mapping.speed_up.clear();

        let back: Config = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(back.nickname.as_deref(), Some("player"));
        assert_eq!(back.frame_delay, 4);
        assert!(back.input_mapping.speed_up.is_empty());
    }
}
