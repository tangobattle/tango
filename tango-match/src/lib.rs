//! The PvP engine shared by all Battle Network games this project
//! supports: both games run locally as an [`mgba_rollback::Link`] (a pair
//! of cores — every game tango supports is a two-player link battle)
//! through mgba's lockstep SIO driver, and the pair is the rollback
//! unit (see [`mgba_rollback::session::Session`]). The games run their
//! *real* link protocol over the emulated cable — no handshake skips,
//! no packet munging, no shadow co-sim — so the only game-specific code
//! is data-side: priming a freshly booted game to its link battle, and
//! reading battle telemetry out of RAM every simulated tick. Per-game
//! support implements [`GameSupport`] in the `tango-gamesupport-<game>`
//! crates.
//!
//! The toplevel pieces:
//!
//! - [`engine`]: [`engine::Match`] boots and primes the pair and runs
//!   the rollback session for live netplay.
//!
//! - [`playback`]: linear re-simulation of recorded matches for the
//!   replay viewer, with snapshot seeking.
//!
//! - [`telemetry`]: per-tick RAM-poll telemetry with rollback
//!   revocation, fed by the per-game pollers and lifecycle traps.
//!
//! - [`analysis`]: match-stats types and the telemetry fold, shared by
//!   the live session and offline replay re-analysis.
//!
//! - [`replay`]: the replay file format ([`replay::VERSION`] — one
//!   continuous run of confirmed pair-tick input pairs).
//!
//! - [`battle`]: the per-tick stats sample encoding.
//!
//! - [`input`]: the joyflags input type that lands in replays.

pub mod analysis;
pub mod battle;
pub mod engine;
pub mod input;
pub mod playback;
pub mod replay;
pub mod telemetry;

/// Simulation failure, shared by the live engine, replay playback, and
/// offline analysis — all three boot and drive the same kind of pair.
/// (The stats sidecar codec has its own parse error,
/// [`analysis::ReadError`].)
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Mgba(#[from] mgba::Error),
    /// Priming never reached a link battle within the tick bound — a
    /// wedged menu walk (or the wrong ROM/save for the primer traps).
    #[error("priming did not reach a link battle within {0} ticks")]
    PrimeTimeout(u32),
    /// The caller's cancel flag flipped mid-simulation.
    #[error("cancelled")]
    Cancelled,
}

/// A PC-sited trap: fires the closure when emulation reaches the ROM
/// address (see `mgba_rollback::Link::set_traps`).
pub type Trap = (u32, Box<dyn Fn(&mut mgba::core::Core)>);

