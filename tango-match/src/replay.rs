//! Replaying a recorded match, concretely, over any [`Link`].
//!
//! A recording is the boot configuration plus one continuous run of
//! confirmed `[p0, p1]` input rows. The pair is deterministic, so
//! playback is a linear re-sim: boot, prime, feed the stream — and any
//! recorded tick can be reached by loading the nearest capture at or
//! before it and stepping forward. None of that is engine business:
//! it all runs over the seam's own [`Link`], so an engine contributes
//! exactly one thing — [`ReplayBoot`], booting a primed pair for the
//! recording — and playback, seeking, and the statistics pass come
//! with it, the same way [`Match`](crate::Match) is the one rollback
//! loop over any link and [`Solo`](crate::Solo) the one ride over any
//! console.
//!
//! The host owns the threads (drive loop, prefetcher, seek worker);
//! this module provides the work they do.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::link::Link;
use crate::seek::SeekController;
use crate::telemetry::TelemetryHandle;

/// Both seats' displays at one captured moment: per-seat RGBA8 frames,
/// empty for a seat that had not drawn yet. What a host is handed when
/// a seek lands or a frame publishes, so it can show pixels without
/// emulating anything.
#[derive(Clone)]
pub struct LiveFrames {
    /// The tick this capture is poised at.
    pub tick: u32,
    /// Per-seat RGBA8 frames; empty for a seat that had not drawn yet.
    pub frames: [Vec<u8>; 2],
}

/// A whole-pair capture poised at a tick: the link's own opaque
/// snapshot plus both seats' rendered frames, so previews and blits
/// never need emulation.
pub struct Capture {
    snap: crate::Snapshot,
    /// Both seats' rendered frames at this tick.
    pub frames: LiveFrames,
}

impl Capture {
    pub fn new(snap: crate::Snapshot, frames: LiveFrames) -> Self {
        Capture { snap, frames }
    }

    /// The tick this capture is poised at.
    pub fn tick(&self) -> u32 {
        self.frames.tick
    }

    /// The link snapshot inside, for a harness restoring by hand —
    /// [`Playback::load`] is the normal path.
    pub fn snapshot(&self) -> &crate::Snapshot {
        &self.snap
    }
}

/// Take a keyframe at most once per this many ticks — same trade-off as
/// the trap engine's `MID_ROUND_SNAPSHOT_INTERVAL`.
pub const KEYFRAME_INTERVAL: u32 = 60;

/// Depth of the [`RewindRing`]'s per-tick window behind the playhead.
pub const REWIND_FRAMES: u32 = 90;

/// Hard cap on rewind-ring entries (see the trap engine's
/// `REWIND_BUFFER_MAX_ENTRIES` for the sizing rationale).
const REWIND_MAX_ENTRIES: usize = 192;

/// Sparse keyframe store covering the whole replay, shared between the
/// prefetch worker, the drive loop, and the seek chase.
///
/// Generic over what it stores — the seam keeps [`Capture`]s, an
/// engine's probe harness can keep its own richer snapshots — with the
/// tick supplied at insertion.
pub struct SnapshotStore<T = Capture>(Arc<Mutex<BTreeMap<u32, Arc<T>>>>);

impl<T> Default for SnapshotStore<T> {
    fn default() -> Self {
        SnapshotStore(Arc::new(Mutex::new(BTreeMap::new())))
    }
}

impl<T> Clone for SnapshotStore<T> {
    fn clone(&self) -> Self {
        SnapshotStore(self.0.clone())
    }
}

