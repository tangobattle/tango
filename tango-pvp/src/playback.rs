//! SIO-replay playback machinery: the linearly-driven pair, its
//! snapshot stores, and the background prefetch body.
//!
//! An SIO replay ([`crate::replay::VERSION`]) is the boot
//! configuration plus one continuous run of confirmed `[p0, p1]` pair
//! ticks. The pair is deterministic, so playback is a linear re-sim:
//! boot, prime, feed the stream — and *any* recorded tick can be
//! reached by loading the nearest pair [`Snapshot`] at or before it
//! and stepping forward. There is no stepper, no shadow, and no
//! per-round structure in the stream (rounds are re-derived from
//! RAM-poll telemetry by the prefetch pass).
//!
//! The host owns the threads (drive loop, prefetcher, seek worker);
//! this module provides the work they do, mirroring
//! [`crate::replay::playback`]'s split for the trap engine.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::telemetry::Telemetry;
use crate::{GameSupport, PrimeConfig};

/// Cap on priming ticks, mirroring the live engine's bound.
const MAX_PRIME_TICKS: u32 = 3600;

/// Take a keyframe at most once per this many ticks — same trade-off as
/// the trap engine's `MID_ROUND_SNAPSHOT_INTERVAL`.
pub const KEYFRAME_INTERVAL: u32 = 60;

/// Depth of the [`RewindRing`]'s per-tick window behind the playhead.
pub const REWIND_FRAMES: u32 = 90;

/// Hard cap on rewind-ring entries (see the trap engine's
/// `REWIND_BUFFER_MAX_ENTRIES` for the sizing rationale).
const REWIND_MAX_ENTRIES: usize = 192;

/// A whole-pair snapshot poised at `tick` (= input pairs consumed),
/// carrying both cores' rendered frames so previews and PiP/swap blits
/// never need emulation.
pub struct Snapshot {
    pub tick: u32,
    pub state: mgba_siolink::Snapshot,
    /// Both cores' framebuffers (native BGR555), indexed by player.
    pub framebuffers: [Vec<u8>; 2],
}

/// Sparse keyframe store covering the whole replay, shared between the
/// prefetch worker, the drive loop, and the seek chase.
#[derive(Clone, Default)]
pub struct SnapshotStore(Arc<Mutex<BTreeMap<u32, Arc<Snapshot>>>>);

impl SnapshotStore {
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

    pub fn push(&self, snap: Arc<Snapshot>) {
        self.0.lock().unwrap().insert(snap.tick, snap);
    }

    /// Largest keyframe with `tick <= target`, if any.
    pub fn best_at_or_before(&self, target: u32) -> Option<Arc<Snapshot>> {
        self.0
            .lock()
            .unwrap()
            .range(..=target)
            .next_back()
            .map(|(_, s)| s.clone())
    }

