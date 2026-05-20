//! On-disk user config. Slim version of `tango/src/config.rs` — keeps
//! Owned by the App; pulled from / written to ProjectDirs.

use serde::{Deserialize, Serialize};
use std::io::Write;

const DATA_DIR_NAME: &str = "Tango";

const QUALIFIER: &str = "net";
const ORGANIZATION: &str = "n1gp";
const APPLICATION: &str = "tango";

pub const DEFAULT_MATCHMAKING_ENDPOINT: &str = "wss://matchmaking.tango.n1gp.net";
pub const DEFAULT_PATCH_REPO: &str = "https://patches.tango.n1gp.net";

fn default_matchmaking_endpoint() -> String {
    DEFAULT_MATCHMAKING_ENDPOINT.to_string()
}

fn default_patch_repo() -> String {
    DEFAULT_PATCH_REPO.to_string()
}

fn default_true() -> bool {
    true
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

/// Picks the live netplay throttler strategy (see
/// `tango_pvp::battle::throttler`). Persisted in config; applied at the
/// start of every new round, so a change in the settings panel takes
/// effect at the next round boundary without a session restart.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum NetplayThrottler {
    /// Continuous EMA, asymmetric (gentle slowdown, snappy recovery).
    /// Default.
    #[default]
    AsymmetricEma,
    /// Idle-until-tripped deadband + linear engagement.
    LinearWatchdog,
    /// Power-law on instantaneous skew.
    Power,
}