impl<T> SnapshotStore<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// True if no keyframe exists within [`KEYFRAME_INTERVAL`] at or
    /// before `tick` — capturing here fills a gap.
    pub fn snapshot_needed(&self, tick: u32) -> bool {
        let lo = tick.saturating_sub(KEYFRAME_INTERVAL);
        self.0
            .lock()
            .unwrap()
            .range((std::ops::Bound::Excluded(lo), std::ops::Bound::Included(tick)))
            .next()
            .is_none()
    }

    pub fn push(&self, tick: u32, snap: Arc<T>) {
        self.0.lock().unwrap().insert(tick, snap);
    }

    /// Largest keyframe with `tick <= target`, if any.
    pub fn best_at_or_before(&self, target: u32) -> Option<Arc<T>> {
        self.0
            .lock()
            .unwrap()
            .range(..=target)
            .next_back()
            .map(|(_, s)| s.clone())
    }

    /// Largest keyframe with `lo_exclusive < tick <= hi_inclusive`.
    pub fn best_in_range(&self, lo_exclusive: u32, hi_inclusive: u32) -> Option<Arc<T>> {
        self.0
            .lock()
            .unwrap()
            .range((
                std::ops::Bound::Excluded(lo_exclusive),
                std::ops::Bound::Included(hi_inclusive),
            ))
            .next_back()
            .map(|(_, s)| s.clone())
    }
}

/// Rolling per-tick snapshot window trailing the playhead — the pair
/// flavor of the trap engine's `RewindBuffer`: every tick the playback
/// pair runs (normal playback and seek chases alike) is captured, so
/// short backward steps land on exact snapshots. Anchor semantics and
/// eviction mirror the trap implementation.
pub struct RewindRing<T = Capture>(Arc<RewindRingInner<T>>);

struct RewindRingInner<T> {
    entries: Mutex<BTreeMap<u32, Arc<T>>>,
    anchor: AtomicU32,
}

impl<T> Default for RewindRing<T> {
    fn default() -> Self {
        RewindRing(Arc::new(RewindRingInner {
            entries: Mutex::new(BTreeMap::new()),
            anchor: AtomicU32::new(0),
        }))
    }
}

impl<T> Clone for RewindRing<T> {
    fn clone(&self) -> Self {
        RewindRing(self.0.clone())
    }
}

impl<T> RewindRing<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-anchor the window at `tick` (each seek chase's target); normal
    /// playback captures only ever raise it.
    pub fn set_anchor(&self, tick: u32) {
        self.0.anchor.store(tick, Ordering::Release);
    }

    pub fn insert(&self, tick: u32, snap: Arc<T>) {
        // Forward playback drags the anchor along; captures below it
        // (seek catch-up runs) leave it where the chase put it.
        self.0.anchor.fetch_max(tick, Ordering::AcqRel);
        let anchor = self.0.anchor.load(Ordering::Acquire);
        let mut entries = self.0.entries.lock().unwrap();
        entries.insert(tick, snap);
        let keep_from = anchor.saturating_sub(REWIND_FRAMES + KEYFRAME_INTERVAL + 1);
        while let Some((&lo, _)) = entries.first_key_value() {
            if lo < keep_from {
                entries.pop_first();
            } else {
                break;
            }
        }
        while entries.len() > REWIND_MAX_ENTRIES {
            let (&lo, _) = entries.first_key_value().unwrap();
            let (&hi, _) = entries.last_key_value().unwrap();
            if lo.abs_diff(anchor) >= hi.abs_diff(anchor) {
                entries.pop_first();
            } else {
                entries.pop_last();
            }
        }
    }

    pub fn best_at_or_before(&self, target: u32) -> Option<Arc<T>> {
        self.0
            .entries
            .lock()
            .unwrap()
            .range(..=target)
            .next_back()
            .map(|(_, s)| s.clone())
    }

    pub fn best_in_range(&self, lo_exclusive: u32, hi_inclusive: u32) -> Option<Arc<T>> {
        self.0
            .entries
            .lock()
            .unwrap()
            .range((
                std::ops::Bound::Excluded(lo_exclusive),
                std::ops::Bound::Included(hi_inclusive),
            ))
            .next_back()
            .map(|(_, s)| s.clone())
    }
}

/// The playback pair: a booted, primed link plus the recorded input
/// stream and a cursor. [`Replay`] wraps it in a mutex — the drive
/// loop, the seek chase, and the audio pull interleave on that lock.
pub struct Playback {
    link: Box<dyn Link>,
    inputs: Arc<Vec<[crate::HostInput; 2]>>,
    cursor: u32,
    /// Scratch for [`discard_audio`](Playback::discard_audio).
    scratch: Vec<i16>,
}

