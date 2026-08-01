//! The mgba engine: a pair of emulated GBAs on an emulated link cable.
//!
//! Everything here used to live in `tango-match`, which left that crate
//! — the engine-neutral seam every game speaks — unable to build without
//! an emulator. A DS game that pulls in `tango-match` should not compile
//! mgba, so the engine moved out to sit beside `tango-match-melonds`.
//!
//! The pieces:
//!
//! - [`link`]: the [`tango_match::Link`] implementation — the pair as
//!   the seam's rollback unit, with audio revocation and telemetry
//!   riding inside.
//! - [`solo`]: one GBA booted alone, as the seam's
//!   [`Console`](tango_match::Console).
//! - [`backend`]: the [`tango_match::Backend`] a game registration
//!   holds — netplay, solo, and replay playback all come out of it,
//!   along with the boot every simulation of a match starts with:
//!   prime the pair and start the seam's rollback
//!   [`Match`](tango_match::Match).
//! - [`analysis`]: the per-tick RAM-poll telemetry as this engine
//!   drives it, the match-stats types, and the fold between them.
//!
//! [`link`]: mod@link

pub mod analysis;
pub mod backend;
pub mod link;
pub mod solo;

pub use link::{Link, JOYFLAGS_MASK};
pub use solo::SoloConsole;

/// Simulation failure, as this engine reports it. Converts into the
/// seam's [`tango_match::Error`], which cannot name mgba's error type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Mgba(#[from] mgba::Error),

    /// Priming never reached a link battle within the tick bound — a
    /// wedged menu walk, or the wrong ROM/save for the primer traps.
    #[error("priming did not reach a link battle within {0} ticks")]
    PrimeTimeout(u32),

    /// The caller's cancel flag flipped mid-simulation.
    #[error("cancelled")]
    Cancelled,
}

impl From<Error> for tango_match::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::PrimeTimeout(ticks) => tango_match::Error::PrimeTimeout(ticks),
            Error::Cancelled => tango_match::Error::Cancelled,
            Error::Mgba(e) => tango_match::Error::Backend(Box::new(e)),
        }
    }
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
    /// lifecycle anchors reporting into `events` — the game's
    /// battle-start-complete site firing
    /// [`round_started`](tango_match::telemetry::EventSink::round_started),
    /// its result-deciding sites firing
    /// [`round_outcome`](tango_match::telemetry::EventSink::round_outcome),
    /// and its match-end site firing
    /// [`match_ended`](tango_match::telemetry::EventSink::match_ended). The
    /// priming pokes must be pure functions of emulation state and
    /// `config`, so both peers' pairs prime bit-identically, and must go
    /// inert once the battle is live (the traps stay installed for the
    /// pair's life). Sink firings are host-side signals only — they
    /// never touch core state, so they can't perturb the simulation.
    fn primer_traps(
        &self,
        config: &PrimeConfig,
        player: usize,
        events: &tango_match::telemetry::EventSink,
        primed: &PrimedLatch,
    ) -> Vec<(u32, Box<dyn Fn(&mut mgba::core::Core)>)>;

    /// The telemetry reader for one core running this game. `player` is
    /// which pair core (and player) this poller answers for — it polls
    /// the tick's levels and reports its own player's chip uses into
    /// the sink as it catches them firing (see
    /// [`CorePoller`](tango_match::telemetry::CorePoller)).
    fn core_poller(&self, player: usize) -> Box<dyn tango_match::telemetry::CorePoller<mgba::core::Core>>;
}

pub use backend::{GbaBackend, Seat};

/// Route mgba's own logging through the `log` crate.
///
/// Called wherever this backend brings a console up rather than once at
/// startup: nothing outside this crate should have to know which
/// emulator a game runs on, so there is no host-side hook left to call
/// it from. Installing it repeatedly is fine, which is what makes that
/// work.
///
/// Without it a core falls through to the emulator's printf stub and
/// writes `GBA BIOS: SWI: …` straight to stdout.
pub fn install_logger() {
    mgba::log::install_default_logger();
}
