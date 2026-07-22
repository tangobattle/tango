//! Live emulator-session machinery, UI-toolkit-agnostic: the session
//! kinds (single-player, live PvP, replay playback), the drive threads
//! that pace them, the shared audio stream, and the netplay transport
//! they run over. The app owns everything presentational — views,
//! input mapping, per-session UI state — and drives a session through
//! [`ActiveSession`] plus each kind's concrete surface.

pub mod audio;
pub mod core_stream;
pub mod net;
pub mod pvp;
pub mod replay;
pub mod singleplayer;
pub mod stats;
pub mod stats_cache;

/// Why a session failed to construct or boot, any kind. One enum for
/// all three session kinds — their failure sets overlap heavily (core
/// boot, thread spawn, engine priming), and hosts route every variant
/// the same way (log + stay on the menu).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Mgba(#[from] mgba::Error),
    /// File IO (the single-player save open, the replay writer) or a
    /// failed thread spawn.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The match engine failed to boot or prime the pair.
    #[error(transparent)]
    Engine(#[from] tango_match::Error),
    /// The netplay handoff's transport bundle failed to assemble.
    #[error(transparent)]
    LinkBringUp(#[from] net::link::BringUpError),
    #[error("replay has a bad local player index")]
    BadLocalPlayerIndex,
    #[error("replay has no inputs")]
    EmptyReplay,
    /// A side's committed SRAM dump didn't parse as a save for its game.
    #[error("parse {side} save: {source}")]
    ParseSave {
        side: &'static str,
        #[source]
        source: tango_gamesupport::Error,
    },
    /// A side's negotiated settings arrived without game info.
    #[error("{side} settings missing game info")]
    MissingGameInfo { side: &'static str },
    /// The PvP drive thread died before reporting boot success.
    #[error("sio drive thread died during boot")]
    DriveThreadDied,
}

/// Create the mgba core every session boots from: a GBA core with audio-sync
/// on, its video buffer enabled, and `rom` loaded. Callers then load the save
/// (which differs per session — RW file vs in-memory SRAM dump) and install
/// their own traps.
pub fn new_gba_core(rom: &[u8]) -> Result<mgba::core::OwnedCore, mgba::Error> {
    let mut core = mgba::core::OwnedCore::new_gba(
        "tango",
        &mgba::core::Options {
            audio_sync: true,
            ..Default::default()
        },
    )?;
    core.enable_video_buffer();
    core.load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
    Ok(core)
}

/// A pause flag a drive thread can block on — flag + condvar instead of a
/// poll-sleep, so a parked loop costs zero wakeups. `wait` carries a
/// defensive timeout so a cancellation signalled without a `set(false)`
/// (or a lost notify) degrades to a slow re-check instead of a wedge;
/// cancel paths should still release the gate for a prompt exit.
pub struct PauseGate {
    paused: std::sync::Mutex<bool>,
    unpaused: std::sync::Condvar,
}

impl PauseGate {
    /// Upper bound on one `wait` — how long a parked loop can take to
    /// notice out-of-band state (cancellation) nobody notified for.
    const DEFENSIVE_TICK: std::time::Duration = std::time::Duration::from_millis(250);

    pub fn new(paused: bool) -> Self {
        Self {
            paused: std::sync::Mutex::new(paused),
            unpaused: std::sync::Condvar::new(),
        }
    }

    pub fn paused(&self) -> bool {
        *self.paused.lock().unwrap()
    }

    pub fn set(&self, paused: bool) {
        *self.paused.lock().unwrap() = paused;
        if !paused {
            self.unpaused.notify_all();
        }
    }

    /// Park until unpaused or the defensive tick elapses (returns
    /// immediately if not paused). Callers loop around this, re-checking
    /// their cancellation flag between waits.
    pub fn wait(&self) {
        let g = self.paused.lock().unwrap();
        let _ = self
            .unpaused
            .wait_timeout_while(g, Self::DEFENSIVE_TICK, |paused| *paused)
            .unwrap();
    }
}

/// Per-session UI ↔ emu-thread frame plumbing: the shared GBA
/// framebuffer (mgba-native BGR555, 2 bytes/pixel) the session's frame
/// callback `copy_from_slice`s into once per emu vblank, and the wake
/// handle it `notify_one()`s whenever a new frame lands or `is_ended`
/// could flip (the PvP end-detection wires). Every session constructor
/// builds its own, so a fresh session always starts on a zeroed
/// framebuffer with no stale wake pending — no cross-session wipe
/// dance. Carries no identity: if the host needs to tell one session's
/// sink from the next (e.g. to key a UI wake stream), that's the
/// host's bookkeeping — a pointer to the `Notify` won't do (a new
/// session's allocation can reuse a dropped one's address).
pub struct FrameSink {
    pub notify: std::sync::Arc<tokio::sync::Notify>,
    pub vbuf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
}

impl FrameSink {
    pub fn new() -> Self {
        Self {
            notify: std::sync::Arc::new(tokio::sync::Notify::new()),
            vbuf: std::sync::Arc::new(std::sync::Mutex::new(vec![
                0u8;
                (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 2)
                    as usize
            ])),
        }
    }
}

/// A running emulator session — replay playback, single-player, or
/// live PvP. At most one is active at a time (the host holds it as a
/// boxed trait object). The trait is the shared surface the host's
/// tick loop drives without caring which kind is running; kind-specific
/// surface (the replay transport, the PvP telemetry + panels) reaches
/// its concrete session through
/// [`downcast_ref`](dyn ActiveSession::downcast_ref) — the `Any`
/// supertrait is what makes that possible.
pub trait ActiveSession: std::any::Any {
    /// Local-perspective Game registration for this session. Used by
    /// the host to pull per-game chrome (background image, logo) into
    /// the emulator pane.
    fn local_game(&self) -> &'static tango_gamesupport::Game;

    /// This session's frame surfaces + wake handle — built fresh by
    /// its constructor, see [`FrameSink`].
    fn frame_sink(&self) -> &FrameSink;

    /// Latest other-perspective frame for the picture-in-picture
    /// inset, as raw BGR555 — `None` except on a replay session with
    /// the PiP toggle on. Polled per frame by the host alongside the
    /// main [`frame_sink`](Self::frame_sink) read.
    fn pip_pixels(&self) -> Option<Vec<u8>> {
        None
    }

    /// Overwrite the entire mgba joyflag bitmap — the configurable
    /// input mapping resolves multiple held bindings into one flag
    /// word and pushes the result here every event. Default no-op:
    /// replay playback feeds recorded input instead.
    fn set_joyflags(&self, _joyflags: u32) {}

    /// Drive the session at `factor` × realtime (fast-forward /
    /// slow-mo). Default no-op: PvP runs at fixed EXPECTED_FPS so
    /// both sides stay in sync — no speed control.
    fn set_speed(&self, _factor: f32) {}

    /// Pre-drop teardown. Default no-op — only PvP has any: it cancels
    /// its token so the receive loop announces the quit to the peer
    /// instead of leaving them hanging on a reconnect window. Replay
    /// and single-player sessions close by being dropped (the mgba
    /// thread joins in Drop).
    fn request_close(&self) {}

    /// True once the session has ended on its own — currently used
    /// by PvP so a peer-disconnect / comm error tears the session
    /// view down automatically instead of leaving the user staring
    /// at a frozen frame.
    fn is_ended(&self) -> bool {
        false
    }
}

impl dyn ActiveSession {
    /// Whether the running session is the concrete kind `T`.
    pub fn is<T: ActiveSession>(&self) -> bool {
        (self as &dyn std::any::Any).is::<T>()
    }

    /// The running session as its concrete kind, for kind-specific
    /// surface the shared trait deliberately doesn't carry (the replay
    /// transport, the PvP telemetry).
    pub fn downcast_ref<T: ActiveSession>(&self) -> Option<&T> {
        (self as &dyn std::any::Any).downcast_ref()
    }

    /// Mutable twin of [`downcast_ref`](Self::downcast_ref).
    pub fn downcast_mut<T: ActiveSession>(&mut self) -> Option<&mut T> {
        (self as &mut dyn std::any::Any).downcast_mut()
    }
}