impl Playback {
    /// Take the wheel of a booted pair, poised at tick 0.
    pub fn new(link: Box<dyn Link>, inputs: Arc<Vec<[crate::HostInput; 2]>>) -> Self {
        Playback {
            link,
            inputs,
            cursor: 0,
            scratch: Vec::new(),
        }
    }

    /// Input pairs consumed so far = the playhead tick.
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    pub fn total(&self) -> u32 {
        self.inputs.len() as u32
    }

    pub fn at_end(&self) -> bool {
        self.cursor >= self.total()
    }

    /// The recorded row that produced `tick` (the row consumed to reach
    /// it), as raw key words.
    pub fn keys_at(&self, tick: u32) -> Option<[u32; 2]> {
        let row = self.inputs.get(tick.checked_sub(1)? as usize)?;
        Some(row.map(|input| input.keys))
    }

    /// Feed the next recorded input pair. Returns false at end-of-stream.
    pub fn step(&mut self) -> bool {
        let Some(&row) = self.inputs.get(self.cursor as usize) else {
            return false;
        };
        self.link.tick(row);
        self.cursor += 1;
        true
    }

    /// Capture the pair (with both frames) at the current cursor.
    pub fn capture(&mut self) -> Result<Arc<Capture>, crate::Error> {
        let snap = self.link.snapshot(None)?;
        let f0 = self.link.side(0).frame().unwrap_or_default();
        let f1 = self.link.side(1).frame().unwrap_or_default();
        Ok(Arc::new(Capture {
            snap,
            frames: LiveFrames {
                tick: self.cursor,
                frames: [f0, f1],
            },
        }))
    }

    /// Restore the pair to `capture` and move the cursor there.
    pub fn load(&mut self, capture: &Capture) -> Result<(), crate::Error> {
        self.link.restore(&capture.snap)?;
        self.cursor = capture.tick();
        Ok(())
    }

    /// Both seats' displays right now, as an owned capture a host can
    /// publish.
    pub fn frames(&mut self) -> LiveFrames {
        let f0 = self.link.side(0).frame().unwrap_or_default();
        let f1 = self.link.side(1).frame().unwrap_or_default();
        LiveFrames {
            tick: self.cursor,
            frames: [f0, f1],
        }
    }