impl std::fmt::Display for NetplayThrottler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NetplayThrottler::AsymmetricEma => "Asymmetric EMA",
            NetplayThrottler::LinearWatchdog => "Linear Watchdog",
            NetplayThrottler::Power => "Power (legacy)",
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
    /// When `true`, the patch autoupdater (`patch::Autoupdater`)
    /// runs in the background and refreshes the local patch
    /// directory every 15 minutes. Defaults to true; off
    /// disables the background loop but leaves the Update button
    /// in the Patches tab working.
    #[serde(default = "default_true")]
    pub enable_patch_autoupdate: bool,
    /// Upscaler applied to each emulator frame before it's
    /// uploaded to the GPU. Empty / "null" = nearest-neighbor
    /// (default). Other values: "hq2x", "hq3x", "hq4x", "mmpx".
    /// See `video::filter_by_name`.
    #[serde(default)]
    pub video_filter: String,
    /// When true, the emulator frame is rendered at the largest
    /// integer multiple of its texture size that fits the
    /// window (instead of the default `ContentFit::Contain`,
    /// which fractionally scales). Keeps every source pixel
    /// at uniform host-pixel size — no bilinear shimmer at
    /// non-integer scales.
    #[serde(default)]
    pub integer_scaling: bool,
    /// When true, the self-updater (`updater::Updater`) runs in
    /// the background and downloads any newer GitHub release.
    /// Toggle takes effect immediately via Settings; downloaded
    /// updates are applied on the next launch (or via the
    /// "Update Now" button in About).
    #[serde(default = "default_true")]
    pub enable_updater: bool,
    /// When true, the updater treats prereleases (semver pre
    /// segment, or GitHub-marked) as upgrade candidates.
    /// Sampled once at start; toggling requires a restart.
    #[serde(default)]
    pub allow_prerelease_upgrades: bool,

    pub last_game: Option<(String, u8)>,
    pub last_patch: Option<String>,
    pub last_patch_version: Option<semver::Version>,
    /// Per-game-per-patch memory of the most recent save selection.
    /// Key: `"family/variant/patch_name/patch_version"` (empty
    /// `patch_name`/`patch_version` segments mean "raw ROM, no patch").
    /// Value: forward-slash-separated path **relative to `data_path`**
    /// (e.g. `"saves/bn6/MyMan.sav"`). Storing relative + slash-joined
    /// keeps the config portable across machines and OSes. Consulted
    /// whenever the active game or patch changes so the previously-used
    /// save for that combination is restored.
    #[serde(default)]
    pub last_save_per_game_per_patch: std::collections::BTreeMap<String, String>,
    /// Names of patches the user has favorited — they sort to the top
    /// of pickers and get a star glyph next to their label.
    #[serde(default)]
    pub favorite_patches: std::collections::BTreeSet<String>,
    /// Last unmaximized window size (logical pixels). Used as the
    /// `iced::window::Settings::size` at startup so the window comes
    /// back at the size the user left it. Updated on every Resized
    /// event *only* when the window isn't currently maximized — so
    /// maximizing + closing doesn't overwrite the restore size with
    /// the screen dimensions.
    #[serde(default)]
    pub last_window_size: Option<(f32, f32)>,
    /// Whether the window was maximized at last shutdown. Used to set
    /// `iced::window::Settings::maximized` at startup.
    #[serde(default)]
    pub last_window_maximized: bool,

    /// User-editable input bindings (keyboard + gamepad). See
    /// [`crate::input::Mapping::default`] for the out-of-the-box
    /// layout. Each mgba key can have multiple bindings.
    #[serde(default)]
    pub input_mapping: crate::input::Mapping,
    /// Picks the netplay throttler strategy used at every new round.
    #[serde(default)]
    pub netplay_throttler: NetplayThrottler,
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
            enable_patch_autoupdate: true,
            video_filter: String::new(),
            integer_scaling: false,
            enable_updater: true,
            allow_prerelease_upgrades: false,
            last_game: None,
            last_patch: None,
            last_patch_version: None,
            last_save_per_game_per_patch: std::collections::BTreeMap::new(),
            favorite_patches: std::collections::BTreeSet::new(),
            last_window_size: None,
            last_window_maximized: false,
            input_mapping: crate::input::Mapping::default(),
            netplay_throttler: NetplayThrottler::default(),
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
    pub fn logs_path(&self) -> std::path::PathBuf {
        self.data_path.join("logs")
    }

    /// Convert an absolute path under `data_path` to the
    /// forward-slash-separated relative string used as a value in
    /// `last_save_per_game_per_patch`. Returns `None` if the path is
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

    pub fn load_or_create() -> Self {
        match config_path() {
            Some(p) => match std::fs::read_to_string(&p) {
                Ok(s) => match serde_json::from_str::<Self>(&s) {
                    Ok(mut c) => {
                        c.migrate();
                        return c;
                    }
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

    /// One-shot config migrations applied on load. Keeps stale
    /// values from breaking the app after a default change.
    fn migrate(&mut self) {
        // Old default pointed at the github repo page, which serves
        // HTML — the patch updater needs the static-file host. If
        // the user is still on the legacy default, silently move
        // them to the new one.
        const STALE_PATCH_REPOS: &[&str] = &[
            "https://github.com/tangobattle/patches",
            "https://github.com/tangobattle/patches/",
        ];
        if STALE_PATCH_REPOS.iter().any(|u| self.patch_repo.eq(*u)) {
            log::info!(
                "migrating stale patch_repo {:?} -> {:?}",
                self.patch_repo, DEFAULT_PATCH_REPO,
            );
            self.patch_repo = DEFAULT_PATCH_REPO.to_string();
            let _ = self.save();
        }
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

/// Build the lookup key used by `Config::last_save_per_game_per_patch`.
/// Empty patch name + version mean "no patch" so a save chosen for the
/// raw ROM doesn't collide with a save chosen under a patch.
pub fn save_memory_key(
    game: crate::rom::GameRef,
    patch_name: Option<&str>,
    patch_version: Option<&semver::Version>,
) -> String {
    let (family, variant) = game.family_and_variant();
    format!(
        "{family}/{variant}/{}/{}",
        patch_name.unwrap_or(""),
        patch_version.map(|v| v.to_string()).unwrap_or_default(),
    )
}

fn config_path() -> Option<std::path::PathBuf> {
    directories_next::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|d| d.config_dir().join("config.json"))
}
