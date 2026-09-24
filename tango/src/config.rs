//! This frontend's config: the library's settings model plus the
//! preferences only this frontend has, and the platform locations it all
//! lives in.
//!
//! [`tango_library::config::Config`] holds what both hosts read: paths,
//! service endpoints, and the library's selection memory. Everything
//! else — identity, appearance, window geometry, audio, netplay knobs,
//! and the input mapping — is this frontend's, so it lives here,
//! `flatten`ed into the same JSON object. The on-disk format is the one
//! the single-struct config wrote, and [`Config`] derefs to the
//! library's, so every `config.<library field>` access reads as before.

use serde::{Deserialize, Serialize};

/// The folder a fresh install keeps its data in, under the user's
/// documents directory.
const DATA_DIR_NAME: &str = "Tango";
/// File name of the config within the platform config directory.
const FILE_NAME: &str = "config.json";

const QUALIFIER: &str = "net";
const ORGANIZATION: &str = "n1gp";
const APPLICATION: &str = "tango";

fn default_true() -> bool {
    true
}

fn default_volume() -> f32 {
    1.0
}

fn default_frame_delay() -> u32 {
    crate::session::pvp::DEFAULT_FRAME_DELAY
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_setup_pane_widths() -> [f32; 2] {
    [420.0; 2]
}

fn default_language() -> unic_langid::LanguageIdentifier {
    tango_library::lang::FALLBACK_LANG
}

fn ser_language<S: serde::Serializer>(lang: &unic_langid::LanguageIdentifier, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&lang.to_string())
}

fn de_language<'de, D: serde::Deserializer<'de>>(d: D) -> Result<unic_langid::LanguageIdentifier, D::Error> {
    let s = String::deserialize(d)?;
    s.parse().map_err(serde::de::Error::custom)
}

/// A stored frame delay from outside the supported range (hand-edited,
/// or written when the range was wider) is pulled into it on load, so
/// everything downstream can take `frame_delay` as valid.
fn de_frame_delay<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    Ok(crate::session::pvp::clamp_frame_delay(u32::deserialize(d)?))
}

/// How the second player's screen is presented during modes that render
/// both perspectives (replay playback and training).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum OpponentView {
    /// Do not render the other perspective.
    #[default]
    Off,
    /// Float the other perspective over the main screen.
    PictureInPicture,
    /// Give both perspectives equal left/right panes.
    StackHorizontally,
    /// Give both perspectives equal top/bottom panes.
    StackVertically,
}