    /// One console's per-side surface, for the audio pull.
    pub fn side(&mut self, player: usize) -> Box<dyn crate::Side + '_> {
        self.link.side(player)
    }

    /// Throw away whatever audio the pair has queued — the fast-forward
    /// burst a seek catch-up piles up, which would play as a garbled
    /// blast if the callback ever reached it. Draining is the one purge
    /// every engine offers.
    pub fn discard_audio(&mut self) {
        self.scratch.resize(4096, 0);
        let Playback { link, scratch, .. } = self;
        for player in 0..2 {
            let mut side = link.side(player);
            while side.drain_audio(scratch).written > 0 {}
        }
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

/// One seek chase, walked a slice of ticks at a time.
///
/// A chase is a plan (find the best capture at or before the target and
/// load it) followed by a walk (step to the target, capturing as it
/// goes). Splitting it this way is what lets a host without threads run
/// one: each step does a bounded amount of work and returns, so a
/// browser can keep painting and a desktop can keep its dedicated
/// worker thread.
///
/// A newer request landing mid-walk re-plans on the next step, and a
/// cancelled controller ends the pass wherever it is.
#[derive(Default)]
struct SeekChase {
    /// Where this chase is going, once it has planned a route. `None`
    /// between chases and after a re-plan.
    target: Option<u32>,
    /// Newest capture taken on the way, published on landing.
    landing: Option<Arc<Capture>>,
}

enum Plan {
    /// Walk to this target.
    Walk(u32),
    /// Nothing left to do — landed on the plan, or couldn't make one.
    Done,
}

impl SeekChase {
    /// Advance a seek by at most `budget` ticks, planning one first if a
    /// request is pending.
    ///
    /// `on_progress` reports the moving cursor, `publish_landing` shows
    /// the landing capture, and `on_resume` unpauses a host whose seek
    /// asked to resume playback when it lands.
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        ctrl: &SeekController,
        playback: &Mutex<Playback>,
        store: &SnapshotStore,
        rewind: &RewindRing,
        budget: u32,
        on_progress: &mut dyn FnMut(u32),
        publish_landing: &mut dyn FnMut(&Capture),
        on_resume: &mut dyn FnMut(),
    ) -> SeekStep {
        if ctrl.is_cancelled() {
            return self.finish(ctrl, on_resume);
        }
        if self.target.is_none() && !ctrl.is_dirty() {
            return SeekStep::Idle;
        }
        // One lock for the whole slice: with an unbounded budget (a
        // worker thread) that means the pair is held for the entire
        // chase — nothing else may step it out from under a walk.
        let mut guard = playback.lock().unwrap();
        let pb = &mut *guard;
        if self.target.is_none() {
            match self.plan(ctrl, pb, store, rewind, on_progress, publish_landing) {
                Plan::Walk(target) => self.target = Some(target),
                Plan::Done => {
                    drop(guard);
                    return self.finish(ctrl, on_resume);
                }
            }
        }
        let target = self.target.expect("planned above");
        for _ in 0..budget {
            if pb.cursor() >= target {
                break;
            }
            if ctrl.is_cancelled() {
                drop(guard);
                return self.finish(ctrl, on_resume);
            }
            if ctrl.is_dirty() {
                // A newer target: abandon this walk and re-plan on the
                // next step, with the pass still open.
                self.target = None;
                self.landing = None;
                drop(guard);
                return SeekStep::Working;
            }
            if !pb.step() {
                break;
            }
            on_progress(pb.cursor());
            match pb.capture() {
                Ok(snap) => {
                    if store.snapshot_needed(snap.tick()) {
                        store.push(snap.tick(), snap.clone());
                    }
                    rewind.insert(snap.tick(), snap.clone());
                    self.landing = Some(snap);
                }
                Err(e) => log::warn!("replay seek: capture failed: {e:?}"),
            }
        }
        if pb.cursor() < target && pb.cursor() < pb.total() {
            drop(guard);
            return SeekStep::Working;
        }
        // The catch-up run pushed fast-forward audio into the pair;
        // purge it so the callback doesn't play a garbled burst.
        pb.discard_audio();
        drop(guard);
        if let Some(snap) = self.landing.take() {
            publish_landing(&snap);
        }
        self.finish(ctrl, on_resume)
    }

    /// Plan a chase: consume the request, load the best capture at or
    /// before it, and say whether there's a walk left to do.
    fn plan(
        &mut self,
        ctrl: &SeekController,
        pb: &mut Playback,
        store: &SnapshotStore,
        rewind: &RewindRing,
        on_progress: &mut dyn FnMut(u32),
        publish_landing: &mut dyn FnMut(&Capture),
    ) -> Plan {
        ctrl.begin_pass();
        let target = ctrl.take_target();
        rewind.set_anchor(target);

        let cur = pb.cursor();
        let start = if target < cur {
            let best = [rewind.best_at_or_before(target), store.best_at_or_before(target)]
                .into_iter()
                .flatten()
                .max_by_key(|s| s.tick());
            match best {
                Some(snap) => Some(snap),
                None => return Plan::Done,
            }
        } else {
            [
                rewind.best_in_range(cur, target.max(cur)),
                store.best_in_range(cur, target.max(cur)),
            ]
            .into_iter()
            .flatten()
            .max_by_key(|s| s.tick())
        };

        if let Some(snap) = &start {
            rewind.insert(snap.tick(), snap.clone());
            if let Err(e) = pb.load(snap) {
                log::error!("replay seek: capture load failed: {e:?}");
                return Plan::Done;
            }
            on_progress(pb.cursor());
            if snap.tick() >= target {
                publish_landing(snap);
                return Plan::Done;
            }
        }
        Plan::Walk(target)
    }

    /// End the pass: clear the chase, and run the resume the seek
    /// scheduled unless a newer request has already superseded it.
    fn finish(&mut self, ctrl: &SeekController, on_resume: &mut dyn FnMut()) -> SeekStep {
        self.target = None;
        self.landing = None;
        ctrl.end_pass();
        if !ctrl.is_dirty() && !ctrl.is_cancelled() && ctrl.take_resume() {
            on_resume();
        }
        SeekStep::Landed
    }
}

