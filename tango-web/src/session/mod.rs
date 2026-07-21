//! Session state: what a running emulator shares with the UI and the
//! audio pump, plus the per-kind drivers the runtime ticks. Two kinds —
//! PvP (rollback via `tango_pvp::engine::Match`, lands at M4) and local
//! (a solo machine) — publish the same [`SharedSession`] so the session
//! view renders them uniformly.
//!
//! Sessions are keyed on the detected [`tango_gamesupport::Game`]: the
//! boot path goes through the library's `detect` and carries the
//! registration, so per-game support (save parsing, PvP hooks, display
//! strings) is always in reach. Tango links are cable-only — there is
//! no wireless adapter on this port.

pub mod local;
pub mod pvp;
pub mod replay;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::library::GameRef;

/// GBA cycles per second / cycles per frame — the exact tick rate.
pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

#[derive(Debug, Clone)]
pub enum SessionEnd {
    LocalQuit,
    /// The games' own match-end path ran to completion on both sides.
    /// Carries the confirmed round tally, local-oriented.
    MatchEnded {
        wins: u32,
        losses: u32,
        draws: u32,
    },
    /// A replay played through its whole recorded stream.
    ReplayFinished,
    Error(String),
}

/// Uniform access to a live link for audio readout, for sessions driven
/// through the rollback engine (which owns its link) and ones we drive
/// directly.
#[derive(Clone)]
pub enum LinkAccess {
    Handle(mgba_siolink::session::LinkHandle),
    Shared(Arc<Mutex<mgba_siolink::Link>>),
    /// A playback pair: the link lives inside the Playback (which owns
    /// the cursor), so audio pulls go through it.
    Playback(Arc<Mutex<tango_pvp::playback::Playback>>),
}

impl LinkAccess {
    /// Run `f` against the live link.
    pub fn with_link<R>(&self, f: impl FnOnce(&mut mgba_siolink::Link) -> R) -> Option<R> {
        match self {
            LinkAccess::Handle(h) => Some(h.with_link(f)),
            LinkAccess::Shared(l) => Some(f(&mut l.lock().unwrap())),
            LinkAccess::Playback(p) => Some(f(p.lock().unwrap().pair_mut())),
        }
    }
}

/// Samples retained per metric (~3 s at the GBA tick rate), matching
/// the desktop's window.
pub const HISTORY_LEN: usize = 180;

/// One per-frame snapshot, kept in a ring buffer so each telemetry
/// metric can draw a sparkline.
#[derive(Clone)]
#[allow(dead_code)] // read by the PvP telemetry panel (M4)
pub struct MetricSample {
    pub tps: f32,
    pub fps_target: f32,
    pub skew: i32,
    pub lead: i32,
    pub depth: u32,
    pub rtt_ms: Option<f32>,
}

impl MetricSample {
    pub fn capture(stats: &Stats) -> Self {
        Self {
            tps: stats.tps,
            fps_target: stats.fps_target,
            skew: stats.skew,
            lead: stats.queue_len as i32,
            depth: stats.rolled_back,
            rtt_ms: stats.rtt_ms,
        }
    }
}

// Written by the drivers; read by the session view and telemetry panel.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub queue_len: u32,
    pub skew: i32,
    pub rolled_back: u32,
    pub confirmed: u32,
    pub frontier: u32,
    /// Peak sio run_loop slices a single simulated tick took inside the
    /// newest advance — the lockstep-livelock early-warning.
    pub slices_peak: u32,
    /// Actually achieved simulated ticks per second (measured over a
    /// rolling one-second window), as opposed to `fps_target` (the pace
    /// the throttler is currently asking for).
    pub tps: f32,
    pub fps_target: f32,
    /// PvP: the opponent's ack-derived round-trip time.
    pub rtt_ms: Option<f32>,
}

