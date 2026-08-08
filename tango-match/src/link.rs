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
//! So the seam is [`Link`], the linked pair itself, and [`Side`], one
//! console of it. An engine hands a link over and the shared
//! [`Match`](crate::Match) does the rest — the rollback loop, the
//! confirmed-input record. Pair-level operations (ticking,
//! snapshotting) live on the link; everything one console answers for
//! itself (display, audio out, savedata) lives on its side, which is
//! also how a console booted with no pair at all
//! ([`Console`](crate::Console)) presents itself. The traits are
//! object-safe on purpose: a `Game` registration cannot name an
//! emulator, so the erasure happens here, at the link, where it costs a
//! virtual call per tick instead of per instruction.
//!
//! A side is borrowed for one call chain and reached only from the
//! thread turning the simulation's crank — a host never holds one. What
//! a host plays is [`audio`](crate::audio)'s ring, which that thread
//! pushes each console's production into on its way past.

/// A whole-link capture — both consoles and anything in flight between
/// them — as the seam carries it. Opaque: only the link that produced
/// it can read it back, and it downcasts to find out. `Sync` because a
/// capture is inert data — threads only ever read one — and the replay
/// machinery shares its keyframes across the host's workers.
pub type Snapshot = Box<dyn std::any::Any + Send + Sync>;

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

    /// One console's per-side surface: display, audio out, savedata.
    ///
    /// Boxed because the trait must stay object-safe; the box lives
    /// for one call chain, so the cost is a small allocation, not a
    /// design constraint.
    fn side(&mut self, player: usize) -> Box<dyn Side + '_>;
}

/// One console of a boot, as the seam reads it: everything a console
/// answers for itself, with no player index in sight.
///
/// [`Link::side`] hands one out per seat and
/// [`Console::side`](crate::Console) for a machine booted alone, so
/// everything above — a host's audio, the solo ride, a host's frame
/// pump — reads any console through this one surface.
///
/// Borrowed, not owned: a side is a view into wherever the engine
/// keeps the console, alive for one call chain.
pub trait Side {
    /// The console's current display as RGBA8, in the layout order the
    /// engine's [`Backend::screen_layout`] declares. `None`
    /// before its first frame.
    fn frame(&mut self) -> Option<Vec<u8>>;

    /// Turn rasterization on or off for this console.
    ///
    /// A netplay match renders only the local side — skipping the
    /// remote one is most of what makes a rollback tick cheap — but a
    /// host showing both (training's picture-in-picture) turns the
    /// other back on. Frameskip is unserialized and invisible to the
    /// simulation, so this is rollback-safe.
    fn set_render(&mut self, on: bool) {
        let _ = on;
    }

    /// Which of this console's screens the host will actually put in
    /// front of someone, as a bitmask over the order
    /// [`Backend::screen_layout`] declares.
    ///
    /// A console may compose a screen nobody is shown: a cart can spend a
    /// whole mode on one of them, and a host can be arranged to present
    /// one of the two. Whatever draws the screen that is dropped is doing
    /// work for nobody, and on the DS that is a whole 2D engine.
    ///
    /// Like [`Side::set_render`] this is presentation, not simulation:
    /// what the skipped engine would have drawn is read by nothing the
    /// match depends on, so it is rollback-safe and two peers arranged
    /// differently still play the same match.
    fn set_displayed_screens(&mut self, screens: u8) {
        let _ = screens;
    }

    /// The cartridge's savedata as it stands, or `None` for a game
    /// that has never written any.
    fn export_save(&mut self) -> Option<Vec<u8>> {
        None
    }

    /// The rate this console produces audio at, in Hz.
    fn audio_sample_rate(&mut self) -> f64;

    /// Take up to `out`'s worth of this console's produced audio, as
    /// interleaved stereo, and answer with how much it had in total —
    /// frames, counting both what went into `out` and what would not
    /// fit. Taking it consumes it.
    ///
    /// One number, because the split is the caller's own arithmetic: a
    /// drain always fills `out` as far as it goes, so the frames that
    /// landed are `min(total, out.len() / 2)` and the rest is still in
    /// the console. What a caller cannot derive is the total — which is
    /// how a caller knows to come back for the rest — so that is what
    /// comes back.
    ///
    /// Called from the thread that ticks the console, right after the
    /// tick: a session's [`Pump`](crate::audio) empties it into the ring
    /// a host plays, so nothing a console keeps ever has to survive
    /// until a sound callback comes asking.
    fn drain_audio(&mut self, out: &mut [i16]) -> usize;
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
///
/// A session may compose fewer screens than its console has — see
/// [`Backend::screen_layout`] — so this describes what the frames
/// actually carry, not what the hardware owns.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScreenLayout {
    pub screens: Vec<Screen>,
    /// Index into [`screens`](Self::screens) of the one a stylus points
    /// at. `None` both for a console with no touch screen and for a
    /// session that composes without it — a host has nothing to point
    /// at either way, which is the only question it asks. Positional
    /// guessing is not an option: a selection can put the touch screen
    /// anywhere in the layout, or leave it out.
    pub touch: Option<usize>,
}