/// A recording being played back: the pair a viewer watches, with the
/// seek machinery that serves it — keyframes across the whole
/// recording, a rewind ring around the playhead, and the chase that
/// walks between them.
///
/// Playback is a pair, not a single console: a replay records both
/// seats' inputs and re-simulates the match from them, so both sides
/// are available to show. The host picks which one it presents.
pub struct Replay {
    playback: Arc<Mutex<Playback>>,
    store: SnapshotStore,
    rewind: RewindRing,
    chase: SeekChase,
    len: u32,
}

impl Replay {
    fn new(playback: Playback, store: SnapshotStore) -> Self {
        let len = playback.total();
        Replay {
            playback: Arc::new(Mutex::new(playback)),
            store,
            rewind: RewindRing::new(),
            chase: SeekChase::default(),
            len,
        }
    }

    /// Feed the next recorded input pair, capturing the tick into the
    /// rewind ring (keyframes shared into the store). `false` at end of
    /// recording.
    pub fn step(&mut self) -> bool {
        let mut pb = self.playback.lock().unwrap();
        if pb.at_end() {
            return false;
        }
        pb.step();
        match pb.capture() {
            Ok(snap) => {
                if self.store.snapshot_needed(snap.tick()) {
                    self.store.push(snap.tick(), snap.clone());
                }
                self.rewind.insert(snap.tick(), snap);
            }
            Err(e) => log::warn!("replay: frame capture failed: {e:?}"),
        }
        true
    }

    /// Input pairs consumed so far — the playhead tick.
    pub fn cursor(&self) -> u32 {
        self.playback.lock().unwrap().cursor()
    }

    /// How many ticks the recording holds.
    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn at_end(&self) -> bool {
        self.cursor() >= self.len()
    }

    /// Advance a pending seek by at most `budget` ticks, planning one
    /// first if a request is waiting on `ctrl`.
    ///
    /// Seeking is sliced for the host's sake: a desktop hands it a
    /// worker thread and an unbounded budget, a browser calls it from
    /// its event loop a slice at a time and keeps painting in between.
    /// `on_progress` reports the moving cursor, `publish` shows a
    /// landing capture, and `on_resume` unpauses a host whose seek
    /// asked to resume playback once it lands.
    pub fn seek_step(
        &mut self,
        ctrl: &SeekController,
        budget: u32,
        on_progress: &mut dyn FnMut(u32),
        publish: &mut dyn FnMut(&LiveFrames),
        on_resume: &mut dyn FnMut(),
    ) -> SeekStep {
        self.chase.step(
            ctrl,
            &self.playback,
            &self.store,
            &self.rewind,
            budget,
            on_progress,
            &mut |snap| publish(&snap.frames),
            on_resume,
        )
    }

    /// Both seats' displays right now, as a capture a host can publish.
    pub fn frames(&mut self) -> LiveFrames {
        self.playback.lock().unwrap().frames()
    }

    /// Audio for whichever seat `seat` currently names — a viewer can
    /// swap perspective mid-playback, so it's read per fill rather than
    /// fixed when the stream is bound.
    pub fn audio(&self, seat: Arc<AtomicUsize>) -> Box<dyn crate::AudioDrain> {
        crate::audio::side_audio(PlaybackSeat {
            playback: self.playback.clone(),
            player: seat,
        })
    }

