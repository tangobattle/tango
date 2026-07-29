//! The engine seam: what a match needs from an emulator, and what a
//! host needs from a match.
//!
//! A Battle Network netplay match is two consoles linked together —
//! but *which* consoles, and what links them, is the engine's
//! business. The GBA games are two GBAs on an emulated link cable
//! (`tango-backend-mgba`); BN5 Double Team DS is two DSes on emulated
//! local wireless (`tango-backend-melonds`). The two engines agree on
//! their whole outline — boot a pair, prime it into a battle, run a
//! rollback session over it, feed local input in and forward the
//! peer's — and disagree on every detail below it.
//!
//! So the seam is one trait: [`Link`], the linked pair itself. An
//! engine hands one over and the shared [`Match`](crate::Match) does
//! the rest — the rollback loop, the confirmed-input record, the
//! audio plumbing. The trait is object-safe on purpose: a `Game`
//! registration cannot name an emulator, so the erasure happens here,
//! at the link, where it costs a virtual call per tick instead of per
//! instruction.

/// A whole-link capture — both consoles and anything in flight between
/// them — as the seam carries it. Opaque: only the link that produced
/// it can read it back, and it downcasts to find out.
pub type Snapshot = Box<dyn std::any::Any + Send>;

/// An emulator's linked pair: two consoles plus whatever connects
/// them, snapshotted and restored as one unit. That is the rollback
/// unit, because the wire between two consoles carries state just as
/// much as the consoles do.
///
/// Everything speaks [`HostInput`](crate::HostInput) — the
/// engine-neutral word hosts, the wire and replays already share — so
/// nothing above a link ever names an emulator's own input type.
pub trait Link: Send + 'static {
    /// Reduce a host's input to what this console could really
    /// produce: keys masked to the pad, a touch clamped to the screen
    /// (or dropped outright by a console with no screen to touch).
    ///
    /// This matters because inputs are simulation state — both peers
    /// must feed their consoles identical values, whatever a host or
    /// the wire handed over. Everything a [`Match`](crate::Match)
    /// applies goes through here first, local and remote alike, and
    /// what the local peer forwards is the sanitized value.
    /// Idempotent by construction: sanitizing twice is sanitizing
    /// once.
    fn sanitize(&self, input: crate::HostInput) -> crate::HostInput;

    /// Advance the pair one video frame.
    fn tick(&mut self, inputs: [crate::HostInput; 2]);

    /// Capture the link. Reuses `recycled`'s allocations when one is
    /// offered — rollback retires a snapshot nearly every tick, and
    /// these run to megabytes. A recycled snapshot is always one this
    /// link produced earlier.
    fn snapshot(&mut self, recycled: Option<Snapshot>) -> Result<Snapshot, crate::Error>;

    /// Resume from a capture. Simulation continues from the frame
    /// *after* the one that had completed when it was taken.
    fn restore(&mut self, snapshot: &Snapshot) -> Result<(), crate::Error>;

    /// How much audio this link has produced and kept, per console.
    ///
    /// Audio is not machine state — no emulator's savestate carries
    /// the buffer a frontend reads from — so a [`restore`](Link::restore)
    /// leaves whatever the speculation voiced sitting there, and the
    /// re-simulation that follows produces the same span a second
    /// time. A link that can take that back reports a mark here and
    /// honours [`revoke_audio`](Link::revoke_audio); the pair is what
    /// a rollback needs to keep sound continuous across a mispredict.
    ///
    /// Defaulted to inert, for a link whose audio nobody plays.
    fn audio_mark(&mut self) -> [u64; 2] {
        [0; 2]
    }

    /// Take back all audio produced since a snapshot recorded `mark`.
    ///
    /// What is still queued is dropped; what a host already took
    /// cannot be unplayed, so its regeneration is swallowed on the way
    /// through instead of queuing as an echo. See
    /// [`audio_mark`](Link::audio_mark).
    fn revoke_audio(&mut self, mark: [u64; 2]) {
        let _ = mark;
    }

    /// One console's current display as RGBA8, in the layout order the
    /// engine's [`Backend::screen_layout`] declares. `None`
    /// before its first frame.
    fn frame(&mut self, player: usize) -> Option<Vec<u8>>;

    /// Turn rasterization on or off for one console.
    ///
    /// A netplay match renders only the local side — skipping the
    /// remote one is most of what makes a rollback tick cheap — but a
    /// host showing both (training's picture-in-picture) turns the
    /// other back on. Frameskip is unserialized and invisible to the
    /// simulation, so this is rollback-safe.
    fn set_render(&mut self, player: usize, on: bool) {
        let _ = (player, on);
    }

    /// The rate one console produces audio at, in Hz.
    fn audio_sample_rate(&mut self, player: usize) -> f64;

    /// How one console's audio production scales when the host paces
    /// the simulation at `fps_target` (see
    /// [`AudioDrain::framerate_ratio`](crate::AudioDrain::framerate_ratio)).
    fn audio_framerate_ratio(&mut self, player: usize, fps_target: f64) -> f64 {
        let _ = (player, fps_target);
        1.0
    }

    /// Take up to `out`'s worth of one console's produced audio, as
    /// interleaved stereo, and report what is left behind. Taking it
    /// consumes it; what stays queued stays revocable.
    fn drain_audio(&mut self, player: usize, out: &mut [i16]) -> crate::Drained;
}

