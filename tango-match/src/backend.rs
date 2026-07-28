//! The engine seam: what a match needs from an emulator, and what a
//! host needs from a match.
//!
//! Everything else in this crate is built on mgba — two GBAs on an
//! emulated link cable. That is not the only shape a Battle Network
//! netplay match comes in: BN5 Double Team DS is two emulated DSes on
//! emulated local wireless, driven by `melonds-rollback`. The two
//! engines agree on their whole outline — boot a pair, prime it into a
//! battle, run a rollback session over it, feed local input in and
//! forward the peer's — and disagree on every detail below it.
//!
//! So the seam comes in two layers:
//!
//! * [`Backend`] is generic. A backend names its own link, snapshot and
//!   input types, and per-game support is written against *those*
//!   rather than against a specific emulator's core type. Nothing is
//!   boxed and nothing is dynamically dispatched on the hot path.
//!
//! * [`MatchFactory`] and [`RunningMatch`] are object-safe. A
//!   [`Game`](tango_gamesupport::Game) registration has to hold one
//!   concrete type, and it cannot name a backend — so the erasure
//!   happens here, one level above the generics, where it costs a
//!   virtual call per frame instead of per instruction.

/// An emulator this crate can run a two-player match on.
///
/// A backend owns a *link*: the pair of consoles plus whatever connects
/// them, snapshotted and restored as one unit. That is the rollback
/// unit, because the wire between two consoles carries state just as
/// much as the consoles do.
pub trait Backend: 'static {
    /// The linked pair.
    type Link;
    /// A whole-link capture — both consoles and anything in flight
    /// between them.
    type Snapshot;
    /// One console's input for one tick.
    type Input: Copy + Eq + Send;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Advance the pair one video frame.
    fn tick(link: &mut Self::Link, inputs: [Self::Input; 2]);

    /// Capture the link. Reuses `recycled`'s allocations when one is
    /// offered — rollback retires a snapshot nearly every tick, and
    /// these run to megabytes.
    fn snapshot(link: &mut Self::Link, recycled: Option<Self::Snapshot>) -> Result<Self::Snapshot, Self::Error>;

    /// Resume from a capture. Simulation continues from the frame
    /// *after* the one that had completed when it was taken.
    fn restore(link: &mut Self::Link, snapshot: &Self::Snapshot) -> Result<(), Self::Error>;

    /// One console's current display as RGBA8, in [`screen_layout`]
    /// order. `None` before its first frame.
    ///
    /// [`screen_layout`]: Backend::screen_layout
    fn frame(link: &mut Self::Link, player: usize) -> Option<Vec<u8>>;

    /// How this console presents its display.
    fn screen_layout() -> ScreenLayout;
}

/// What a console's [`frame`](Backend::frame) buffer contains. A GBA
/// hands back one screen; a DS hands back two, stacked top then bottom,
/// so a host can size a texture without knowing which emulator produced
/// it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenLayout {
    pub width: u32,
    pub height: u32,
    /// Screens stacked vertically in the buffer.
    pub screens: u32,
}

impl ScreenLayout {
    /// Total pixel height of the buffer.
    pub fn buffer_height(&self) -> u32 {
        self.height * self.screens
    }

    /// Size of one frame in bytes (RGBA8).
    pub fn buffer_len(&self) -> usize {
        (self.width * self.buffer_height() * 4) as usize
    }
}

/// Starts matches for one game on whatever engine that game uses.
///
/// This is the object-safe half of the seam: a `Game` registration
/// holds one of these, so the registry never names a backend and the
/// app never learns which emulator a game runs on.
pub trait MatchFactory: Sync {
    /// How this game's console presents its display, known before a
    /// match exists so a host can lay out its pane.
    fn screen_layout(&self) -> ScreenLayout;

    /// Boot a pair, prime it into a link battle, and start the rollback
    /// session over it.
    fn start(&self, config: StartConfig) -> Result<Box<dyn RunningMatch>, crate::Error>;
}

/// Everything a match needs to come up, in terms every backend shares.
/// Both peers pass identical values except
/// [`local_player`](Self::local_player).
pub struct StartConfig<'a> {
    /// Per-console ROM images, already patched. `roms[i]` runs on
    /// console `i`; a single-cart game passes the same image twice.
    pub roms: [&'a [u8]; 2],
    /// Per-console save memory.
    pub saves: [Option<&'a [u8]>; 2],
    /// The negotiated match seed, for whatever reseeding the game's
    /// priming does.
    pub rng_seed: [u8; 16],
    /// The negotiated match clock, pinned into both consoles.
    pub rtc: std::time::SystemTime,
    /// The game's mode selection (type and subtype).
    pub match_type: (u8, u8),
    /// Which console this peer drives.
    pub local_player: usize,
    /// How many ticks behind the frontier to present. Purely local.
    pub present_delay: u32,
    /// Silence battle BGM. Purely local presentation.
    pub disable_bgm: bool,
}

/// A running match, as a host drives it — one virtual call per frame,
/// with the backend's generics resolved underneath.
pub trait RunningMatch: Send {
    /// Advance one frame with the local console's input: settle what
    /// the peer's arrivals confirm, rolling back on a misprediction,
    /// then speculate to the present target. Returns the tick and input
    /// to forward to the peer.
    fn advance(&mut self, local_keys: u32) -> Result<(u32, u32, i16), crate::Error>;

    /// Feed one remote input packet, in tick order.
    fn add_remote_input(&mut self, keys: u32, tick_advantage: i16);

    /// The local console's display, RGBA8 in [`screen_layout`] order.
    ///
    /// [`screen_layout`]: MatchFactory::screen_layout
    fn frame(&mut self) -> Option<Vec<u8>>;

    /// Clock-sync skew for the host's throttler.
    fn skew(&self) -> i32;

    /// Ticks the next [`advance`](Self::advance) could settle from
    /// input already buffered — nonzero means advancing drains the
    /// queue rather than only growing it.
    fn matchable(&self) -> usize;

    fn local_player(&self) -> usize;
}
