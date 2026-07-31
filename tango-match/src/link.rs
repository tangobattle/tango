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
//! confirmed-input record, the audio plumbing. Pair-level operations
//! (ticking, snapshotting) live on the link; everything one console
//! answers for itself (display, audio out, savedata) lives on its
//! side, which is also how a console booted with no pair at all
//! ([`Console`](crate::Console)) presents itself. The traits are
//! object-safe on purpose: a `Game` registration cannot name an
//! emulator, so the erasure happens here, at the link, where it costs
//! a virtual call per tick instead of per instruction.

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
/// everything above — the shared audio drain, the solo ride, a host's
/// frame pump — reads any console through this one surface.
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

    /// The cartridge's savedata as it stands, or `None` for a game
    /// that has never written any.
    fn export_save(&mut self) -> Option<Vec<u8>> {
        None
    }

    /// The rate this console produces audio at, in Hz.
    fn audio_sample_rate(&mut self) -> f64;

    /// How this console's audio production scales when the host paces
    /// the simulation at `fps_target` (see
    /// [`AudioDrain::framerate_ratio`](crate::AudioDrain::framerate_ratio)).
    fn audio_framerate_ratio(&mut self, fps_target: f64) -> f64 {
        let _ = fps_target;
        1.0
    }

    /// Take up to `out`'s worth of this console's produced audio, as
    /// interleaved stereo, and report what is left behind. Taking it
    /// consumes it; what stays queued stays revocable.
    fn drain_audio(&mut self, out: &mut [i16]) -> crate::Drained;
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

/// Which kind of session a console's shape is being asked for.
///
/// A cart can use fewer of its console's screens in a link battle than
/// it does played alone — EXE OSS runs its whole netbattle on the
/// upper screen — so the layout is a question about the session, not
/// about the hardware.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionMode {
    /// A primed pair: live netplay, its replay, and training against
    /// it — everything [`Backend::start`] and
    /// [`Backend::open_replay`] produce.
    PvP,
    /// One console on its own, as [`Backend::start_solo`] boots it.
    Solo,
}

/// Opaque per-side session data riding beside a save: minted by the
/// game's save view, carried through the netplay commit and the replay
/// metadata, and read back — by downcast — only by the same game's
/// engine support. BN5DS's says which of its cartridge's two files the
/// session plays; most games have none. Everything in between treats
/// it as a sealed value: the wire and the replay store
/// [`serialize`](SessionPayload::serialize)'s bytes, and
/// [`Backend::parse_session_payload`] is what turns them back into the
/// typed payload.
pub trait SessionPayload: std::any::Any + Send + Sync {
    /// The bytes the wire and the replay metadata store. Never empty —
    /// empty is how those places spell "no payload" — and the game's
    /// [`Backend::parse_session_payload`] must round-trip them.
    fn serialize(&self) -> Vec<u8>;
    fn clone_box(&self) -> BoxedSessionPayload;
}

/// Boxed opaque session payload, as the owning seams hold it.
pub type BoxedSessionPayload = Box<dyn SessionPayload>;

/// Both sides' stored session payloads, typed: each seat's bytes parsed
/// by its own game's backend ([`Backend::parse_session_payload`]),
/// empty bytes staying `None`. The shape every replay opener needs
/// between a recording's raw metadata and a
/// [`ReplayConfig`](crate::ReplayConfig).
pub fn parse_session_payloads(
    backends: [&dyn Backend; 2],
    bytes: &[Vec<u8>; 2],
) -> Result<[Option<BoxedSessionPayload>; 2], crate::Error> {
    let mut payloads = [None, None];
    for seat in 0..2 {
        if !bytes[seat].is_empty() {
            payloads[seat] = Some(backends[seat].parse_session_payload(&bytes[seat])?);
        }
    }
    Ok(payloads)
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
    /// The typed [`SessionPayload`] behind a side's committed or
    /// recorded bytes — [`StartConfig::session_payloads`]' currency,
    /// and what a save view is initialized with to show the save a
    /// session plays. Only called when there *are* bytes: a side
    /// without a payload has nothing to parse, and its callers carry
    /// the absence themselves. The default is for a game that mints no
    /// payloads — bytes claiming to be one have no type to be, which
    /// is exactly what malformed means.
    fn parse_session_payload(&self, bytes: &[u8]) -> Result<BoxedSessionPayload, crate::Error> {
        let _ = bytes;
        Err(crate::Error::MalformedSessionPayload)
    }

    /// How this game's console presents its display in `mode`, known
    /// before a session exists so a host can lay out its pane.
    ///
    /// Per-mode because a cart may compose fewer screens in a link
    /// battle than it does played alone: a DS game whose netbattle
    /// lives entirely on the upper screen presents that one, and a
    /// pane, an export and a stylus area all follow from the layout
    /// rather than each re-deriving the rule. Whatever comes back must
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
    /// Per-console [`SessionPayload`], as each side's save view
    /// committed it beside its save — read, by downcast, only by the
    /// game's own engine support. `None` for games that mint none;
    /// BN5DS's names which of the cartridge's two files the session
    /// plays, and its priming walks the game's own file select to it.
    /// Determinism-critical like the saves themselves: both peers must
    /// pass identical pairs.
    pub session_payloads: [Option<&'a dyn SessionPayload>; 2],
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
