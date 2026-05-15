//! Replay-playback engine: snapshot store, background prefetch worker
//! body, and the synchronous seek-catch-up body. The host owns the
//! mgba playback thread and the prefetch `std::thread`; this module
//! provides the work those threads do.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::hooks::Hooks;
use crate::replay::Replay;
use crate::shadow::Shadow;
use crate::stepper::{self, ReplayCheckpoint, ReplaySnapshot};

/// Take a fresh mid-round snapshot at most once per this many absolute_ticks.
/// Coarser snapshots leave long fast-forwards on backward scrub; finer ones
/// cost RAM and a `core.save_state()` per capture (~256KB each). 240 ticks
/// ≈ 4 seconds of GBA time.
pub const MID_ROUND_SNAPSHOT_INTERVAL: u32 = 240;

/// Shared, cloneable handle to the replay-playback snapshot collection.
/// All clones share the same underlying `Vec<ReplaySnapshot>` behind a
/// mutex — the prefetch worker, the mgba frame callback, and the
/// synchronous seek path all push into the same store.
#[derive(Clone, Default)]
pub struct SnapshotStore(Arc<Mutex<Vec<ReplaySnapshot>>>);

impl SnapshotStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// True if pushing a snapshot for `cp` would fill a gap — either no
    /// round-start snapshot exists yet for this round, or no mid-round
    /// snapshot has been taken within the prior `MID_ROUND_SNAPSHOT_INTERVAL`.
    pub fn snapshot_needed(&self, cp: &ReplayCheckpoint) -> bool {
        let snaps = self.0.lock();
        let want_round_start = !cp.has_committed_this_round
            && !snaps.iter().any(|s| {
                s.checkpoint.current_round_index == cp.current_round_index && !s.checkpoint.has_committed_this_round
            });
        let lo = cp.absolute_tick.saturating_sub(MID_ROUND_SNAPSHOT_INTERVAL);
        let want_mid_round = cp.has_committed_this_round
            && !snaps
                .iter()
                .any(|s| s.checkpoint.absolute_tick > lo && s.checkpoint.absolute_tick <= cp.absolute_tick);
        want_round_start || want_mid_round
    }

    pub fn push(&self, snapshot: ReplaySnapshot) {
        self.0.lock().push(snapshot);
    }

    /// Largest snapshot with `absolute_tick <= target`, if any. Used by
    /// backward seek to pick the closest jumping-off point.
    pub fn best_at_or_before(&self, target: u32) -> Option<ReplaySnapshot> {
        self.0
            .lock()
            .iter()
            .filter(|s| s.checkpoint.absolute_tick <= target)
            .max_by_key(|s| s.checkpoint.absolute_tick)
            .cloned()
    }

    /// Largest snapshot with `lo_exclusive < absolute_tick <= hi_inclusive`,
    /// if any. Used by forward seek to skip frames when a prefetched
    /// snapshot lands closer to the target than the current playhead.
    pub fn best_in_range(&self, lo_exclusive: u32, hi_inclusive: u32) -> Option<ReplaySnapshot> {
        self.0
            .lock()
            .iter()
            .filter(|s| s.checkpoint.absolute_tick > lo_exclusive && s.checkpoint.absolute_tick <= hi_inclusive)
            .max_by_key(|s| s.checkpoint.absolute_tick)
            .cloned()
    }
}

impl SnapshotStore {
    /// Capture a snapshot for `cp` if the store has a gap there. Checks the
    /// store under the lock first so `core.save_state` / `shadow.save_state`
    /// (both ~256KB allocations) only fire on a hit.
    pub fn capture_if_needed(
        &self,
        cp: ReplayCheckpoint,
        core: &mut mgba::core::CoreMutRef<'_>,
        shadow: &Mutex<Shadow>,
    ) {
        if !self.snapshot_needed(&cp) {
            return;
        }
        let Ok(state) = core.save_state() else {
            return;
        };
        let Ok(shadow_snapshot) = shadow.lock().save_state() else {
            return;
        };
        self.push(ReplaySnapshot {
            checkpoint: cp,
            mgba_state: state,
            shadow_snapshot,
        });
    }
}