    /// Largest keyframe with `lo_exclusive < tick <= hi_inclusive`.
    pub fn best_in_range(&self, lo_exclusive: u32, hi_inclusive: u32) -> Option<Arc<Snapshot>> {
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

    /// Keyframe closest to `target` on either side, if any.
    pub fn nearest(&self, target: u32) -> Option<Arc<Snapshot>> {
        let entries = self.0.lock().unwrap();
        let below = entries.range(..=target).next_back();
        let above = entries
            .range((std::ops::Bound::Excluded(target), std::ops::Bound::Unbounded))
            .next();
        [below, above]
            .into_iter()
            .flatten()
            .min_by_key(|(k, _)| k.abs_diff(target))
            .map(|(_, s)| s.clone())
    }
}

/// Rolling per-tick snapshot window trailing the playhead — the pair
/// flavor of the trap engine's `RewindBuffer`: every tick the playback
/// pair runs (normal playback and seek chases alike) is captured, so
/// short backward steps land on exact snapshots. Anchor semantics and
/// eviction mirror the trap implementation.
#[derive(Clone, Default)]
pub struct RewindRing(Arc<RewindRingInner>);

#[derive(Default)]
struct RewindRingInner {
    entries: Mutex<BTreeMap<u32, Arc<Snapshot>>>,
    anchor: AtomicU32,
}

impl RewindRing {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-anchor the window at `tick` (each seek chase's target); normal
    /// playback captures only ever raise it.
    pub fn set_anchor(&self, tick: u32) {
        self.0.anchor.store(tick, Ordering::Release);
    }

    pub fn insert(&self, snap: Arc<Snapshot>) {
        // Forward playback drags the anchor along; captures below it
        // (seek catch-up runs) leave it where the chase put it.
        self.0.anchor.fetch_max(snap.tick, Ordering::AcqRel);
        let anchor = self.0.anchor.load(Ordering::Acquire);
        let mut entries = self.0.entries.lock().unwrap();
        entries.insert(snap.tick, snap);
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

    pub fn contains(&self, tick: u32) -> bool {
        self.0.entries.lock().unwrap().contains_key(&tick)
    }

    pub fn best_at_or_before(&self, target: u32) -> Option<Arc<Snapshot>> {
        self.0
            .lock_entries()
            .range(..=target)
            .next_back()
            .map(|(_, s)| s.clone())
    }

    pub fn best_in_range(&self, lo_exclusive: u32, hi_inclusive: u32) -> Option<Arc<Snapshot>> {
        self.0
            .lock_entries()
            .range((
                std::ops::Bound::Excluded(lo_exclusive),
                std::ops::Bound::Included(hi_inclusive),
            ))
            .next_back()
            .map(|(_, s)| s.clone())
    }

    pub fn nearest(&self, target: u32) -> Option<Arc<Snapshot>> {
        let entries = self.0.lock_entries();
        let below = entries.range(..=target).next_back();
        let above = entries
            .range((std::ops::Bound::Excluded(target), std::ops::Bound::Unbounded))
            .next();
        [below, above]
            .into_iter()
            .flatten()
            .min_by_key(|(k, _)| k.abs_diff(target))
            .map(|(_, s)| s.clone())
    }
}

impl RewindRingInner {
    fn lock_entries(&self) -> std::sync::MutexGuard<'_, BTreeMap<u32, Arc<Snapshot>>> {
        self.entries.lock().unwrap()
    }
}

/// Everything needed to boot a playback pair. All fields are in
/// **absolute** player order (core 0 runs player 0's game) — see
/// [`crate::analysis::AnalyzeConfig`] for the orientation contract.
pub struct BootConfig {
    pub roms: [Vec<u8>; 2],
    pub saves: [Vec<u8>; 2],
    pub support: [&'static (dyn GameSupport + Send + Sync); 2],
    pub match_type: (u8, u8),
    pub rng_seed: [u8; 16],
    pub rtc: std::time::SystemTime,
}

/// Boot and prime a pair per `config`. With `render` unset, both cores
/// skip rasterization (the analysis/prefetch fast path renders anyway —
/// its snapshots feed thumbnails — but callers can opt out).
fn boot_and_prime(
    config: &BootConfig,
    render: bool,
    cancel: Option<&AtomicBool>,
    lifecycle: &crate::telemetry::LifecycleSink,
) -> anyhow::Result<(mgba_siolink::Pair, [mgba::trapper::Trapper; 2])> {
    let mut pair = mgba_siolink::Pair::with_options(mgba_siolink::PairOptions {
        sides: [
            mgba_siolink::SideOptions {
                rom: config.roms[0].clone(),
                save: Some(config.saves[0].clone()),
            },
            mgba_siolink::SideOptions {
                rom: config.roms[1].clone(),
                save: Some(config.saves[1].clone()),
            },
        ],
        rtc: Some(config.rtc),
    })?;
    if !render {
        pair.set_frameskip(0, i32::MAX);
        pair.set_frameskip(1, i32::MAX);
    }

    let prime_config = PrimeConfig {
        match_type: config.match_type,
        rng_seed: config.rng_seed,
    };
    let primed = [crate::PrimedLatch::new(), crate::PrimedLatch::new()];
    let trappers = [
        mgba::trapper::Trapper::new(
            pair.core_mut(0),
            config.support[0].primer_traps(&prime_config, 0, lifecycle, &primed[0]),
        ),
        mgba::trapper::Trapper::new(
            pair.core_mut(1),
            config.support[1].primer_traps(&prime_config, 1, lifecycle, &primed[1]),
        ),
    ];

    let mut prime_ticks = 0;
    while !(primed[0].is_set() && primed[1].is_set()) {
        if prime_ticks >= MAX_PRIME_TICKS {
            anyhow::bail!("pvp playback: priming did not reach a link battle within {MAX_PRIME_TICKS} ticks");
        }
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            anyhow::bail!("cancelled");
        }
        pair.tick([0, 0]);
        prime_ticks += 1;
    }
    Ok((pair, trappers))
}

/// The playback pair: a booted, primed pair plus the recorded input
/// stream and a cursor. The host wraps it in a mutex — the drive loop,
/// the seek chase, and the audio pull interleave on that lock.
pub struct Playback {
    pair: mgba_siolink::Pair,
    _trappers: [mgba::trapper::Trapper; 2],
    inputs: Arc<Vec<[u32; 2]>>,
    cursor: u32,
}

impl Playback {
    /// Boot + prime a rendering pair poised at tick 0. Takes seconds of
    /// wall clock (a few hundred priming ticks) — call off the UI thread.
    /// `lifecycle` receives the pair's trap-fired round events; callers
    /// with no telemetry observer (the viewer's display pair) pass a
    /// fresh write-only stub.
    pub fn new(
        config: &BootConfig,
        inputs: Arc<Vec<[u32; 2]>>,
        lifecycle: &crate::telemetry::LifecycleSink,
    ) -> anyhow::Result<Self> {
        let (pair, trappers) = boot_and_prime(config, true, None, lifecycle)?;
        Ok(Self {
            pair,
            _trappers: trappers,
            inputs,
            cursor: 0,
        })
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

    /// Feed the next recorded input pair. Returns false at end-of-stream.
    pub fn step(&mut self) -> bool {
        let Some(&keys) = self.inputs.get(self.cursor as usize) else {
            return false;
        };
        self.pair.tick(keys);
        self.cursor += 1;
        true
    }

    /// Capture a whole-pair snapshot (with both framebuffers) at the
    /// current cursor.
    pub fn capture(&mut self) -> anyhow::Result<Arc<Snapshot>> {
        let state = self.pair.save()?;
        let framebuffers = [
            self.pair.video_buffer(0).map(|b| b.to_vec()).unwrap_or_default(),
            self.pair.video_buffer(1).map(|b| b.to_vec()).unwrap_or_default(),
        ];
        Ok(Arc::new(Snapshot {
            tick: self.cursor,
            state,
            framebuffers,
        }))
    }

    /// Restore the pair to `snap` and move the cursor there.
    pub fn load(&mut self, snap: &Snapshot) -> anyhow::Result<()> {
        self.pair.load(&snap.state)?;
        self.cursor = snap.tick;
        Ok(())
    }

    /// Direct pair access, for video/audio readout.
    pub fn pair_mut(&mut self) -> &mut mgba_siolink::Pair {
        &mut self.pair
    }
}

/// Body of the seek worker thread for SIO playback. Sleeps until a
/// [`SeekController`](crate::playback::SeekController) request
/// lands, then chases the newest target on the playback pair: load the
/// best snapshot at or before it (rewind ring ∪ keyframe store), step
/// forward feeding the recorded inputs, capturing every tick on the way
/// (the ring backfills itself), and publish the landing frame. Newer
/// requests supersede an in-flight chase at the next tick boundary.
///
/// The pair mutex is held for a chase's duration: the drive loop just
/// waits its turn (it re-paces on wake), and the audio pull uses
/// `try_lock` so it plays silence rather than stalling. Backward seeks
/// with no snapshot at or before the target are dropped silently, same
/// as the trap engine's pre-round boot window.
///
/// `on_progress` reports the moving cursor (the host mirrors it for
/// lock-free UI reads); `publish_landing` shows a landed snapshot;
/// `on_resume` unpauses the host's drive loop when a request asked to
/// resume after landing.
pub fn run_seek_worker(
    ctrl: &crate::playback::SeekController,
    playback: &Mutex<Option<Playback>>,
    store: &SnapshotStore,
    rewind: &RewindRing,
    on_progress: &mut dyn FnMut(u32),
    publish_landing: &mut dyn FnMut(&Snapshot),
    on_resume: &mut dyn FnMut(),
) {
    while ctrl.wait_for_request() {
        ctrl.begin_pass();
        'plan: loop {
            let target = ctrl.take_target();
            rewind.set_anchor(target);

            let mut guard = playback.lock().unwrap();
            let Some(pb) = guard.as_mut() else {
                // Still booting — drop the request; the user can seek
                // again once the pair is up.
                break 'plan;
            };

            let cur = pb.cursor();
            let start = if target < cur {
                let best = [rewind.best_at_or_before(target), store.best_at_or_before(target)]
                    .into_iter()
                    .flatten()
                    .max_by_key(|s| s.tick);
                match best {
                    Some(snap) => Some(snap),
                    None => break 'plan,
                }
            } else {
                [
                    rewind.best_in_range(cur, target.max(cur)),
                    store.best_in_range(cur, target.max(cur)),
                ]
                .into_iter()
                .flatten()
                .max_by_key(|s| s.tick)
            };

            if let Some(snap) = &start {
                rewind.insert(snap.clone());
                if let Err(e) = pb.load(snap) {
                    log::error!("pvp seek: snapshot load failed: {e:?}");
                    break 'plan;
                }
                on_progress(pb.cursor());
                if snap.tick >= target {
                    publish_landing(snap);
                    break 'plan;
                }
            }

            let mut landing: Option<Arc<Snapshot>> = None;
            while pb.cursor() < target {
                if ctrl.is_cancelled() {
                    ctrl.end_pass();
                    return;
                }
                if ctrl.is_dirty() {
                    drop(guard);
                    continue 'plan;
                }
                if !pb.step() {
                    break;
                }
                on_progress(pb.cursor());
                match pb.capture() {
                    Ok(snap) => {
                        if store.snapshot_needed(snap.tick) {
                            store.push(snap.clone());
                        }
                        rewind.insert(snap.clone());
                        landing = Some(snap);
                    }
                    Err(e) => log::warn!("pvp seek: capture failed: {e:?}"),
                }
            }
            // The catch-up run pushed fast-forward audio into the cores'
            // buffers; purge it so the callback doesn't play a garbled
            // burst.
            for i in 0..2 {
                pb.pair_mut().core_mut(i).audio_buffer().clear();
            }
            if let Some(snap) = landing {
                publish_landing(&snap);
            }
            break 'plan;
        }
        ctrl.end_pass();

        if !ctrl.is_dirty() && ctrl.take_resume() {
            on_resume();
        }
    }
}

/// Body of the background prefetch worker: boots its own pair and runs
/// the whole recorded stream as fast as the host allows, capturing a
/// keyframe every [`KEYFRAME_INTERVAL`] into `store` and publishing the
/// playhead-scale progress. With `round_marks` set, each round
/// boundary's tick (the second and later rounds' telemetry `Started`
/// events — the same boundaries the recorder stamps into the stream) is
/// appended as it's discovered; hosts pass `None` when the replay file
/// already carries its markers.
///
/// With `stats` set, the pass doubles as the match-stats analysis: the
/// same fold as [`crate::analysis::analyze`], reported through the
/// hook once per tick; the finished stats are returned. One simulation,
/// both products — mirroring the trap engine's `run_prefetch`.
#[allow(clippy::too_many_arguments)]
pub fn run_prefetch(
    config: &BootConfig,
    inputs: &[[u32; 2]],
    local_player: usize,
    store: SnapshotStore,
    progress: Arc<AtomicU32>,
    round_marks: Option<Arc<Mutex<Vec<u32>>>>,
    cancel: Arc<AtomicBool>,
    stats: Option<(
        crate::analysis::ChipSemantics,
        bool,
        &mut dyn FnMut(u32, u32, &crate::analysis::MatchStatsBuilder),
    )>,
) -> anyhow::Result<Option<crate::analysis::MatchStats>> {
    let lifecycle = crate::telemetry::LifecycleSink::new();
    let (mut pair, _trappers) = boot_and_prime(config, true, Some(&cancel), &lifecycle)?;

    let (mut observer, telemetry_store) = Telemetry::new(
        [config.support[0].core_poller(0), config.support[1].core_poller(1)],
        lifecycle,
    );
    let (mut builder, mut on_progress) = match stats {
        Some((chip_semantics, counts_buster, hook)) => (
            Some(crate::analysis::MatchStatsBuilder::new(chip_semantics, counts_buster)),
            Some(hook),
        ),
        None => (None, None),
    };

    // Keyframe at tick 0: the primed pre-battle state every backward
    // seek bottoms out on.
    let capture = |pair: &mut mgba_siolink::Pair, tick: u32| -> anyhow::Result<Arc<Snapshot>> {
        let state = pair.save()?;
        let framebuffers = [
            pair.video_buffer(0).map(|b| b.to_vec()).unwrap_or_default(),
            pair.video_buffer(1).map(|b| b.to_vec()).unwrap_or_default(),
        ];
        Ok(Arc::new(Snapshot {
            tick,
            state,
            framebuffers,
        }))
    };
    store.push(capture(&mut pair, 0)?);

    let total = inputs.len() as u32;
    let mut rounds_started = 0u32;

    for (i, &keys) in inputs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let tick = i as u32 + 1;
        pair.tick(keys);
        mgba_siolink::session::TickObserver::on_tick(&mut observer, &mut pair, tick);

        let (samples, events) = telemetry_store.lock().unwrap().drain_confirmed(tick);
        if let Some(round_marks) = &round_marks {
            for (event_tick, event) in &events {
                if let crate::telemetry::RoundEvent::Started = event {
                    rounds_started += 1;
                    if rounds_started > 1 {
                        round_marks.lock().unwrap().push(*event_tick);
                    }
                }
            }
        }
        if let Some(builder) = &mut builder {
            crate::analysis::fold_confirmed(builder, local_player, samples, events, &mut |t| {
                (t == tick).then_some(keys)
            });
            if let Some(hook) = &mut on_progress {
                hook(tick, total, builder);
            }
        }

        if store.snapshot_needed(tick) {
            store.push(capture(&mut pair, tick)?);
        }
        progress.store(tick, Ordering::Relaxed);
    }

