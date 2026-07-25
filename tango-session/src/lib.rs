//! Emulator-session machinery, UI-toolkit-agnostic: the session kinds
//! (single-player, live PvP, replay playback), the loops that pace
//! them, the shared audio stream, and the netplay transport they run
//! over. The host owns everything presentational — views, input
//! mapping, per-session UI state — and drives a session through
//! [`Session`] plus each kind's concrete surface.
//!
//! **Sessions are driven, not self-running.** Each kind exposes a
//! driver whose `tick` advances exactly one emulated frame, and a host
//! decides what turns the crank: on a desktop, a thread per session
//! looping it against the host's own pacer; in a browser, `requestAnimationFrame`
//! and the audio sink's queue reports pumping it from the event loop,
//! where there are no threads to spawn. Nothing below the driver knows
//! which one it is. [`replay_export`] is the same shape without the
//! pacing — it pumps as fast as the encoders allow.
//!
//! Modules the browser can't have yet — the netplay transport and the
//! sessions built on it — are gated to native; the rest compiles for
//! wasm32.

// The session kinds. Each hands a host a [`Drive`] and publishes what
// the host shows; nothing here spawns or sleeps.
/// Live netplay, on the transport below.
#[cfg(not(target_arch = "wasm32"))]
pub mod pvp;
pub mod replay;
pub mod singleplayer;
pub mod training;

// What they're built out of.
pub mod audio;
/// The netplay transport: two byte-pipe planes over one peer
/// connection. Native for now — the pipes build for the browser, but
/// the signaling client that gets a peer connection in the first place
/// is a native WebSocket.
#[cfg(not(target_arch = "wasm32"))]
pub mod net;
pub mod platform;
/// The match-stats sidecar a finished match leaves next to its replay,
/// so the Replays tab never re-simulates one it has already seen.
#[cfg(not(target_arch = "wasm32"))]
pub mod stats;

/// Rendering a recorded replay to video. Not a session at all — it
/// drives the same re-simulation as fast as the encoders allow, and is
/// here because that re-simulation is.
pub mod replay_export;

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
    #[cfg(not(target_arch = "wasm32"))]
    #[error(transparent)]
    LinkBringUp(#[from] crate::net::link::BringUpError),
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

/// A pause flag a drive thread can block on — flag + condvar instead of
/// a poll-sleep, so a parked loop costs zero wakeups. A pumped host
/// reads the flag and simply doesn't tick; only [`PauseGate::wait`]
/// parks a thread, which is why it's the one native-only part.
///
/// `wait` carries a defensive timeout so a cancellation signalled
/// without a `set(false)` (or a lost notify) degrades to a slow
/// re-check instead of a wedge; cancel paths should still release the
/// gate for a prompt exit.
pub struct PauseGate {
    paused: std::sync::Mutex<bool>,
    unpaused: std::sync::Condvar,
}

impl PauseGate {
    /// Upper bound on one `wait` — how long a parked loop can take to
    /// notice out-of-band state (cancellation) nobody notified for.
    #[cfg(not(target_arch = "wasm32"))]
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
    /// their cancellation flag between waits. A host without threads
    /// polls [`PauseGate::paused`] instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(&self) {
        let g = self.paused.lock().unwrap();
        let _ = self
            .unpaused
            .wait_timeout_while(g, Self::DEFENSIVE_TICK, |paused| *paused)
            .unwrap();
    }
}

/// Fast-forward pacing target: `base_fps * factor`, clamped to
/// `[1, 4×base_fps]`. Above ~4× one audio callback interval's production
/// overshoots the [`CoreStream`](core_stream::CoreStream) discard cap and
/// fast-forward turns into constant skips, so the clamp keeps it coherent.
/// Shared by the single-player and training `set_speed`.
pub fn clamp_speed(base_fps: f32, factor: f32) -> f32 {
    (base_fps * factor).clamp(1.0, base_fps * 4.0)
}

/// One shared GBA screen — mgba-native BGR555, 2 bytes/pixel — with
/// a session's emu thread writing it and the session reading it back
/// out for the host. Internal: sessions publish
/// [`frame`](Session::frame) pixels, not surfaces. A session
/// builds one per screen it shows: its main display, plus the replay
/// PiP's opponent view. Each starts zeroed, so a fresh session never
/// flashes the previous one's last frame.
///
/// Waking the host is deliberately not part of this: a session can
/// write several screens for one tick (the replay PiP), and it has
/// state worth a repaint that isn't a frame at all, so the wake is the
/// session's ([`Session::wake`]) rather than the surface's.
pub(crate) struct Framebuffer(std::sync::Mutex<Vec<u8>>);