    /// The latest capture at or before `tick`, if one was kept — what
    /// backs a scrub bar's hover thumbnail. The ring holds the last
    /// second or so exactly, the keyframes cover the whole recording
    /// sparsely. Emulation-free, so a miss is `None` rather than a
    /// catch-up run.
    pub fn nearest_capture(&self, tick: u32) -> Option<Arc<Capture>> {
        [self.store.best_at_or_before(tick), self.rewind.best_at_or_before(tick)]
            .into_iter()
            .flatten()
            .max_by_key(|s| s.tick())
    }

    /// The tick of the latest capture strictly before `tick`, which is
    /// where a clip export can pick up instead of simulating from boot.
    pub fn capture_before(&self, tick: u32) -> Option<u32> {
        self.nearest_capture(tick.checked_sub(1)?).map(|s| s.tick())
    }
}

/// One seat of the playback pair, as the source the shared audio drain
/// reads. The pair lives behind the seek machinery's mutex, so the
/// pull's no-wait discipline is what keeps a chase from stuttering the
/// sound device.
struct PlaybackSeat {
    playback: Arc<Mutex<Playback>>,
    /// Read per call, so a perspective swap moves the sound without the
    /// resampler above it being rebuilt.
    player: Arc<AtomicUsize>,
}

impl crate::audio::SideSource for PlaybackSeat {
    fn with_side(&self, f: &mut dyn FnMut(&mut dyn crate::Side)) {
        let mut pb = self.playback.lock().unwrap();
        f(&mut *pb.side(self.player.load(Ordering::Relaxed)));
    }

    fn try_side(&self, f: &mut dyn FnMut(&mut dyn crate::Side)) -> bool {
        match self.playback.try_lock() {
            Ok(mut pb) => {
                f(&mut *pb.side(self.player.load(Ordering::Relaxed)));
                true
            }
            Err(_) => false,
        }
    }
}

/// The statistics pass: a second pair racing ahead of the viewer
/// through the whole stream, laying a keyframe every
/// [`KEYFRAME_INTERVAL`] into the shared store and — when the pair was
/// booted with telemetry — folding what it observes into round marks
/// and [`MatchStats`](crate::analysis::MatchStats).
///
/// Separate from playback because it is a separate simulation: it runs
/// ahead of what the viewer is watching, on its own pair, so a user
/// scrubbing around does not disturb it and it does not disturb them.
/// Sliced for the same reason a seek is: this is background work
/// competing with playback for a host's time, so how much of it happens
/// at once is the host's call.
pub struct StatsPass {
    playback: Playback,
    /// The confirmed-telemetry store the pair's own link feeds, when
    /// this boot observes one.
    telemetry: Option<TelemetryHandle>,
    builder: Option<crate::analysis::StatsBuilder>,
    local_player: usize,
    store: SnapshotStore,
    round_marks: Option<Arc<Mutex<Vec<u32>>>>,
    cancel: Arc<AtomicBool>,
    rounds_started: u32,
}

impl StatsPass {
    /// Run up to `budget` ticks of the pass. `true` while there is more
    /// to do; `false` once the recording is finished or the pass was
    /// cancelled.
    pub fn step(&mut self, budget: u32) -> Result<bool, crate::Error> {
        for _ in 0..budget {
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            if !self.playback.step() {
                return Ok(false);
            }
            let tick = self.playback.cursor();

            if let Some(telemetry) = &self.telemetry {
                // Everything is final on a linear re-sim — fold as we go.
                let (samples, events) = telemetry.lock().unwrap().drain_confirmed(tick);
                if let Some(round_marks) = &self.round_marks {
                    for (event_tick, event) in &events {
                        if let crate::telemetry::RoundEvent::Started = event {
                            self.rounds_started += 1;
                            if self.rounds_started > 1 {
                                round_marks.lock().unwrap().push(*event_tick);
                            }
                        }
                    }
                }
                if let Some(builder) = &mut self.builder {
                    crate::analysis::fold_confirmed(builder, self.local_player, samples, events, &mut |t| {
                        (t == tick).then(|| self.playback.keys_at(tick)).flatten()
                    });
                }
            }

            if self.store.snapshot_needed(tick) {
                self.store.push(tick, self.playback.capture()?);
            }
        }
        Ok(!self.playback.at_end())
    }

