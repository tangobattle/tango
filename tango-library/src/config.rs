//! The persisted user config: the model, the derived content paths, and
//! its load/save.
//!
//! *Where* it is stored is the frontend's business — natively that is
//! the platform config dir, in a browser an OPFS key — so the paths come
//! in as arguments rather than being resolved here.

use crate::storage::{self, Storage};
use serde::{Deserialize, Serialize};

pub const DATA_DIR_NAME: &str = "Tango";

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

fn default_setup_pane_widths() -> [f32; 2] {
    [420.0; 2]
}

fn default_language() -> unic_langid::LanguageIdentifier {
    crate::lang::FALLBACK_LANG
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

/// Which color the UI chrome runs in — the palette `primary` that
/// paints CTA buttons, panel frames, glows, and the cyberworld
/// backdrop. The structure never changes; only the accent swaps.
/// Colors live in `theme::accent_color` (per dark/light shade),
/// this enum is just the persisted choice.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum AccentColor {
    #[default]
    TangoGreen,
    MegaManBlue,
    ProtoManRed,
    RollPink,
    GutsManYellow,
    /// Was `BassGold` before Bass went to his canon violet (the gold
    /// moved to GutsMan); the alias keeps existing configs loading.
    #[serde(alias = "BassGold")]
    BassPurple,
}

/// How a two-screen console's screens are arranged in the emulator
/// pane. Pure presentation: the session always composes its frame the
/// same way, and the frontend re-lays it out at draw time — which is
/// what lets this switch take effect mid-session.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum DsScreenStacking {
    /// The console's own arrangement, one screen above the other. The
    /// default: it's the shape players know the games by.
    #[default]
    Vertical,
    /// Side by side.
    Horizontal,
    /// Only the primary screen (see [`DsPrimaryScreen`]), full pane.
    PrimaryOnly,
}