impl ScreenLayout {
    /// A layout with no touch screen in it. Composers that present one
    /// say so with [`with_touch`](Self::with_touch).
    pub fn new(screens: impl IntoIterator<Item = Screen>) -> Self {
        ScreenLayout {
            screens: screens.into_iter().collect(),
            touch: None,
        }
    }

    /// Mark the screen at `index` as the stylus target. Panics on an
    /// index this layout has no screen for: a console that composes a
    /// touch screen it isn't presenting is a bug in the composer, and
    /// one that reaches a host silently misplaces every stylus press.
    pub fn with_touch(mut self, index: usize) -> Self {
        assert!(index < self.screens.len(), "touch screen is not in this layout");
        self.touch = Some(index);
        self
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

/// Which session a console's shape is being asked for, described
/// enough to answer.
///
/// A cart can use fewer of its console's screens in a link battle than
/// it does played alone — EXE OSS runs its whole netbattle on the
/// upper screen — and fewer in one match mode than another: BN5DS
/// carries its touch screen for Team Battle and not for the plain
/// subtypes. So the layout is a question about the session, and the
/// match type is part of what a session is; carrying it here is what
/// makes the question unaskable without the answer's input.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionMode {
    /// A primed pair: live netplay, its replay, and training against
    /// it — everything [`Backend::start`] and
    /// [`Backend::open_replay`] produce. `match_type` is the mode the
    /// pair was primed into, indexed as the registration's
    /// `match_types` table lists it (the same pair
    /// [`StartConfig::match_type`] carries).
    PvP { match_type: (u8, u8) },
    /// One console on its own, as [`Backend::start_solo`] boots it.
    /// No match type: a cart played alone is in no mode.
    Solo,
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
    /// How this build simulates this game, as one number: bumping it
    /// says "a match here no longer runs the way it used to". Both
    /// consumers of that fact use this same value.
    ///
    /// - Replays. Recorders stamp it into each side's metadata, and
    ///   playback requires the recorded value to equal the current one
    ///   — so a bump invalidates this game's existing recordings and
    ///   nobody else's.
    /// - Netplay. Peers announce it beside the game they've picked, and
    ///   a pairing whose two values differ is refused before either
    ///   side commits — the two builds would simulate the same inputs
    ///   into different matches.
    ///
    /// Bump it when anything under this backend changes in a way that
    /// makes the same inputs produce a different match (input mapping,
    /// priming, trap layout, the emulated timeline underneath it).
    /// Because the number comes off the backend a registration holds,
    /// that costs the games that changed and no others — the wire's own
    /// `PROTOCOL_VERSION` is for changes to the netplay protocol
    /// itself, and container-wide replay layout changes still belong to
    /// `tango_replay::VERSION`.
    ///
    /// Two independent things can move a match, though, and one backend
    /// serves every game on its emulator: so both engines pack their
    /// own version into the high 16 bits and the game's into the low
    /// 16. An emulator change bumps the engine half once and re-cuts
    /// every game it runs; a game's own priming or traps bump only that
    /// game's half. Nobody outside reads the halves apart — to its two
    /// consumers this is one opaque number, compared for equality (and,
    /// in the lobby, for which of the two peers is behind).
    fn sim_version(&self) -> u32;

    /// How this game's console presents its display in `mode`, known
    /// before a session exists so a host can lay out its pane.
    ///
    /// Per-mode because a cart may compose fewer screens in a link
    /// battle than it does played alone, or in one match mode than
    /// another: a DS game whose netbattle lives entirely on the upper
    /// screen presents that one, and a pane, an export and a stylus
    /// area all follow from the layout rather than each re-deriving
    /// the rule. Whatever comes back must
    /// match what that mode's frames actually carry — a session sizes
    /// its framebuffer from this, and a frame of the wrong size is
    /// dropped rather than drawn.
    fn screen_layout(&self, mode: SessionMode) -> ScreenLayout;

    /// The buttons this game's console has, as a mask over the
    /// [`keys`](crate::keys) bits a host sets in
    /// [`HostInput::keys`](crate::HostInput). A GBA's pad is ten; the
    /// DS adds X and Y.
    ///
    /// A host draws its input displays from this. The screen count used
    /// to stand in for the question — two screens meant a DS — and
    /// can't any more, now that a session composes whichever screens
    /// its mode uses.
    fn keys_mask(&self) -> u32;

    /// The console's native frame clock — known, like the layout,
    /// before a match exists: hosts pace their drive loops and size
    /// their audio streams around its [`fps`](FrameTiming::fps), and a
    /// video encoder timestamps frames by the exact rational. The GBA's
    /// rate is not a round 60, and the DS's differs again, so nothing
    /// above an engine may hardcode one.
    fn frame_timing(&self) -> FrameTiming;

    /// A session that will run `consoles` of this game's console is
    /// being put together; get whatever per-console machinery ready
    /// that benefits from a head start. Fired from the session's
    /// construction — which is async and yields — where the boot
    /// itself is synchronous and doesn't.
    ///
    /// The default does nothing, which is right for every native
    /// engine. The DS engine's browser build spawns its per-console
    /// worker threads here: a Web Worker only finishes starting once
    /// the browser's main thread has had event-loop turns, and by boot
    /// time there are none to give.
    fn prepare(&self, consoles: u32) {
        let _ = consoles;
    }

    /// Whether what [`prepare`](Self::prepare) started is done — a
    /// host's boot loop holds off (and keeps yielding) until it is.
    /// Meaningful only where prepare does something: the DS engine's
    /// browser build answers for its worker threads; everywhere else
    /// the default `true` boots on the first ask.
    fn ready(&self, consoles: u32) -> bool {
        let _ = consoles;
        true
    }

    /// Boot a pair, prime it into a link battle, and start the rollback
    /// session over it.
    fn start(&self, config: StartConfig) -> Result<crate::Match, crate::Error>;

    /// Boot one console on its own, for a host that just wants to play
    /// the game. Not every engine offers this — a game supported for
    /// netplay only says so by leaving it alone.
    fn start_solo(&self, config: crate::SoloConfig) -> Result<crate::Solo, crate::Error> {
        let _ = config;
        Err(crate::Error::Unsupported("this game has no single-player support"))
    }

    /// Re-simulate a recorded match from its inputs. Cheap: the two
    /// simulations a [`ReplaySet`](crate::ReplaySet) offers boot when
    /// the host asks for them. An engine implements this by handing
    /// [`ReplaySet::new`](crate::ReplaySet::new) its
    /// [`ReplayBoot`](crate::ReplayBoot) — the machinery above the boot
    /// is the seam's.
    fn open_replay(&self, config: crate::ReplayConfig) -> Result<crate::ReplaySet, crate::Error> {
        let _ = config;
        Err(crate::Error::Unsupported("this game has no replay support"))
    }
}

/// One video frame's length on the console's own clock, as an exact
/// rational: a frame lasts `frame_duration / timescale` seconds.
///
/// A rational rather than a rate, because a video encoder timestamps
/// frames in integer clock ticks — a rate rounded through a float
/// accumulates drift over a long recording, where the console's own
/// cycle counts don't. Everything that wants the rate takes
/// [`fps`](FrameTiming::fps) of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameTiming {
    /// Ticks of the console's clock per second.
    pub timescale: u32,
    /// Clock ticks one video frame lasts.
    pub frame_duration: u64,
}