/// Body of the background prefetch worker. Spins up a fresh mgba core +
/// shadow and runs forward against `replay` as fast as the host CPU
/// allows, pushing snapshots into `store` and writing the latest
/// `absolute_tick` to `progress` after every frame. Returns `Ok(())` when
/// the replay is exhausted or `cancel` flips true.
///
/// The host spawns the thread that drives this — this function takes no
/// thread handle and never blocks except on its own work loop.
pub fn run_prefetch(
    local_rom: &[u8],
    remote_rom: &[u8],
    replay: &Replay,
    local_hooks: &'static (dyn Hooks + Send + Sync),
    remote_hooks: &'static (dyn Hooks + Send + Sync),
    store: SnapshotStore,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
) -> anyhow::Result<()> {
    let mut core = mgba::core::Core::new_gba("tango-prefetch")?;
    core.enable_video_buffer();
    core.as_mut()
        .load_rom(mgba::vfile::VFile::from_vec(local_rom.to_vec()))?;
    core.as_mut()
        .load_save(mgba::vfile::VFile::from_vec(replay.local_sram.clone()))?;
    // mgba::thread::Thread::start does this implicitly for the playback
    // core; a raw Core driven by run_frame needs it explicitly.
    core.as_mut().reset();

    local_hooks.patch(core.as_mut());

    let total_replay_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum::<u32>();
    let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);

    use rand::SeedableRng;
    let mut shadow_rng = rand_pcg::Mcg128Xsl64::from_seed(replay.rng_seed);
    let _ = rand::Rng::gen::<bool>(&mut shadow_rng);
    let shadow = Shadow::new_from_sram(
        remote_rom,
        &replay.remote_sram,
        remote_hooks,
        match_type,
        replay.is_offerer,
        replay.local_player_index,
        shadow_rng,
    )?;
    let shadow = Arc::new(Mutex::new(shadow));

    let stepper_state = stepper::State::new(
        match_type,
        replay.local_player_index,
        replay.rounds.clone(),
        0,
        replay.rng_seed,
        replay.is_offerer,
        total_replay_ticks,
        shadow.clone(),
        Box::new(|| {}),
    );
    let mut traps = local_hooks.common_traps();
    traps.extend(local_hooks.stepper_traps(stepper_state.clone()));
    core.set_traps(traps);

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        let (total_left, absolute_tick) = {
            let inner = stepper_state.lock_inner();
            (inner.total_input_pairs_left(), inner.absolute_tick())
        };
        if total_left == 0 && absolute_tick > 0 {
            // Reached end-of-replay. Mark the bar fully buffered and exit —
            // the playback thread will pick up from existing snapshots from
            // here on out.
            progress.store(total_replay_ticks, Ordering::Relaxed);
            return Ok(());
        }

        if let Some(cp) = stepper_state.capture_replay_checkpoint() {
            let mut core_ref = core.as_mut();
            store.capture_if_needed(cp, &mut core_ref, &shadow);
        }

        progress.store(absolute_tick, Ordering::Relaxed);
        core.as_mut().run_frame();
    }
}

/// Catch-up loop on the playback mgba core: optionally loads a starting
/// snapshot, then runs frames until `stepper_state.absolute_tick() >= target`,
/// capturing intermediate snapshots into `store` along the way.
///
/// Must be called on the thread that owns `core` — i.e. from inside an mgba
/// `run_on_core` callback.
pub fn seek_on_core(
    mut core: mgba::core::CoreMutRef<'_>,
    target: u32,
    stepper_state: &stepper::State,
    shadow: &Mutex<Shadow>,
    replay: &Replay,
    store: &SnapshotStore,
    start_snap: Option<&ReplaySnapshot>,
) -> anyhow::Result<()> {
    if let Some(snap) = start_snap {
        core.load_state(snap.mgba_state.as_ref())?;
        stepper_state.restore_replay_checkpoint(&snap.checkpoint, &replay.rounds)?;
        // Restore shadow alongside stepper. The shadow holds its own mgba
        // core + Round state; without restoring it, post-seek apply_input
        // calls would feed the stepper packets from the shadow's stale
        // pre-seek state.
        shadow.lock().load_state(&snap.shadow_snapshot)?;
    }

    loop {
        let cur = stepper_state.lock_inner().absolute_tick();
        if cur >= target {
            return Ok(());
        }

        if let Some(cp) = stepper_state.capture_replay_checkpoint() {
            store.capture_if_needed(cp, &mut core, shadow);
        }

        core.run_frame();
    }
}
