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

fn default_volume() -> f32 {
    1.0
}

fn default_frame_delay() -> u32 {
    2
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_language() -> unic_langid::LanguageIdentifier {
    crate::i18n::FALLBACK_LANG
}

fn ser_language<S: serde::Serializer>(lang: &unic_langid::LanguageIdentifier, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&lang.to_string())
}

fn de_language<'de, D: serde::Deserializer<'de>>(d: D) -> Result<unic_langid::LanguageIdentifier, D::Error> {
    let s = String::deserialize(d)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
}

/// Whether matchmaking connections may/must go through the TURN
/// relay. `Auto` lets ICE pick the best route (direct when possible,
/// relay as fallback); `Always` forces every candidate through the
/// relay (`ice_transport_policy = Relay`); `Never` strips the TURN
/// servers from the ICE config entirely, so only direct routes are
/// attempted.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
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
    /// When `true`, the patch autoupdater (`patch::Autoupdater`)
    /// runs in the background and refreshes the local patch
    /// directory every 15 minutes. Defaults to true; off
    /// disables the background loop but leaves the Update button
    /// in the Patches tab working.
    #[serde(default = "default_true")]
    pub enable_patch_autoupdate: bool,
    /// GPU upscale effect applied to the emulator frame while it's
    /// drawn (the native frame is uploaded once and magnified in the
    /// fragment shader). Empty = nearest-neighbor pass-through
    /// (default). Other values: "hq2x", "hq3x", "hq4x", "mmpx".
    /// See `video::framebuffer::EFFECTS`.
    #[serde(default)]
    pub video_filter: String,
    /// When true, the emulator frame uses the full fractional
    /// scale that fits the window. Default (false) snaps to the
    /// largest whole-integer multiple of the source texture so
    /// every source pixel maps to the same host-pixel count —
    /// no bilinear shimmer at non-integer scales.
    #[serde(default)]
    pub fractional_scaling: bool,
    /// When true, hide the BNLC per-game background art that
    /// sits behind the framebuffer — fall back to a plain black
    /// backdrop instead. Default (false) shows the BNLC border
    /// when the corresponding volume is installed.
    #[serde(default)]
    pub hide_emulator_border: bool,
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
    /// Last selected game *family* (region-specific gamedb family string,
    /// e.g. `"bn3"`). The family drives the picker; the concrete game is
    /// re-derived from the chosen save. Persisted separately from
    /// `last_game` so a family selected with no owned ROM still restores.
    #[serde(default)]
    pub last_family: Option<String>,
    /// Legacy (pre-loadout-model) global "last patch" selection. Read
    /// once by [`Config::migrate`] to seed [`Config::last_patch_per_save`],
    /// never written back.
    #[serde(default, skip_serializing)]
    last_patch: Option<String>,
    /// See [`Config::last_patch`].
    #[serde(default, skip_serializing)]
    last_patch_version: Option<semver::Version>,
    /// Legacy per-(game, patch) save memory, keyed
    /// `"family/variant/patch_name/patch_version"`. Read once by
    /// [`Config::migrate`] to seed `last_save_per_game` +
    /// `last_patch_per_save`, never written back.
    #[serde(default, skip_serializing)]
    last_save_per_game_per_patch: std::collections::BTreeMap<String, String>,
    /// Per-game memory of the most recent save selection. Key:
    /// `"family/variant"` ([`game_key`]). Value: forward-slash-separated
    /// path **relative to `data_path`** (e.g. `"saves/bn6/MyMan.sav"`).
    /// Storing relative + slash-joined keeps the config portable across
    /// machines and OSes. Consulted when the active game/family changes
    /// so the previously-used save is restored. (The patch is no longer
    /// part of the key — a save is the identity, and the patch hangs
    /// off it via [`Config::last_patch_per_save`].)
    #[serde(default)]
    pub last_save_per_game: std::collections::BTreeMap<String, String>,
    /// Per-save memory of the patch each save was last used with — the
    /// patch is an *overlay* on a loadout (game + save), dynamically
    /// selectable and remembered per save. Key: the save's data-relative
    /// path (same convention as `last_save_per_game` values). Value:
    /// `Some((patch_name, version))`, or `None` for "this save was last
    /// used unpatched" — distinct from a missing entry (save never
    /// selected), which lets the current patch carry over to brand-new
    /// saves. Saves created from a patch's template are seeded with that
    /// patch, encoding the intrinsic save↔patch association where one
    /// exists.
    #[serde(default)]
    pub last_patch_per_save: std::collections::BTreeMap<String, Option<(String, semver::Version)>>,
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
    /// Whether the app should launch (and stay) in fullscreen. The
    /// graphics-settings toggle calls `iced::window::set_mode` live;
    /// this value persists the user's choice across restarts.
    #[serde(default)]
    pub fullscreen: bool,
    /// Global UI scale factor, fed to `iced::application().scale_factor`.
    /// `1.0` = native; higher values enlarge every widget uniformly.
    /// Independent of the OS DPI scale — multiplies on top of it.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,

    /// User-editable input bindings (keyboard + gamepad). See
    /// [`crate::input::Mapping::default`] for the out-of-the-box
    /// layout. Each mgba key can have multiple bindings.
    #[serde(default)]
    pub input_mapping: crate::input::Mapping,
    /// Master output volume in `[0.0, 1.0]`. Multiplied into each
    /// audio sample by [`crate::audio::LateBinder`]; takes effect on
    /// the next buffer fill.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Local frame delay in frames for PvP — how far behind the live
    /// netcode frontier the display core renders. Purely local (not negotiated
    /// with the peer); snapshotted into the match at start.
    #[serde(default = "default_frame_delay")]
    pub frame_delay: u32,
    /// Relay (TURN) usage policy for matchmaking connections. See
    /// [`RelayMode`]. Sampled at connect time.
    #[serde(default)]
    pub relay_mode: RelayMode,
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
            fractional_scaling: false,
            hide_emulator_border: false,
            enable_updater: true,
            allow_prerelease_upgrades: false,
            last_game: None,
            last_family: None,
            last_patch: None,
            last_patch_version: None,
            last_save_per_game_per_patch: std::collections::BTreeMap::new(),
            last_save_per_game: std::collections::BTreeMap::new(),
            last_patch_per_save: std::collections::BTreeMap::new(),
            favorite_patches: std::collections::BTreeSet::new(),
            last_window_size: None,
            last_window_maximized: false,
            fullscreen: false,
            ui_scale: default_ui_scale(),
            input_mapping: crate::input::Mapping::default(),
            volume: 1.0,
            frame_delay: default_frame_delay(),
            relay_mode: RelayMode::default(),
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
                self.patch_repo,
                DEFAULT_PATCH_REPO,
            );
            self.patch_repo = DEFAULT_PATCH_REPO.to_string();
            let _ = self.save();
        }

        // Clamp any stale frame_delay into the supported frame-delay
        // range so the lobby slider's handle and the displayed number stay in
        // range.
        let clamped = self
            .frame_delay
            .clamp(tango_pvp::battle::MIN_FRAME_DELAY, tango_pvp::battle::MAX_FRAME_DELAY);
        if clamped != self.frame_delay {
            log::info!("clamping stale frame_delay {} -> {}", self.frame_delay, clamped);
            self.frame_delay = clamped;
            let _ = self.save();
        }

        // Selection-memory model flip: the old config remembered saves
        // per (game, patch) and the active patch globally; the new
        // model remembers one save per game and the patch per save.
        // Seed the new maps from the legacy ones once (the legacy
        // fields are skip_serializing, so they vanish from disk on the
        // next save).
        if self.last_save_per_game.is_empty() && self.last_patch_per_save.is_empty() {
            let mut migrated = false;
            for (key, rel) in &self.last_save_per_game_per_patch {
                // Key shape: "family/variant/patch_name/patch_version",
                // empty patch segments meaning "no patch". Patch names
                // can't contain '/' (they're scanner directory names),
                // so the version is everything after the last slash.
                let mut segs = key.splitn(3, '/');
                let (Some(family), Some(variant)) = (segs.next(), segs.next()) else {
                    continue;
                };
                migrated = true;
                self.last_save_per_game
                    .insert(format!("{family}/{variant}"), rel.clone());
                if let Some((name, version)) = segs.next().and_then(|rest| rest.rsplit_once('/')) {
                    if !name.is_empty() {
                        if let Ok(v) = semver::Version::parse(version) {
                            // A save filed under a patch key carries a real
                            // association — let it overwrite a no-patch entry.
                            self.last_patch_per_save
                                .insert(rel.clone(), Some((name.to_string(), v)));
                            continue;
                        }
                    }
                    self.last_patch_per_save.entry(rel.clone()).or_insert(None);
                }
            }
            // The globally-active legacy patch wins for the save it was
            // restored alongside, mirroring what the old startup path
            // would have resolved.
            if let (Some((family, variant)), Some(name), Some(version)) =
                (&self.last_game, &self.last_patch, &self.last_patch_version)
            {
                let legacy_key = format!("{family}/{variant}/{name}/{version}");
                if let Some(rel) = self.last_save_per_game_per_patch.get(&legacy_key) {
                    self.last_patch_per_save
                        .insert(rel.clone(), Some((name.clone(), version.clone())));
                }
            }
            if migrated {
                log::info!(
                    "migrated selection memory: {} save(s), {} patch association(s)",
                    self.last_save_per_game.len(),
                    self.last_patch_per_save.len(),
                );
                let _ = self.save();
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(p) = config_path() else {
            return Err(std::io::Error::other("no config dir"));
        };
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s =
            serde_json::to_string_pretty(self).map_err(|e| std::io::Error::other(format!("serialize failed: {e}")))?;
        let mut f = std::fs::File::create(&p)?;
        f.write_all(s.as_bytes())?;
        Ok(())
    }
}

/// Build the lookup key used by `Config::last_save_per_game`.
pub fn game_key(game: crate::rom::GameRef) -> String {
    let (family, variant) = game.family_and_variant();
    format!("{family}/{variant}")
}

fn config_path() -> Option<std::path::PathBuf> {
    directories_next::ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|d| d.config_dir().join("config.json"))
}