impl FrameTiming {
    /// The frame rate the rational reduces to — what hosts pace their
    /// drive loops and size their audio streams around.
    pub fn fps(&self) -> f64 {
        self.timescale as f64 / self.frame_duration as f64
    }
}

/// A cartridge as its own ROM header names it.
///
/// A match can span two variants and two regions — Gregar against
/// Falzar, a Japanese cart against an American one — and an engine may
/// need per-cartridge support for each seat. But a backend hangs off
/// the *local* game, so the peer arrives as an identity the game's own
/// crate resolves against its siblings, which is the only place that
/// knows what they are. That keeps the seam free of registry types and
/// needs no downcasting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PeerRom {
    /// 4-byte ROM code, e.g. `b"BR5E"`.
    pub code: [u8; 4],
    /// Mask-ROM revision.
    pub revision: u8,
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
    /// Where the pair's sound goes — the producing end of a
    /// [`channel`](crate::audio::channel) whose other end the host
    /// plays. The simulation pushes into it every tick and takes back
    /// what a rollback revokes, so a host's device callback never
    /// reaches past the lock this pair ticks under. `None` for a caller
    /// with nobody listening (the offline analysis passes, the probe
    /// harnesses).
    pub audio: Option<crate::AudioIn>,
    /// Abandon the priming walk if this flips, failing the start with
    /// [`Error::Cancelled`]. The walk is seconds of blocking emulation
    /// on a DS-class game and a host runs it under a session the user
    /// can already leave — without this, leaving waits out the whole
    /// walk. `None` for a caller with nothing to cancel from (the
    /// offline analysis passes, the probe harnesses).
    pub cancel: Option<&'a std::sync::atomic::AtomicBool>,
    /// Training only: the control a per-game
    /// [`Trainer`](crate::trainer::Trainer) should honor, if this
    /// game's engine support offers one. `None` everywhere else — a
    /// trainer writes game memory from live host-mutable state, which
    /// is only sound on a pair that never rolls back (see
    /// [`trainer`](crate::trainer)). Netplay and replay must never set
    /// this: a rollback would re-apply writes from current control
    /// state and desync the re-simulation from its first pass.
    pub trainer: Option<std::sync::Arc<crate::trainer::TrainerControl>>,
}