    /// Ticks the pass has covered so far, for a host drawing its
    /// progress.
    pub fn progress(&self) -> u32 {
        self.playback.cursor()
    }

    /// The fold so far, for a host drawing the chart while the pass is
    /// still running. `None` if this pass collects no statistics.
    pub fn preview(&self) -> Option<crate::analysis::MatchStats> {
        self.builder.as_ref().map(|b| b.snapshot())
    }

    /// The finished analysis, once [`step`](Self::step) has reported the
    /// pass done. A cancelled pass yields nothing.
    pub fn finish(self) -> Option<crate::analysis::MatchStats> {
        self.playback.at_end().then(|| self.builder.map(|b| b.finish()))?
    }
}

/// What an engine's replay boot hands back: the pair, primed and
/// poised at tick 0.
pub struct BootedReplay {
    /// The primed pair, rendering on both sides.
    pub link: Box<dyn Link>,
    /// The store the pair's telemetry feeds, when the boot was asked to
    /// observe and the game has telemetry to wire. The link itself
    /// polls and revokes; this is the reading end.
    pub telemetry: Option<TelemetryHandle>,
}

/// The one thing an engine contributes to replays: booting a primed
/// pair for the recording. Everything above it — playback, seeking,
/// the statistics pass — is [`ReplaySet`]'s, concretely.
pub trait ReplayBoot: Send + Sync {
    /// Boot + prime one pair. Called up to twice — the display pair and
    /// the stats pass each boot on whichever host thread pays for it,
    /// which is what keeps those boots concurrent. With `observe`, wire
    /// the game's telemetry and hand back its store — the stats pass
    /// asks for it, the display pair doesn't pay for it. Flipping
    /// `cancel` mid-prime fails the boot with
    /// [`Error::Cancelled`](crate::Error::Cancelled) instead of
    /// finishing the walk.
    fn boot(&self, observe: bool, cancel: &AtomicBool) -> Result<BootedReplay, crate::Error>;
}

/// One recording, ready to simulate: the pair a viewer watches and the
/// statistics pass that runs ahead of it.
///
/// Both are booted on demand rather than up front, because booting one
/// is seconds of priming and a host runs the two on separate threads —
/// asking for them separately is what keeps those boots concurrent.
/// What they share is the keyframes they lay down, so the pass racing
/// ahead is also what makes seeking behind the playhead cheap.
pub struct ReplaySet {
    boot: Box<dyn ReplayBoot>,
    inputs: Arc<Vec<[crate::HostInput; 2]>>,
    local_player: usize,
    /// The local game's usage-event fold, taken by the stats boot.
    usage: Mutex<Option<crate::analysis::UsageFold>>,
    /// Shared: the pass racing ahead lays the keyframes a seek behind
    /// the playhead lands on.
    store: SnapshotStore,
    round_marks: Option<Arc<Mutex<Vec<u32>>>>,
    cancel: Arc<AtomicBool>,
}