/// One core's "the battle has started" latch — the priming handoff.
/// Each core's battle-start trap (the game's own battle-start-complete
/// code path, the trap engine's match-start hook) sets it; the engine's
/// priming loop runs until both cores' latches are set, at which point
/// the games accept input and the session takes over. Latching is a
/// host-side signal only — core state is untouched.
#[derive(Clone, Default)]
pub struct PrimedLatch(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl PrimedLatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Trap-side: this core's battle-start routine completed.
    pub fn set(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// The engine's clock-sync governor, re-exported so hosts driving a
/// [`Match`](engine::Match) don't need their own mgba-rollback
/// dependency: feed it `skew()` + `speculation_balance()` each frame and
/// shave the returned fps off the tick rate.
pub use mgba_rollback::throttler::Throttler;

/// The linked core pair, re-exported for hosts that reach through
/// [`Match::with_pair`](engine::Match::with_pair) for video/audio
/// readout.
pub use mgba_rollback::Link;

/// Cross-thread readout handle to a running match's pair (see
/// [`Match::pair_handle`](engine::Match::pair_handle)).
pub use mgba_rollback::session::LinkHandle;

/// Per-tick observer hook, re-exported for hosts that step a
/// [`playback::Playback`] themselves and feed each tick to a
/// [`telemetry::Telemetry`] observer (e.g. tango's replay video export).
pub use mgba_rollback::session::TickObserver;

/// Match parameters the primer needs before the games can negotiate the
/// rest themselves over the emulated cable.
pub struct PrimeConfig {
    /// The game's link-battle mode selection (same encoding as
    /// `battle::MatchType`: type and subtype).
    pub match_type: (u8, u8),
    /// The negotiated match seed. Both cores boot bit-identically, so
    /// without reseeding the two games' RNGs hold the same state and
    /// both players get identical draws; the primer traps seed each
    /// core's RNGs with values derived from this and the core index
    /// (identical on both peers, distinct between the two cores).
    pub rng_seed: [u8; 16],
    /// Silence the games' battle BGM: each game's primer installs a trap
    /// that skips the battle-start music call (on both cores of this
    /// pair). Purely local presentation — the sound driver's state never
    /// feeds battle logic, so peers are free to disagree and replays
    /// don't record it (trap-era semantics: a local setting, never
    /// negotiated).
    pub disable_bgm: bool,
}

impl PrimeConfig {
    /// A per-core, per-stream 32-bit seed derived from the match seed —
    /// stream `n` of this core's game RNGs. Never zero (some generators
    /// stick at a zero state).
    pub fn core_rng_seed(&self, player: usize, stream: usize) -> u32 {
        let i = (player * 2 + stream) * 4 % self.rng_seed.len();
        let v = u32::from_le_bytes(self.rng_seed[i..i + 4].try_into().unwrap());
        // Perturb by lane so identical seed words still land distinct
        // streams, and keep it nonzero.
        let v = v ^ (0x9e37_79b9u32.wrapping_mul((player as u32) * 2 + stream as u32 + 1));
        if v == 0 {
            1
        } else {
            v
        }
    }
}

/// Per-ROM-variant support for the PvP engine, implemented in the
/// gamesupport crates. Everything here is data-side: no packet munging,
/// no handshake skips — the games run their real link protocol over the
/// emulated cable (which is why priming must NOT jump the comm-menu
/// dispatcher's states: the bring-up states are where the real handshake
/// happens; skipping them yields the games' "communication failed" path).
///
/// Priming is entirely memory munging — the pair's joypads stay idle
/// throughout and no input state of any kind is synthesized. The traps
/// walk boot → comm menu → battle with control-state pokes at known
/// menu-code anchors, letting the games' own link exchanges run for
/// real over the emulated cable wherever the flow depends on them. The
/// menus are poked into existence with no other input path, so every
/// cursor is at its deterministic init position and no wrong option
/// can ever be selected. Priming ends when the games' own battle-start
/// code fires on both cores (`primed` — the trap engine's match-start
/// hook), which is where the games begin accepting input.
pub trait GameSupport: Sync {
    /// PC-sited traps for one core running this game: the priming walk
    /// (boot → the comm menu → the link battle; `player` is which pair
    /// core this is, 0 = lockstep primary) plus, for core 0, the round
    /// lifecycle anchors reporting into `lifecycle` — the game's
    /// battle-start-complete site firing
    /// [`round_started`](telemetry::LifecycleSink::round_started) and
    /// its match-end site firing
    /// [`match_ended`](telemetry::LifecycleSink::match_ended). The
    /// priming pokes must be pure functions of emulation state and
    /// `config`, so both peers' pairs prime bit-identically, and must go
    /// inert once the battle is live (the traps stay installed for the
    /// pair's life). Lifecycle firings are host-side signals only — they
    /// never touch core state, so they can't perturb the simulation.
    fn primer_traps(
        &self,
        config: &PrimeConfig,
        player: usize,
        lifecycle: &telemetry::LifecycleSink,
        primed: &PrimedLatch,
    ) -> Vec<(u32, Box<dyn Fn(&mut mgba::core::Core)>)>;

    /// The telemetry reader for one core running this game. `player` is
    /// which pair core (and player) this poller answers for.
    fn core_poller(&self, player: usize) -> Box<dyn telemetry::CorePoller>;

    /// How this game's per-tick chip reports are to be decoded into
    /// chip-use events — see [`ChipSemantics`]'s variants for the two
    /// reporting contracts. Takes the (patched) ROM because the
    /// contract can depend on the applied patch: exe45's community PvP
    /// patch replaces the dealt-queue system with per-screen hands,
    /// flipping it from `QueueSum` to `LoadedChip`.
    ///
    /// [`ChipSemantics`]: crate::analysis::ChipSemantics
    fn chip_semantics(&self, rom: &[u8]) -> crate::analysis::ChipSemantics {
        let _ = rom;
        crate::analysis::ChipSemantics::LoadedChip
    }

    /// Whether B presses are buster shots in this game on this (patched)
    /// ROM. Vanilla exe45's navi fights autonomously — B is a menu key
    /// there, so its edges aren't buster events; the PvP patch's manual
    /// control makes them real again.
    fn counts_buster(&self, rom: &[u8]) -> bool {
        let _ = rom;
        true
    }
}
