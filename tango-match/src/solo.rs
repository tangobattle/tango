//! Running one console on its own, and replaying a recorded match.
//!
//! Netplay is not the only thing a host asks an engine for. It also
//! wants a single machine for the save-editor's "just play it" ride, a
//! second copy of the game to practise against, and playback of a
//! recorded match. All three are the same shape as a match — boot a
//! console, feed it input, take frames and audio off it — so they take
//! the same shape here: an object-safe handle the host drives, built by
//! the game's own registration, with the backend's generics resolved
//! underneath.
//!
//! Everything here is optional. A game that only supports netplay (BN5
//! Double Team DS today) implements none of it, and the host reports
//! that the ride isn't available rather than failing to build.

/// One console running by itself, as a host drives it.
///
/// The backend keeps the console behind whatever sharing its audio pull
/// needs, so a host holds this and the pull side by side without
/// knowing how they meet.
pub trait RunningSolo: Send {
    /// Advance one video frame with the input held this tick — the
    /// joypad bits (see [`keys`](crate::keys)) plus the stylus, which
    /// only a touch-screen console reads. An error ends the ride — a
    /// corrupt core stops the session rather than panicking the host.
    fn tick(&mut self, input: crate::HostInput) -> Result<(), crate::Error>;

    /// The console's display, RGBA8 in
    /// [`screen_layout`](crate::Backend::screen_layout) order.
    /// `None` before its first frame.
    fn frame(&mut self) -> Option<Vec<u8>>;

    /// The cartridge's savedata as it stands, or `None` for a game that
    /// has never written any. The host owns persisting it.
    fn export_save(&self) -> Option<Vec<u8>>;

    /// This console's audio, for the host's sound stream. Available
    /// once, right after the ride starts.
    fn audio(&self) -> Option<Box<dyn crate::AudioDrain>>;
}

/// Everything a solo ride needs to come up.
pub struct SoloConfig<'a> {
    /// The cartridge image, already patched.
    pub rom: &'a [u8],
    /// Save memory, or `None` for a blank cart.
    pub save: Option<&'a [u8]>,
    /// Pins the cart clock. `None` leaves it on the real one, which is
    /// what a desktop wants and what a browser — where there is no such
    /// clock to read — must fill in.
    pub rtc: Option<std::time::SystemTime>,
}

/// Both seats' displays at one captured moment of a replay.
///
/// A host is handed one of these when a seek lands, so it can show the
/// landing frame without emulating anything. It never holds one past
/// the call — which is what keeps the engine's snapshots entirely
/// inside the engine, with nothing to downcast on the way back.
pub trait ReplayFrames {
    /// The tick this capture is poised at.
    fn tick(&self) -> u32;

    /// One seat's display, RGBA8. Empty if that seat had not drawn yet.
    fn frame(&self, player: usize) -> Vec<u8>;
}

/// The plain owned capture: both seats' RGBA8 frames at one tick. An
/// engine publishes one directly when the pixels aren't already living
/// inside richer storage — and embeds one where they are (a seek
/// keyframe carrying a savestate alongside), so the [`ReplayFrames`]
/// impl exists once, here.
pub struct LiveFrames {
    /// The tick this capture is poised at.
    pub tick: u32,
    /// Per-seat RGBA8 frames; empty for a seat that had not drawn yet.
    pub frames: [Vec<u8>; 2],
}

impl ReplayFrames for LiveFrames {
    fn tick(&self) -> u32 {
        self.tick
    }

    fn frame(&self, player: usize) -> Vec<u8> {
        self.frames[player].clone()
    }
}

/// What one slice of a seek did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekStep {
    /// No request was pending; nothing happened.
    Idle,
    /// Still walking — call again.
    Working,
    /// The chase landed (or gave up on a plan it couldn't make).
    Landed,
}

/// A recorded match being played back.
///
/// Playback is a pair, not a single console: a replay records both
/// seats' inputs and re-simulates the match from them, so both sides
/// are available to show. The host picks which one it presents.
///
/// Seeking is the engine's, because only the engine knows what it kept
/// — keyframes, a rewind ring, how expensive a catch-up is. What the
/// host owns is *when* to seek and how much time to give it, which is
/// why [`seek_step`](Self::seek_step) takes a budget and reports back
/// rather than running to completion: a desktop hands it a worker
/// thread and an unbounded budget, a browser calls it from its event
/// loop a slice at a time and keeps painting in between.
pub trait RunningReplay: Send {
    /// Feed the next recorded input pair. `false` at end of recording.
    fn step(&mut self) -> bool;

    /// Input pairs consumed so far — the playhead tick.
    fn cursor(&self) -> u32;

    /// How many ticks the recording holds.
    fn len(&self) -> u32;