impl ReplaySet {
    /// The set over an engine's boot. `usage` is the local game's
    /// usage-event fold when the host wants statistics computed — the
    /// engine resolves it off the local seat's (patched) ROM, which is
    /// the cart whose statistics a viewer is shown.
    pub fn new(config: &ReplayConfig, usage: Option<crate::analysis::UsageFold>, boot: impl ReplayBoot + 'static) -> Self {
        ReplaySet {
            boot: Box::new(boot),
            inputs: config.inputs.clone(),
            local_player: config.local_player,
            usage: Mutex::new(usage),
            store: SnapshotStore::new(),
            round_marks: config.want_round_marks.then(Default::default),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Boot the display pair. Blocks for the priming walk.
    pub fn playback(&self) -> Result<Replay, crate::Error> {
        let booted = self.boot.boot(false, &self.cancel)?;
        Ok(Replay::new(
            Playback::new(booted.link, self.inputs.clone()),
            self.store.clone(),
        ))
    }

    /// Boot the statistics pass. Blocks for the priming walk.
    pub fn stats(&self) -> Result<StatsPass, crate::Error> {
        let booted = self.boot.boot(true, &self.cancel)?;
        let mut playback = Playback::new(booted.link, self.inputs.clone());
        // Keyframe at tick 0: the primed pre-battle state every backward
        // seek bottoms out on.
        self.store.push(0, playback.capture()?);
        // No telemetry, no fold — a game without pollers still lays
        // keyframes, and the host keeps the rest of the ride.
        let builder = booted
            .telemetry
            .is_some()
            .then(|| self.usage.lock().unwrap().take())
            .flatten()
            .map(crate::analysis::StatsBuilder::new);
        Ok(StatsPass {
            playback,
            telemetry: booted.telemetry,
            builder,
            local_player: self.local_player,
            store: self.store.clone(),
            round_marks: self.round_marks.clone(),
            cancel: self.cancel.clone(),
            rounds_started: 0,
        })
    }

    /// Where the pass reports each round boundary it crosses, if the
    /// host asked for round marks when it opened the set.
    pub fn round_marks(&self) -> Option<Arc<Mutex<Vec<u32>>>> {
        self.round_marks.clone()
    }

    /// Abandon both simulations. A pass mid-slice sees this and stops,
    /// and a boot mid-prime fails out.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
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
    pub inputs: Arc<Vec<[crate::HostInput; 2]>>,
    /// The match's negotiated seed and clock, so re-simulation lands
    /// where the original did.
    pub rng_seed: [u8; 16],
    pub rtc: std::time::SystemTime,
    pub match_type: (u8, u8),
    /// Which seat the recording was taken from.
    pub local_player: usize,
    /// The peer's cartridge, for the seat this backend does not own.
    pub peer_rom: crate::PeerRom,
    /// Collect per-round statistics as the pass runs. A host that only
    /// wants to watch leaves this off and the pass just lays keyframes.
    pub want_stats: bool,
    /// Record where each round after the first begins, for the scrub
    /// bar's round marks.
    pub want_round_marks: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(tick: u32) -> Arc<Capture> {
        Arc::new(Capture::new(
            Box::new(()),
            LiveFrames {
                tick,
                frames: [vec![], vec![]],
            },
        ))
    }

    /// The store thins captures to one per keyframe interval (the
    /// window's lower bound is exclusive), and lookups walk backwards
    /// from the target.
    #[test]
    fn the_store_keeps_sparse_keyframes() {
        let store: SnapshotStore = SnapshotStore::new();
        assert!(store.snapshot_needed(1));
        store.push(1, cap(1));
        assert!(!store.snapshot_needed(KEYFRAME_INTERVAL));
        assert!(store.snapshot_needed(KEYFRAME_INTERVAL + 2));
        store.push(KEYFRAME_INTERVAL + 2, cap(KEYFRAME_INTERVAL + 2));
        assert_eq!(store.best_at_or_before(KEYFRAME_INTERVAL).map(|s| s.tick()), Some(1));
        assert_eq!(store.best_in_range(1, 500).map(|s| s.tick()), Some(KEYFRAME_INTERVAL + 2));
        assert_eq!(
            store.best_in_range(KEYFRAME_INTERVAL + 2, 500).map(|s| s.tick()),
            None
        );
    }

    /// The ring keeps a bounded window behind its anchor: entries
    /// falling out the back are evicted as the anchor advances.
    #[test]
    fn the_ring_trails_its_anchor() {
        let ring: RewindRing = RewindRing::new();
        let horizon = REWIND_FRAMES + KEYFRAME_INTERVAL + 1;
        for tick in 0..=horizon * 2 {
            ring.insert(tick, cap(tick));
        }
        assert_eq!(
            ring.best_at_or_before(horizon * 2).map(|s| s.tick()),
            Some(horizon * 2)
        );
        // The oldest entries fell out as the anchor (dragged by the
        // inserts) moved past them.
        assert_eq!(ring.best_at_or_before(horizon - 1).map(|s| s.tick()), None);
    }
}