/// One screen a console presents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
}

impl Screen {
    /// Size of this screen in a frame buffer, in bytes (RGBA8).
    pub fn len(&self) -> usize {
        (self.width * self.height * 4) as usize
    }
}

/// The screens a console presents, left to right in the order a
/// [`frame`](Link::frame) buffer lays them out.
///
/// A GBA has one; a DS has two. They are listed rather than described
/// by a count and a shared size because nothing guarantees a console's
/// screens match each other — and a host laying out a pane wants each
/// one's dimensions anyway.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScreenLayout {
    pub screens: Vec<Screen>,
}

impl ScreenLayout {
    pub fn new(screens: impl IntoIterator<Item = Screen>) -> Self {
        ScreenLayout {
            screens: screens.into_iter().collect(),
        }
    }

    /// A single-screen layout, the common case.
    pub fn single(width: u32, height: u32) -> Self {
        ScreenLayout::new([Screen { width, height }])
    }

    /// Size of a whole frame in bytes (RGBA8).
    pub fn buffer_len(&self) -> usize {
        self.screens.iter().map(Screen::len).sum()
    }
}

/// Starts matches for one game on whatever engine that game uses.
///
/// This is the registration seam: a `Game` registration holds one of
/// these, so the registry never names a backend and the app never
/// learns which emulator a game runs on. What comes back from
/// [`start`](Backend::start) is the one concrete
/// [`Match`](crate::Match) — the engine underneath it is erased at the
/// [`Link`].
pub trait Backend: Sync {
    /// How this game's console presents its display, known before a
    /// match exists so a host can lay out its pane.
    fn screen_layout(&self) -> ScreenLayout;

    /// Boot a pair, prime it into a link battle, and start the rollback
    /// session over it.
    fn start(&self, config: StartConfig) -> Result<crate::Match, crate::Error>;

    /// The stats aggregator for a live match on this game — the host
    /// folds confirmed telemetry into it as the match plays, and how
    /// chip use decodes stays the engine's business. Probed off the
    /// patched ROM, because the decoding can depend on the patch
    /// (exe45's community PvP patch). An engine that reports no chip
    /// events leaves the inert default, and the host keeps the rest of
    /// the stats without them.
    fn stats_builder(&self, rom: &[u8]) -> crate::analysis::StatsBuilder {
        let _ = rom;
        crate::analysis::StatsBuilder::default()
    }

    /// Boot one console on its own, for a host that just wants to play
    /// the game. Not every engine offers this — a game supported for
    /// netplay only says so by leaving it alone.
    fn start_solo(&self, config: crate::SoloConfig) -> Result<Box<dyn crate::RunningSolo>, crate::Error> {
        let _ = config;
        Err(crate::Error::Unsupported("this game has no single-player support"))
    }

    /// Re-simulate a recorded match from its inputs. Cheap: the two
    /// simulations a [`ReplaySet`](crate::ReplaySet) offers boot when
    /// the host asks for them.
    fn open_replay(&self, config: crate::ReplayConfig) -> Result<Box<dyn crate::ReplaySet>, crate::Error> {
        let _ = config;
        Err(crate::Error::Unsupported("this game has no replay support"))
    }
}

/// Everything a match needs to come up, in terms every engine shares.
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
    /// The peer's cartridge (see [`PeerRom`](crate::PeerRom)), which
    /// the local game's crate resolves against its siblings when the
    /// engine needs per-cartridge support for that seat.
    pub peer_rom: crate::PeerRom,
    /// Which console this peer drives.
    pub local_player: usize,
    /// How many ticks behind the frontier to present. Purely local.
    pub present_delay: u32,
    /// Silence battle BGM. Purely local presentation.
    pub disable_bgm: bool,
}