/// State shared between the driver, the audio pump, and the UI. One
/// instance per session, regardless of kind. On the web build all of it
/// lives on one thread, but the atomics/mutexes are kept — they're free
/// uncontended and the types stay `Send` for the engine's sake.
pub struct SharedSession {
    /// Latest presented frame: mGBA's raw little-endian BGR555,
    /// 240x160x2 bytes.
    pub vbuf: Mutex<Vec<u8>>,
    /// Bumped whenever `vbuf` changes, so the presenter knows to
    /// re-upload.
    pub vbuf_rev: AtomicU64,
    /// The pace the simulation is currently targeting, as f32 bits; the
    /// audio servo keys its faux clock off it. 0.0 = paused/silent.
    pub fps_target: AtomicU32,
    /// The local joypad, written by the runtime pump every tick.
    pub joyflags: AtomicU32,
    /// Which player's screen (and audio) to present. For PvP this is
    /// pinned to the local player.
    pub view_player: AtomicUsize,
    /// PvP: present delay, adjustable live.
    #[allow(dead_code)] // PvP (M4)
    pub present_delay: AtomicU32,
    /// Local: pause flag.
    pub paused: AtomicBool,
    /// Local: resume must also discard the old pacing deadline.
    /// This is separate from `paused` because a short pause can begin and
    /// end between two pumps; in that case the pump never observes
    /// `paused == true` and cannot reset its clock on its own.
    pace_reset_requested: AtomicBool,
    /// Local: speed percent (100 = 1x), for hold-to-fast-forward.
    pub speed: AtomicU32,
    /// UI → driver: end the session.
    pub quit: AtomicBool,
    /// Driver → UI: why the session ended.
    pub end: Mutex<Option<SessionEnd>>,
    pub stats: Mutex<Stats>,
}

impl SharedSession {
    pub fn new(present_delay: u32) -> Arc<SharedSession> {
        Arc::new(SharedSession {
            vbuf: Mutex::new(vec![0; crate::platform::video::SCREEN_BYTES]),
            vbuf_rev: AtomicU64::new(0),
            fps_target: AtomicU32::new(0f32.to_bits()),
            joyflags: AtomicU32::new(0),
            view_player: AtomicUsize::new(0),
            present_delay: AtomicU32::new(present_delay),
            paused: AtomicBool::new(false),
            pace_reset_requested: AtomicBool::new(false),
            speed: AtomicU32::new(100),
            quit: AtomicBool::new(false),
            end: Mutex::new(None),
            stats: Mutex::new(Stats::default()),
        })
    }

    pub fn set_fps_target(&self, fps: f32) {
        self.fps_target.store(fps.to_bits(), Ordering::Relaxed);
    }

    /// Resume a locally paced session without trying to make up time spent
    /// paused. The reset request is published before the pause flag clears,
    /// so a pump that observes the resume also observes the reset.
    #[allow(dead_code)] // pause/resume UI
    pub fn resume(&self) {
        self.pace_reset_requested.store(true, Ordering::Relaxed);
        self.paused.store(false, Ordering::Release);
    }

    pub(crate) fn take_pace_reset(&self) -> bool {
        self.pace_reset_requested.swap(false, Ordering::Relaxed)
    }

    /// Publish the presented core's raw BGR555 frame.
    pub fn publish_video(&self, bgr555: &[u8]) {
        let mut vbuf = self.vbuf.lock().unwrap();
        if vbuf.len() != bgr555.len() {
            vbuf.resize(bgr555.len(), 0);
        }
        vbuf.copy_from_slice(bgr555);
        drop(vbuf);
        self.vbuf_rev.fetch_add(1, Ordering::Release);
    }

    pub fn finish(&self, end: SessionEnd) {
        let mut slot = self.end.lock().unwrap();
        if slot.is_none() {
            *slot = Some(end);
        }
        drop(slot);
        self.set_fps_target(0.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Pvp,
    Local,
    Replay,
}

/// What the session view needs to label things.
pub struct SessionDescriptor {
    pub kind: SessionKind,
    pub local_player: usize,
    /// The registered game this session is running — the boot path
    /// always goes through `library::detect`, so every session knows
    /// its game.
    pub game: GameRef,
}

/// Deepen every core's audio buffer past mgba's 2048 default so servo
/// regulation has room, and drop anything buffered during boot.
pub fn prepare_audio_buffers(link: &mut mgba_siolink::Link) {
    for i in 0..link.num_players() {
        let core = link.core_mut(i);
        core.set_audio_buffer_size(16384);
        core.audio_buffer().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_requests_one_pacer_reset() {
        let shared = SharedSession::new(0);
        shared.paused.store(true, Ordering::Relaxed);

        shared.resume();

        assert!(!shared.paused.load(Ordering::Acquire));
        assert!(shared.take_pace_reset());
        assert!(!shared.take_pace_reset());
    }
}