/// Which DS screen leads the arrangement — sits on the left of a
/// horizontal pair, or on top of a vertical stack. Presentation only,
/// like [`DsScreenStacking`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum DsPrimaryScreen {
    /// The console's upper screen — where these games put the battle.
    #[default]
    Upper,
    /// The touch screen.
    Touch,
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
    pub accent: AccentColor,
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
    /// How a DS game's two screens stack in the emulator pane.
    /// Applied at draw time, so switching it mid-session re-lays the
    /// pane out immediately. Ignored for single-screen consoles.
    #[serde(default)]
    pub ds_screen_stacking: DsScreenStacking,
    /// Which DS screen leads the arrangement (left of a horizontal
    /// pair, top of a vertical stack). Applied at draw time like
    /// [`ds_screen_stacking`](Self::ds_screen_stacking).
    #[serde(default)]
    pub ds_primary_screen: DsPrimaryScreen,
    /// When true, hide the BNLC per-game background art that
    /// sits behind the framebuffer — fall back to a plain black
    /// backdrop instead. Default (false) shows the BNLC border
    /// when the corresponding volume is installed.
    #[serde(default)]
    pub hide_emulator_border: bool,
    /// When true, replay playback shows the input display overlay:
    /// one pad chip per side with the recorded buttons lit at the
    /// playhead. Toggled from the replay transport bar.
    #[serde(default)]
    pub show_replay_inputs: bool,
    /// When true, replay playback shows the opponent's screen as a
    /// picture-in-picture inset (their perspective is re-simulated
    /// anyway; this turns its renderer on). Toggled from the replay
    /// transport bar, like [`show_replay_inputs`](Self::show_replay_inputs).
    #[serde(default)]
    pub show_opponent_pip: bool,
    /// Width in logical pixels of each PvP setup drawer, `[self,
    /// opponent]`. Dragged from the drawer's inner edge during a match
    /// and persisted on release, so the next one opens the panes where
    /// the user left them.
    #[serde(default = "default_setup_pane_widths")]
    pub pvp_setup_pane_widths: [f32; 2],
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
    /// Last *fullscreen* window position (logical pixels) — the
    /// monitor origin the window parks at while fullscreen. Updated on
    /// Moved events only while fullscreen, and restored as the startup
    /// position only for a fullscreen relaunch, so it puts a fullscreen
    /// Tango back on the right monitor. Windowed positions are not
    /// persisted: restoring an exact x/y is janky on multi-monitor
    /// setups (saved coords can land off-screen or on the wrong
    /// display).
    #[serde(default)]
    pub last_window_position: Option<(f32, f32)>,
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

    /// Master output volume in `[0.0, 1.0]`. Multiplied into each
    /// audio sample by the frontend's audio binder; takes effect on
    /// the next buffer fill.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// When true, PvP sessions install the per-game BGM-skip trap so
    /// battle music never starts (sound effects still play). Local-only,
    /// like the volume; sampled at match start.
    #[serde(default)]
    pub disable_bgm_in_pvp: bool,
    /// Local frame delay in frames for PvP — how far behind the live
    /// netcode frontier the display core renders. Purely local (not negotiated
    /// with the peer); snapshotted into the match at start.
    #[serde(default = "default_frame_delay")]
    pub frame_delay: u32,
    /// Relay (TURN) usage policy for matchmaking connections. See
    /// [`RelayMode`]. Sampled at connect time.
    #[serde(default)]
    pub relay_mode: RelayMode,
    /// Last "blind my setup from the opponent" choice made in the
    /// netplay lobby. Seeded into `LobbyState::blind_setup` at connect
    /// time so the checkbox comes back the way the user last left it;
    /// each lobby remains independently toggleable thereafter.
    #[serde(default)]
    pub last_blind_setup: bool,
    /// Slide the opponent's setup drawer open automatically at PvP
    /// match start (when they haven't blinded their setup). Off, the
    /// drawer starts closed and the edge handle is the invitation.
    /// Sampled once when the session is installed; the drawer stays
    /// freely toggleable afterwards.
    #[serde(default)]
    pub show_opponent_setup: bool,
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
            nickname: None,
            language: default_language(),
            streamer_mode: false,
            theme: ThemeMode::default(),
            accent: AccentColor::default(),
            data_path,
            matchmaking_endpoint: default_matchmaking_endpoint(),
            patch_repo: default_patch_repo(),
            enable_patch_autoupdate: true,
            video_filter: String::new(),
            fractional_scaling: false,
            ds_screen_stacking: DsScreenStacking::default(),
            ds_primary_screen: DsPrimaryScreen::default(),
            hide_emulator_border: false,
            show_replay_inputs: false,
            show_opponent_pip: false,
            pvp_setup_pane_widths: default_setup_pane_widths(),
            enable_updater: true,
            allow_prerelease_upgrades: false,
            last_game: None,
            last_family: None,
            last_save_per_family: std::collections::BTreeMap::new(),
            last_patch_per_save: std::collections::BTreeMap::new(),
            last_match_type_per_family: std::collections::BTreeMap::new(),
            favorite_patches: std::collections::BTreeSet::new(),
            last_window_size: None,
            last_window_maximized: false,
            last_window_position: None,
            fullscreen: false,
            ui_scale: default_ui_scale(),
            volume: 1.0,
            disable_bgm_in_pvp: false,
            frame_delay: default_frame_delay(),
            relay_mode: RelayMode::default(),
            last_blind_setup: false,
            show_opponent_setup: false,
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
    pub fn logs_path(&self) -> std::path::PathBuf {
        self.data_path.join("logs")
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

    /// Read the config at `path`, falling back to defaults (and
    /// creating the file) when it isn't there.
    /// Defaults rooted at `data_path`. Resolving that root is the
    /// frontend's job — the platform documents directory natively, an
    /// OPFS root in a browser.
    pub fn with_data_path(data_path: std::path::PathBuf) -> Self {
        Self {
            data_path,
            ..Default::default()
        }
    }

    pub fn load_or_create(storage: &dyn Storage, path: &std::path::Path, data_path: &std::path::Path) -> Self {
        let defaults = || Self::with_data_path(data_path.to_path_buf());
        match storage::read_opt(storage, path) {
            Ok(Some(raw)) => match serde_json::from_slice::<Self>(&raw) {
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
        let _ = c.save(storage, path);
        c
    }

    /// Write-then-rename, so an interrupted save can't leave a truncated
    /// config.json behind.
    pub fn save(&self, storage: &dyn Storage, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            storage.create_dir_all(parent)?;
        }
        let s =
            serde_json::to_string_pretty(self).map_err(|e| std::io::Error::other(format!("serialize failed: {e}")))?;
        storage::write_atomic(storage, path, s.as_bytes())
    }
}

/// File name of the config within whatever directory the frontend
/// resolves for it.
pub const FILE_NAME: &str = "config.json";