/// Read the old `show_opponent_pip: bool` setting as well as the new
/// four-way enum. The field itself carries a serde alias below, so an
/// existing `true` becomes picture-in-picture without invalidating the
/// user's whole config file.
fn de_opponent_view<'de, D: serde::Deserializer<'de>>(d: D) -> Result<OpponentView, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        View(OpponentView),
        LegacyPip(bool),
    }

    Ok(match Wire::deserialize(d)? {
        Wire::View(view) => view,
        Wire::LegacyPip(true) => OpponentView::PictureInPicture,
        Wire::LegacyPip(false) => OpponentView::Off,
    })
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
    #[serde(flatten)]
    pub library: tango_library::config::Config,

    pub nickname: Option<String>,
    #[serde(serialize_with = "ser_language", deserialize_with = "de_language")]
    pub language: unic_langid::LanguageIdentifier,
    pub streamer_mode: bool,
    pub theme: ThemeMode,
    pub accent: AccentColor,
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
    /// How replay playback and training present the opponent's screen.
    /// The renderer only runs while this is not [`OpponentView::Off`].
    /// `show_opponent_pip` is the pre-menu field name and remains a read
    /// alias so existing configs migrate in place.
    #[serde(default, alias = "show_opponent_pip", deserialize_with = "de_opponent_view")]
    pub opponent_view: OpponentView,
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
    /// Safe mode for the geometry above: written just before the window
    /// is built with it, cleared as soon as the window opens. Finding it
    /// still set at startup means the previous launch died on the way to
    /// its first frame — which a restored size can cause. The saved size
    /// is *logical*, so the surface it asks for is that size times the
    /// DPI scale of whatever monitor the window lands on: a window sized
    /// on a 4K screen at 100% asks for an 8700×6672 surface when it
    /// reopens on a 300%-scaled panel, past the 8192 px maximum texture
    /// dimension, and `Surface::configure` answers that with a panic
    /// rather than an error. Which monitor that will be is unknowable
    /// before the window exists, so instead we notice the crash after
    /// the fact and drop the geometry instead of looping on it.
    #[serde(default)]
    pub window_geometry_unverified: bool,
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
    #[serde(default = "default_frame_delay", deserialize_with = "de_frame_delay")]
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

    /// User-editable input bindings (keyboard + gamepad). See
    /// [`crate::platform::input::Mapping::default`] for the
    /// out-of-the-box layout. Each GBA key can have multiple bindings.
    pub input_mapping: crate::platform::input::Mapping,

    /// Replay-viewer preference: temporarily run at least 2× while either
    /// player is in the custom screen.
    pub replay_custom_screen_speedup: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            library: tango_library::config::Config::default(),
            nickname: None,
            language: default_language(),
            streamer_mode: false,
            theme: ThemeMode::default(),
            accent: AccentColor::default(),
            enable_patch_autoupdate: true,
            video_filter: String::new(),
            fractional_scaling: false,
            ds_screen_stacking: DsScreenStacking::default(),
            ds_primary_screen: DsPrimaryScreen::default(),
            hide_emulator_border: false,
            show_replay_inputs: false,
            opponent_view: OpponentView::Off,
            pvp_setup_pane_widths: default_setup_pane_widths(),
            enable_updater: true,
            allow_prerelease_upgrades: false,
            last_window_size: None,
            last_window_maximized: false,
            window_geometry_unverified: false,
            last_window_position: None,
            fullscreen: false,
            ui_scale: default_ui_scale(),
            volume: default_volume(),
            disable_bgm_in_pvp: false,
            frame_delay: default_frame_delay(),
            relay_mode: RelayMode::default(),
            last_blind_setup: false,
            show_opponent_setup: false,
            input_mapping: crate::platform::input::Mapping::default(),
            replay_custom_screen_speedup: false,
        }
    }
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
    /// Where the crash supervisor rotates its session logs.
    pub fn logs_path(&self) -> std::path::PathBuf {
        self.data_path.join("logs")
    }

    pub fn load_or_create() -> Self {
        let Some(path) = config_path() else {
            log::warn!("could not resolve config dir, using defaults");
            return Self::default();
        };
        let mut config = tango_library::config::load_json_or_create(crate::library::storage(), &path, Self::defaults);
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
            ..Self::default()
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = config_path() else {
            return Err(std::io::Error::other("no config dir"));
        };
        tango_library::config::save_json(crate::library::storage(), &path, self)
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

/// The platform config directory Tango stores `config.json` under.
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

    /// A `config.json` as the single-struct config wrote it, every key
    /// present and every value off its default, so a key either half
    /// stops reading shows up as a changed value on the way back out.
    fn every_key() -> serde_json::Value {
        serde_json::json!({
            "nickname": "someone",
            "language": "ja-JP",
            "streamer_mode": true,
            "theme": "Light",
            "accent": "RollPink",
            "data_path": "/home/someone/Tango",
            "matchmaking_endpoint": "wss://example.invalid/mm",
            "patch_repo": "https://example.invalid/patches",
            "enable_patch_autoupdate": false,
            "video_filter": "hq3x",
            "fractional_scaling": true,
            "ds_screen_stacking": "Horizontal",
            "ds_primary_screen": "Touch",
            "hide_emulator_border": true,
            "show_replay_inputs": true,
            "opponent_view": "StackVertically",
            "pvp_setup_pane_widths": [300.0, 512.5],
            "enable_updater": false,
            "allow_prerelease_upgrades": true,
            "last_game": ["bn6", 1],
            "last_family": "bn6",
            "last_save_per_family": { "bn6": "saves/falzar.sav" },
            "last_patch_per_save": {
                "saves/falzar.sav": ["exe6_pvp", "1.2.3"],
                "saves/gregar.sav": null,
            },
            "last_match_type_per_family": { "bn6": [1, 0] },
            "favorite_patches": ["exe6_pvp"],
            "last_window_size": [1280.0, 720.0],
            "last_window_maximized": true,
            "window_geometry_unverified": true,
            "last_window_position": [-1920.0, 0.0],
            "fullscreen": true,
            "ui_scale": 1.25,
            "volume": 0.5,
            "disable_bgm_in_pvp": true,
            "frame_delay": 7,
            "relay_mode": "Never",
            "last_blind_setup": true,
            "show_opponent_setup": true,
            "input_mapping": serde_json::to_value(crate::platform::input::Mapping::default()).unwrap(),
            "replay_custom_screen_speedup": true,
        })
    }

    /// Splitting the settings model into a library half and a frontend
    /// half must not change what lands on disk: a full config loads into
    /// the two halves and writes back out as the same flat object, key
    /// for key and value for value.
    #[test]
    fn a_full_config_round_trips_unchanged() {
        let raw = every_key();
        let config: Config = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&config).unwrap(), raw);
    }

    /// A fresh config writes exactly the keys the single-struct config
    /// did: the library half isn't nested under a `library` key, and
    /// `cache_dir` (a host location, resolved per launch) stays out.
    #[test]
    fn a_default_config_writes_the_same_keys() {
        let keys = |v: &serde_json::Value| {
            v.as_object()
                .expect("config serializes as one object")
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(
            keys(&serde_json::to_value(Config::default()).unwrap()),
            keys(&every_key())
        );
    }

    /// A config file from an older build — missing keys added since —
    /// still loads, with unset fields on both halves at their defaults.
    #[test]
    fn an_old_config_missing_keys_still_loads() {
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
        assert_eq!(config.matchmaking_endpoint, DEFAULT_MATCHMAKING_ENDPOINT);
        assert!(config.enable_patch_autoupdate);
        assert!(config.enable_updater);
        assert_eq!(config.ui_scale, 1.0);
        assert_eq!(config.volume, 1.0);
        assert_eq!(config.pvp_setup_pane_widths, [420.0; 2]);
        assert_eq!(config.language, tango_library::lang::FALLBACK_LANG);
        assert_eq!(config.opponent_view, OpponentView::Off);
    }

    #[test]
    fn legacy_names_still_load() {
        let config: Config =
            serde_json::from_value(serde_json::json!({ "show_opponent_pip": true, "accent": "BassGold" })).unwrap();
        assert_eq!(config.opponent_view, OpponentView::PictureInPicture);
        assert_eq!(config.accent, AccentColor::BassPurple);

        let off: Config = serde_json::from_value(serde_json::json!({ "show_opponent_pip": false })).unwrap();
        assert_eq!(off.opponent_view, OpponentView::Off);

        let json = serde_json::to_value(config).unwrap();
        assert_eq!(json.get("opponent_view"), Some(&serde_json::json!("PictureInPicture")));
        assert!(json.get("show_opponent_pip").is_none());
    }

    #[test]
    fn an_out_of_range_frame_delay_is_clamped_on_load() {
        let config: Config = serde_json::from_value(serde_json::json!({ "frame_delay": 99 })).unwrap();
        assert_eq!(config.frame_delay, crate::session::pvp::MAX_FRAME_DELAY);
    }
}