    Ok(builder.map(|b| b.finish()))
}

// ---------------------------------------------------------------------------
// Seek coordination (host-facing half of the seek machinery).

/// Coordination state between seek requesters (the UI thread), the seek
/// worker thread, and the playback core's frame callback. Requests
/// coalesce: only the most recent target matters, and an in-flight chase
/// retargets mid-loop instead of finishing stale work.
pub struct SeekController {
    /// Latest requested absolute tick.
    target: AtomicU32,
    /// `target` holds a request no chase has consumed yet.
    dirty: AtomicBool,
    /// A chase is currently running on the playback core.
    chasing: AtomicBool,
    /// Unpause the playback thread once the chase lands (set by seeks
    /// that paused playback for the duration, e.g. a scrub drag).
    resume: AtomicBool,
    /// Tells the worker and any in-flight chase to exit.
    cancel: AtomicBool,
    wake_mutex: Mutex<()>,
    wake_cv: Condvar,
}

impl Default for SeekController {
    fn default() -> Self {
        Self::new()
    }
}

impl SeekController {
    pub fn new() -> Self {
        Self {
            target: AtomicU32::new(0),
            dirty: AtomicBool::new(false),
            chasing: AtomicBool::new(false),
            resume: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            wake_mutex: Mutex::new(()),
            wake_cv: Condvar::new(),
        }
    }