impl Framebuffer {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self(std::sync::Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 2)
                as usize
        ])))
    }

    /// Emu side: put this frame up. A wrong-sized `pixels` is ignored
    /// — the last good frame stays up rather than tearing the surface.
    pub fn write(&self, pixels: &[u8]) {
        let mut vbuf = self.0.lock().unwrap();
        if vbuf.len() == pixels.len() {
            vbuf.copy_from_slice(pixels);
        }
    }

    /// Host side: a copy of the frame currently up, as raw BGR555.
    pub fn read(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

/// One emulated frame at a time, for whoever is turning the crank.
///
/// Each session kind that a host drives exposes one of these. The
/// desktop runs it on a thread that sleeps to [`Drive::fps_target`]
/// between ticks; a browser calls `tick` from its event loop, paced by
/// how much audio its sink is short of. The session itself neither
/// spawns nor sleeps — that is the whole point of the split.
///
/// Not `Send`: a browser's driver is full of `Rc`s and never leaves its
/// thread. A host that wants to move one onto a thread asks for `Send`
/// where it spawns.
pub trait Drive {
    /// Advance one emulated frame, publishing whatever it produced.
    /// `false` once the session is over (dropped, ended, or failed) and
    /// there is nothing left to drive.
    fn tick(&mut self) -> bool;

    /// The rate this session wants to run at, in frames per second —
    /// realtime, or faster while the user holds fast-forward. A host
    /// paces to it, and the session's audio stream stretches to match.
    fn fps_target(&self) -> f32;
}

/// A running emulator session — replay playback, single-player, or
/// live PvP. At most one is active at a time (the host holds it as a
/// boxed trait object). The trait is the shared surface the host's
/// tick loop drives without caring which kind is running; kind-specific
/// surface (the replay transport, the PvP telemetry + panels) reaches
/// its concrete session through
/// [`downcast_ref`](dyn Session::downcast_ref) — the `Any`
/// supertrait is what makes that possible.
pub trait Session: std::any::Any {
    /// Local-perspective Game registration for this session. Used by
    /// the host to pull per-game chrome (background image, logo) into
    /// the emulator pane.
    fn local_game(&self) -> &'static tango_gamesupport::Game;

    /// This session's current display frame, as raw BGR555 (2 bytes
    /// per pixel, mgba-native) — the host uploads it to a GPU texture
    /// every repaint, so it hands back a copy rather than the live
    /// surface the emu thread is writing.
    fn frame(&self) -> Vec<u8>;

    /// Signalled whenever the host should take another look: a new
    /// frame landed, or state it re-checks on the same beat moved
    /// (`is_ended` after a peer-end packet, a link drop, a reconnect's
    /// give-up bar). The host parks one repaint stream on it, so a
    /// session that has stopped producing frames can still drive its
    /// own teardown. Coalescing — a slow host sees one wake per park,
    /// not a queue — and a wake fired before it parks isn't lost (the
    /// permit is stored). Handed out owned, so that stream can outlive
    /// its borrow of the session.
    fn wake(&self) -> std::sync::Arc<tokio::sync::Notify>;

    /// The other-perspective frame behind the picture-in-picture
    /// inset — `None` except on a replay session with the PiP toggle
    /// on and a frame captured since it was flipped on. Polled per
    /// frame by the host alongside the main [`frame`](Self::frame).
    fn pip_frame(&self) -> Option<Vec<u8>> {
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

impl dyn Session {
    /// Whether the running session is the concrete kind `T`.
    pub fn is<T: Session>(&self) -> bool {
        (self as &dyn std::any::Any).is::<T>()
    }

    /// The running session as its concrete kind, for kind-specific
    /// surface the shared trait deliberately doesn't carry (the replay
    /// transport, the PvP telemetry).
    pub fn downcast_ref<T: Session>(&self) -> Option<&T> {
        (self as &dyn std::any::Any).downcast_ref()
    }

    /// Mutable twin of [`downcast_ref`](Self::downcast_ref).
    pub fn downcast_mut<T: Session>(&mut self) -> Option<&mut T> {
        (self as &mut dyn std::any::Any).downcast_mut()
    }
}