    fn at_end(&self) -> bool {
        self.cursor() >= self.len()
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Advance a pending seek by at most `budget` ticks, planning one
    /// first if a request is waiting on `ctrl`.
    ///
    /// `on_progress` reports the moving cursor, `publish` shows a
    /// landing capture, and `on_resume` unpauses a host whose seek
    /// asked to resume playback once it lands.
    fn seek_step(
        &mut self,
        ctrl: &crate::seek::SeekController,
        budget: u32,
        on_progress: &mut dyn FnMut(u32),
        publish: &mut dyn FnMut(&dyn ReplayFrames),
        on_resume: &mut dyn FnMut(),
    ) -> SeekStep;

    /// Both seats' displays right now, as a capture a host can publish.
    fn frames(&mut self, publish: &mut dyn FnMut(&dyn ReplayFrames));

    /// Audio for whichever seat `seat` currently names — a viewer can
    /// swap perspective mid-playback, so this is read per fill rather
    /// than fixed when the stream is bound.
    fn audio(&self, seat: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> Option<Box<dyn crate::AudioDrain>>;

    /// The capture nearest `tick`, if the engine kept one — what backs
    /// a scrub bar's hover thumbnail. Emulation-free, so a miss is
    /// `false` rather than a catch-up run.
    fn nearest_capture(&self, tick: u32, publish: &mut dyn FnMut(&dyn ReplayFrames)) -> bool {
        let _ = (tick, publish);
        false
    }

    /// The tick of the latest capture strictly before `tick`, which is
    /// where a clip export can pick up instead of simulating from boot.
    fn capture_before(&self, tick: u32) -> Option<u32> {
        let _ = tick;
        None
    }
}

/// A second, faster-than-realtime pass over the same recording that
/// reads the match's statistics off it.
///
/// Separate from playback because it is a separate simulation: it runs
/// ahead of what the viewer is watching, on its own consoles, so a user
/// scrubbing around does not disturb it and it does not disturb them.
///
/// Sliced for the same reason a seek is: this is background work
/// competing with playback for a host's time, so how much of it happens
/// at once is the host's call.
pub trait StatsPass: Send {
    /// Run up to `budget` ticks of the pass. `true` while there is more
    /// to do; `false` once the recording is finished or the pass was
    /// cancelled.
    fn step(&mut self, budget: u32) -> Result<bool, crate::Error>;

    /// Ticks the pass has covered so far, for a host drawing its
    /// progress.
    fn progress(&self) -> u32;

    /// The fold so far, for a host drawing the chart while the pass is
    /// still running. `None` if this pass collects no statistics.
    fn preview(&self) -> Option<crate::analysis::MatchStats> {
        None
    }

    /// The finished analysis, once [`step`](Self::step) has reported the
    /// pass done. A cancelled pass yields nothing.
    fn finish(self: Box<Self>) -> Option<crate::analysis::MatchStats>;
}

/// One recording, as an engine offers it: the pair a viewer watches and
/// the statistics pass that runs ahead of it.
///
/// Both are booted on demand rather than here, because booting one is
/// seconds of priming and a host runs the two on separate threads —
/// asking for them separately is what keeps those boots concurrent.
/// What they share is the keyframes they lay down, so the pass racing
/// ahead is also what makes seeking behind the playhead cheap.
pub trait ReplaySet: Send + Sync {
    /// Boot the playback pair. Blocks for the priming walk.
    fn playback(&self) -> Result<Box<dyn RunningReplay>, crate::Error>;

    /// Boot the statistics pass. Blocks for the priming walk.
    fn stats(&self) -> Result<Box<dyn StatsPass>, crate::Error>;

    /// Where the pass reports each round boundary it crosses, if the
    /// host asked for round marks when it opened the set.
    fn round_marks(&self) -> Option<std::sync::Arc<std::sync::Mutex<Vec<u32>>>>;

    /// Abandon both simulations. A pass mid-slice sees this and stops.
    fn cancel(&self);
}

/// Everything a replay needs to come up: the recording's own header,
/// which is what the app read out of the replay file.
///
/// Owned rather than borrowed, because a [`ReplaySet`] outlives the
/// call that made it — each of its two simulations boots later, on
/// whichever thread the host decided should pay for it.
pub struct ReplayConfig {
    /// Per-seat ROM images, already patched.
    pub roms: [Vec<u8>; 2],
    /// Per-seat save memory as it stood when the match started.
    pub saves: [Vec<u8>; 2],
    /// The recorded input rows, `(p0, p1)` per tick.
    pub inputs: std::sync::Arc<Vec<[crate::HostInput; 2]>>,
    /// The match's negotiated seed and clock, so re-simulation lands
    /// where the original did.
    pub rng_seed: [u8; 16],
    pub rtc: std::time::SystemTime,
    pub match_type: (u8, u8),
    /// Which seat the recording was taken from.
    pub local_player: usize,
    /// The peer's cartridge, for the seat this factory does not own.
    pub peer_rom: PeerRom,
    /// Collect per-round statistics as the pass runs. A host that only
    /// wants to watch leaves this off and the pass just lays keyframes.
    pub want_stats: bool,
    /// Record where each round after the first begins, for the scrub
    /// bar's round marks.
    pub want_round_marks: bool,
}

/// A cartridge as its own ROM header names it.
///
/// A match can span two variants and two regions — Gregar against
/// Falzar, a Japanese cart against an American one — and an engine may
/// need per-cartridge support for each seat. But a factory hangs off
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