    /// Record `target` as the newest seek request and wake the worker.
    /// Supersedes any not-yet-landed request. Never blocks on the core.
    pub fn request(&self, target: u32, resume_after: bool) {
        self.target.store(target, Ordering::Release);
        self.resume.store(resume_after, Ordering::Release);
        self.dirty.store(true, Ordering::Release);
        // Hold the wake mutex across notify so the signal can't slip
        // between the worker's dirty check and its wait.
        let _guard = self.wake_mutex.lock().unwrap();
        self.wake_cv.notify_one();
    }

    /// Permanently stop the worker (and abort any in-flight chase).
    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::Release);
        let _guard = self.wake_mutex.lock().unwrap();
        self.wake_cv.notify_one();
    }

    /// Target of the not-yet-landed seek, if any. Lets the UI draw the
    /// playhead where it's headed instead of where the core still is.
    pub fn pending_target(&self) -> Option<u32> {
        (self.dirty.load(Ordering::Acquire) || self.chasing.load(Ordering::Acquire))
            .then(|| self.target.load(Ordering::Acquire))
    }

    /// True while a not-yet-landed seek will unpause playback when it
    /// lands. The playback thread is technically paused during the
    /// chase, but showing that to the user reads as "paused" when the
    /// session is really just mid-seek — the UI should keep displaying
    /// the playing state.
    pub fn resume_pending(&self) -> bool {
        (self.dirty.load(Ordering::Acquire) || self.chasing.load(Ordering::Acquire))
            && self.resume.load(Ordering::Acquire)
    }

    /// Withdraw a pending resume: the seek still lands, but playback
    /// stays paused afterwards. Lets a pause press during the chase win
    /// over the resume the commit scheduled.
    pub fn clear_resume(&self) {
        self.resume.store(false, Ordering::Release);
    }

    /// Whether the frame at `frame_index` should reach the display.
    /// During a chase only the landing frame passes — publishing every
    /// intermediate catch-up frame strobes a fast-forward of everything
    /// between the start snapshot and the target. `frame_index` is the
    /// recorded-frame index, same scale as the target.
    pub fn should_publish_frame(&self, frame_index: u32) -> bool {
        !self.chasing.load(Ordering::Acquire) || frame_index >= self.target.load(Ordering::Acquire)
    }

    // --- worker-side surface, for seek workers living outside this
    // module (the SIO engine's — see `crate::playback`). The trap
    // worker below predates these and touches the fields directly.

    /// Block until a request lands ([`Self::request`]) or the controller
    /// shuts down. Returns false on shutdown.
    pub fn wait_for_request(&self) -> bool {
        let mut guard = self.wake_mutex.lock().unwrap();
        loop {
            if self.cancel.load(Ordering::Acquire) {
                return false;
            }
            if self.dirty.load(Ordering::Acquire) {
                return true;
            }
            guard = self.wake_cv.wait(guard).unwrap();
        }
    }

    /// Mark a chase pass running — the publish gate closes and
    /// [`Self::pending_target`] keeps reporting until [`Self::end_pass`].
    pub fn begin_pass(&self) {
        self.chasing.store(true, Ordering::Release);
    }

    pub fn end_pass(&self) {
        self.chasing.store(false, Ordering::Release);
    }

    /// Consume the pending request: clears dirty and returns the target.
    /// Order matters — dirty clears before the read, so a request racing
    /// in re-flags for the next pass instead of being lost.
    pub fn take_target(&self) -> u32 {
        self.dirty.store(false, Ordering::Release);
        self.target.load(Ordering::Acquire)
    }

    /// A newer request landed mid-pass — abandon the current chase.
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    /// Consume a pending resume-on-landing, if one was requested.
    pub fn take_resume(&self) -> bool {
        self.resume.swap(false, Ordering::AcqRel)
    }
}
