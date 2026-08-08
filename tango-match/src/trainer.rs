//! Training-mode control over a live pair: the write-side seam.
//!
//! Telemetry's [`CorePoller`](crate::telemetry::CorePoller) reads a
//! game's battle RAM every tick and is contractually forbidden to write
//! it — netplay and replay share those pollers, and a write would
//! desync a rollback re-simulation from its first pass. Training has no
//! such constraint: both seats' inputs are supplied locally before every
//! tick advances, so the pair runs lockstep and nothing is ever
//! re-simulated. That is the one place a live-mutable poke is sound,
//! and this module is that poke's seam.
//!
//! The shape mirrors telemetry's: an engine-neutral shared control
//! ([`TrainerControl`]) the host writes and the engine reads, plus a
//! per-game hook ([`Trainer`]) the engine drives once per core per
//! tick. A game's support crate implements [`Trainer`] against its own
//! offsets; the backend installs it only when
//! [`StartConfig::trainer`](crate::StartConfig::trainer) is set, which
//! only a training session ever sets.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Host-set training inputs, shared between the UI (which writes them),
/// the session (which holds the [`Arc`]), and the engine's per-tick
/// [`Trainer`] (which reads them under the pair lock).
///
/// Lockstep-only by contract: honoring these mutates game memory from
/// state the host can change at any moment, so a pair honoring them
/// must never roll back — a re-simulation would re-apply writes from
/// *current* control state, not the state the first pass saw.
#[derive(Default)]
pub struct TrainerControl {
    /// Forced hand per absolute player: up to 6 chip ids, in fire
    /// order. While set, the game's own pick is overwritten with this
    /// list every time that player's custom screen closes — cleared
    /// (`None`), the game's picks stand.
    forced_hands: Mutex<[Option<Vec<u16>>; 2]>,
    /// Set by the backend iff a per-game [`Trainer`] was installed —
    /// how a host knows chip forcing exists for this game at all.
    wired: AtomicBool,
}

impl TrainerControl {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Set or clear `player`'s forced hand. Takes effect at that
    /// player's next custom-screen close.
    pub fn set_forced_hand(&self, player: usize, hand: Option<Vec<u16>>) {
        self.forced_hands.lock().unwrap()[player & 1] = hand;
    }

    /// `player`'s forced hand as it stands, cloned out.
    pub fn forced_hand(&self, player: usize) -> Option<Vec<u16>> {
        self.forced_hands.lock().unwrap()[player & 1].clone()
    }

    /// Backend-side: a per-game [`Trainer`] was installed over this
    /// control.
    pub fn set_wired(&self) {
        self.wired.store(true, Ordering::Relaxed);
    }

    /// Whether a per-game [`Trainer`] honors this control — `false`
    /// for a game whose support offers none, which a host reads as
    /// "chip forcing isn't available here".
    pub fn is_wired(&self) -> bool {
        self.wired.load(Ordering::Relaxed)
    }
}

/// The per-game write-side hook: the engine calls it once per core per
/// tick, right after the pair ticks and *before* the telemetry poll —
/// so the pollers read post-write state and never see a hand the game
/// picked but the trainer replaced.
///
/// Implementations live in the gamesupport crates, generic over the
/// engine's core type like [`CorePoller`](crate::telemetry::CorePoller).
/// Unlike a poller, a trainer MAY write game memory — which is exactly
/// why it exists only on the training path (see [`TrainerControl`]).
/// Whatever it writes must land identically on both cores: the pair is
/// one simulation, and the two games' copies of shared battle state
/// must keep agreeing. Detecting the same edge on each core's own tick
/// call is the usual way (lockstep mirrors see identical state).
pub trait Trainer<Core>: Send {
    fn tick(&mut self, core: &mut Core, core_index: usize, control: &TrainerControl);
}
