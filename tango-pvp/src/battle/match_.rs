use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as SyncMutex};

use super::round::Round;
use super::{MatchIdentity, ReplayConfig, MAX_QUEUE_LENGTH};

/// Handoff queue from the net receive task to the live round: inputs from
/// the peer, tagged at receive time with the peer-round index they belong
/// to. The net task pushes; the live round drains it at the top of every
/// frame (which is also the only time the rollback engine looks at remote
/// inputs, so this adds no latency). Entries tagged for an already-ended
/// local round are dropped at drain; entries for a future round wait in
/// place until that round starts.
///
/// Flow control: [`push`](RemoteInputs::push) suspends while the queue is
/// full — the net task stops reading the socket, so an overactive peer
/// backs up in their own send buffer instead of overflowing us. The drain
/// stores a wake permit whenever it removes entries, so a parked push
/// resumes by the next frame.
#[derive(Default)]
pub(super) struct RemoteInputs {
    queue: SyncMutex<VecDeque<(u32, crate::net::Input)>>,
    drained: tokio::sync::Notify,
}

impl RemoteInputs {
    /// One round's worth of pipe depth. The peer can't legitimately get
    /// further ahead than this: their own engine caps at
    /// [`MAX_QUEUE_LENGTH`] unacknowledged local inputs.
    const CAPACITY: usize = MAX_QUEUE_LENGTH;

    /// Queue one input, waiting for space if the round is slow to drain
    /// (e.g. we're still playing out a round the peer already finished).
    pub(super) async fn push(&self, peer_round: u32, input: crate::net::Input) {
        loop {
            // Arm the wakeup BEFORE re-checking, so a drain that lands
            // between the check and the await leaves a stored permit.
            let notified = self.drained.notified();
            if self.queue.lock().unwrap().len() < Self::CAPACITY {
                break;
            }
            notified.await;
        }
        self.queue.lock().unwrap().push_back((peer_round, input));
    }

    /// Drain entries for local round `round_idx` into `f` (the engine
    /// push): stale tags drop, future tags stay queued for the round
    /// they belong to.
    pub(super) fn drain(
        &self,
        round_idx: u32,
        mut f: impl FnMut(crate::net::Input) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let mut queue = self.queue.lock().unwrap();
        let mut popped = false;
        let result = loop {
            match queue.front() {
                Some(&(peer_round, _)) if peer_round < round_idx => {
                    // Stale tail from a round we already ended locally.
                    queue.pop_front();
                    popped = true;
                }
                Some(&(peer_round, _)) if peer_round == round_idx => {
                    let (_, input) = queue.pop_front().unwrap();
                    popped = true;
                    if let Err(e) = f(input) {
                        break Err(e);
                    }
                }
                // Empty, or the peer raced ahead — leave those for the
                // next round's drain.
                _ => break Ok(()),
            }
        };
        if popped {
            // Permit-storing notify: no lost wakeup if the push isn't
            // parked yet.
            self.drained.notify_one();
        }
        result
    }
}

/// Engine metrics for the live round, surfaced to the host for the status
/// bar. All zero while the round is armed (pre first-commit).
#[derive(Clone, Copy, Debug)]
pub struct RoundMetrics {
    /// How far our input frontier leads the inputs we've confirmed from the
    /// peer, in frames.
    pub local_tick_advantage: i16,
    /// The peer's most recently reported view of the same, from their side.
    pub remote_tick_advantage: i16,
    /// Speculative frames the most recent step discarded and re-simulated
    /// because a confirmed remote input contradicted the prediction.
    pub misprediction_depth: u32,
}

/// A restorable snapshot of a live training round: the engine's settled state
/// bundle, the match RNG at capture, and the prediction seed. Opaque to the
/// host — produced by [`Match::training_checkpoint`] and consumed by
/// [`Match::restore_training_checkpoint`]. ~Two core states big; hosts keep a
/// handful of slots, not a stream.
#[derive(Clone)]
pub struct TrainingCheckpoint {
    state: super::world::MgbaState,
    rng: rand_pcg::Mcg128Xsl64,
    remote: crate::input::PartialInput,
}

/// Training's in-process remote: asked for the dummy's joyflags once per
/// local input, inside the same primary trap fire, with the live core in
/// hand — the implementation may read any game state it wants (the core is
/// at its settled pre-advance boundary) before answering.
pub trait TrainingRemoteSource: Send {
    fn next_joyflags(&mut self, core: mgba::core::CoreMutRef<'_>) -> u16;
}

/// Where a match's remote inputs come from, picked at construction: a
/// networked peer, or an in-process training source.
pub enum Remote {
    /// The receive loop pushes tagged inputs into the round's queue as
    /// they arrive; the round drains them each frame.
    Peer,
    /// The round asks this source synchronously, once per local input,
    /// inside the same trap fire — every tick confirms with zero
    /// speculation. Swappable mid-match via
    /// [`Match::set_training_remote_source`] (script reloads).
    Training(Box<dyn TrainingRemoteSource>),
}

/// [`Remote`] in its constructed form, shared between the match (receive
/// loop, source swaps) and its rounds (the per-tick staging in
/// `add_local_input_and_fastforward`).
pub(super) enum RemoteSource {
    Peer(RemoteInputs),
    Training(SyncMutex<Box<dyn TrainingRemoteSource>>),
}

/// The outbound network channel. Locked only from the emulator thread —
/// primary traps shipping the per-frame input and `end_round` emitting the
/// `EndOfRound` marker; the net task only receives. A std mutex: `send` is
/// synchronous (it `blocking_send`s into the pump channel), so the lock is
/// never held across an await.
pub(crate) type SenderMutex = SyncMutex<Box<dyn crate::net::Sender + Send + Sync>>;

/// Connection-level state for a single PvP match.
pub struct Match {
    shadow: Arc<SyncMutex<crate::shadow::Shadow>>,
    rom: Vec<u8>,
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    sender: SenderMutex,
    /// Shared match RNG (both peers hold the same stream and must draw in
    /// lockstep). Only ever locked from trap closures on the emulator
    /// thread, hence a plain std mutex.
    rng: SyncMutex<rand_pcg::Mcg128Xsl64>,
    cancellation_token: tokio_util::sync::CancellationToken,
    identity: MatchIdentity,
    /// The in-progress round, if any. Locked only from the emulator thread
    /// (traps) and the UI's stats scrape — the net receive task talks to the
    /// round exclusively through [`SharedRemoteInputs`], which is what lets
    /// this be a plain std mutex.
    round_state: SyncMutex<Option<Round>>,
    /// Fires when the live round's local input queue climbs to
    /// [`RECONNECT_QUEUE_LENGTH`](crate::battle::RECONNECT_QUEUE_LENGTH) — a
    /// dead link, since the peer has stopped matching our inputs. The round
    /// raises it (rising-edge, from the emulator thread) at the point it
    /// enqueues a local input; the session's reconnect coordinator awaits it via
    /// [`stalled`](Self::stalled) to pause and rebuild before the queue can
    /// reach the overflow bail. Detecting the stall where the queue grows beats
    /// polling its depth — no latency, no cross-thread round-state lock.
    local_stall: Arc<tokio::sync::Notify>,
    primary_thread_handle: mgba::thread::Handle,
    /// Handoff queue from the net receive task to the live round.
    remote_source: Arc<RemoteSource>,
    /// Count of local `end_round` calls. Each remote Input is tagged with
    /// `peer_round_idx` at receive time; each [`Round`] is stamped with this
    /// counter at creation, and drains only matching entries — a stale tail
    /// from a round we've already closed gets dropped and a peer who's raced
    /// ahead waits in the queue until we catch up.
    local_round_idx: AtomicU32,
    /// Count of `EndOfRound` packets received from the peer. Loaded at
    /// receive time to stamp each Input with the round it belongs to.
    peer_round_idx: AtomicU32,
    replay_writer: Arc<SyncMutex<Option<crate::replay::Writer>>>,
    /// This side's frame delay, in frames. Realized purely locally by the
    /// `Round` rendering `frontier − frame_delay`; never touches the netcode or
    /// the wire. Shared atomic so the owning `PvpSession` can live-adjust it
    /// mid-match (footer slider) and every round reads the current value each
    /// frame.
    frame_delay: Arc<AtomicU32>,
    /// Whether this side mutes battle BGM. Reaches the game through the
    /// battle-start play-music traps on both cores that execute ticks: the
    /// live primary's (via `install_on_primary`) and each round's re-sim
    /// [`Stepper`](crate::stepper::Stepper)'s — the stepper's matters most,
    /// since its snapshots (carrying the sound driver's RAM) are loaded into
    /// the live core every frame. Local-only, like `frame_delay`.
    disable_bgm: bool,
    /// Simulation speed multiplier as `f32` bits, applied by the live round to
    /// its per-frame fps target. 1.0 (the default) for real PvP — the peers
    /// must run at [`EXPECTED_FPS`](super::EXPECTED_FPS) to stay in sync;
    /// training sessions, whose "peer" is in-process, may set it freely via
    /// [`set_speed_factor`](Self::set_speed_factor).
    speed_factor: Arc<AtomicU32>,
    /// Training drill loop: when set, a round that is about to end is
    /// instead reset to this checkpoint (see
    /// [`end_round_or_cancel`](Self::end_round_or_cancel)) — a KO snaps the
    /// rep back to its starting state rather than tearing the round down.
    /// `None` (always, in real PvP) ends rounds normally.
    training_round_end_reset: SyncMutex<Option<TrainingCheckpoint>>,
    /// Counts every applied training reset (manual restores + round-end
    /// interceptions). The host watches it to rewind its dummy scripting.
    training_resets: AtomicU32,
    /// Counts round-end interceptions only — the host additionally stops
    /// an in-progress authoring take on these (the KO ended the take).
    training_round_end_resets: AtomicU32,
}

impl Match {
    pub fn new(
        rom: Vec<u8>,
        local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        primary_thread_handle: mgba::thread::Handle,
        sender: Box<dyn crate::net::Sender + Send + Sync>,
        remote: Remote,
        cancellation_token: tokio_util::sync::CancellationToken,
        rng: rand_pcg::Mcg128Xsl64,
        shadow: crate::shadow::Shadow,
        identity: MatchIdentity,
        replay: ReplayConfig,
        frame_delay: Arc<AtomicU32>,
        disable_bgm: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            shadow: Arc::new(SyncMutex::new(shadow)),
            local_hooks,
            rom,
            sender: SyncMutex::new(sender),
            rng: SyncMutex::new(rng),
            cancellation_token,
            identity,
            round_state: SyncMutex::new(None),
            local_stall: Arc::new(tokio::sync::Notify::new()),
            primary_thread_handle,
            remote_source: Arc::new(match remote {
                Remote::Peer => RemoteSource::Peer(RemoteInputs::default()),
                Remote::Training(source) => RemoteSource::Training(SyncMutex::new(source)),
            }),
            local_round_idx: AtomicU32::new(0),
            peer_round_idx: AtomicU32::new(0),
            replay_writer: Arc::new(SyncMutex::new(replay.writer)),
            frame_delay,
            disable_bgm,
            speed_factor: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            training_round_end_reset: SyncMutex::new(None),
            training_resets: AtomicU32::new(0),
            training_round_end_resets: AtomicU32::new(0),
        })
    }

    pub(super) fn disable_bgm(&self) -> bool {
        self.disable_bgm
    }

    pub(super) fn rtc_time(&self) -> std::time::SystemTime {
        self.identity.rtc_time
    }

    pub(super) fn rom(&self) -> &[u8] {
        &self.rom
    }

    pub(super) fn local_hooks(&self) -> &'static (dyn crate::hooks::Hooks + Send + Sync) {
        self.local_hooks
    }

    pub(super) fn local_player_index(&self) -> u8 {
        self.identity.local_player_index
    }

    pub(super) fn shadow_handle(&self) -> Arc<SyncMutex<crate::shadow::Shadow>> {
        self.shadow.clone()
    }

    pub(crate) fn sender(&self) -> &SenderMutex {
        &self.sender
    }

    pub(super) fn replay_writer_handle(&self) -> Arc<SyncMutex<Option<crate::replay::Writer>>> {
        self.replay_writer.clone()
    }

    pub(super) fn primary_thread_handle(&self) -> mgba::thread::Handle {
        self.primary_thread_handle.clone()
    }

    pub(super) fn frame_delay(&self) -> Arc<AtomicU32> {
        self.frame_delay.clone()
    }

    pub(super) fn remote_source_handle(&self) -> Arc<RemoteSource> {
        self.remote_source.clone()
    }

    pub(super) fn speed_factor_handle(&self) -> Arc<AtomicU32> {
        self.speed_factor.clone()
    }

    /// Set the simulation speed multiplier (1.0 = realtime). Training-only:
    /// the live round scales its per-frame fps target by this, which is only
    /// sound when the "peer" is in-process — a networked peer must stay at
    /// [`EXPECTED_FPS`](super::EXPECTED_FPS). Takes effect on the next frame.
    pub fn set_speed_factor(&self, factor: f32) {
        self.speed_factor.store(factor.to_bits(), Ordering::Relaxed);
    }

    /// Swap the training remote-input source (a script reload); takes
    /// effect on the next tick. No-op with a logged error on a
    /// [`Remote::Peer`] match — the peer path has no source to swap.
    pub fn set_training_remote_source(&self, source: Box<dyn TrainingRemoteSource>) {
        match &*self.remote_source {
            RemoteSource::Training(slot) => *slot.lock().unwrap() = source,
            RemoteSource::Peer(_) => {
                log::error!("set_training_remote_source called on a peer match");
            }
        }
    }

    /// Turn the shadow core's rasterization on/off — see
    /// [`Shadow::set_rendering`](crate::shadow::Shadow::set_rendering).
    /// Training-only: shows the opponent's perspective while the user
    /// possesses the dummy.
    pub fn set_shadow_rendering(&self, on: bool) {
        self.shadow.lock().unwrap().set_rendering(on);
    }

    /// Copy the shadow core's latest rendered frame into `buf` — see
    /// [`Shadow::read_video_buffer`](crate::shadow::Shadow::read_video_buffer).
    /// May briefly block on an in-flight shadow tick (the co-sim worker
    /// holds the same lock while stepping).
    pub fn read_shadow_video_buffer(&self, buf: &mut [u8]) -> bool {
        self.shadow.lock().unwrap().read_video_buffer(buf)
    }

    pub(super) fn local_stall_handle(&self) -> Arc<tokio::sync::Notify> {
        self.local_stall.clone()
    }

    /// The local round counter a [`Round`] created right now belongs to.
    pub(crate) fn current_local_round_idx(&self) -> u32 {
        self.local_round_idx.load(Ordering::Acquire)
    }

    /// Host-facing snapshot of the live round's engine metrics, for the
    /// status bar. The host reads these through
    /// [`MatchHandle::round_metrics`](crate::hooks::MatchHandle::round_metrics)
    /// instead of locking round state — the round object itself is
    /// trap-side API.
    pub fn round_metrics(&self) -> Option<RoundMetrics> {
        let round_state = self.round_state.lock().unwrap();
        round_state.as_ref().map(|round| round.metrics())
    }

    /// Picks the per-match local_player_index. Both peers must call this with
    /// the same shared RNG state at the same point in the protocol so they end
    /// up on opposite sides. Advances the RNG by one draw.
    pub fn pick_local_player_index(rng: &mut rand_pcg::Mcg128Xsl64, is_offerer: bool) -> u8 {
        use rand::Rng;
        let did_polite_win = rng.gen::<bool>();
        if did_polite_win == is_offerer {
            0
        } else {
            1
        }
    }

    /// Snapshot the live round's authoritative settled state as a training
    /// checkpoint. `None` while no round is running, while the round is still
    /// armed (pre first-commit — there is no engine state yet), or once the
    /// settled stream has already ended the round (the tail after a round end
    /// is not a useful place to come back to, and the live core may have left
    /// the battle loop that restores would resume through).
    ///
    /// Bundles the match RNG alongside the state so a restore rewinds any
    /// shared-stream draws with it. Cheap-ish (~two core states' memcpy);
    /// callable from any thread — the round lock serializes it against traps.
    pub fn training_checkpoint(&self) -> Option<TrainingCheckpoint> {
        let round_state = self.round_state.lock().unwrap();
        let (state, remote) = round_state.as_ref()?.training_state()?;
        Some(TrainingCheckpoint {
            state,
            rng: self.rng.lock().unwrap().clone(),
            remote,
        })
    }

    /// Restore the live round to a training checkpoint: reset the rollback
    /// engine (which force-reloads the stepper + shadow cores), rewind the
    /// match RNG, drop any in-flight remote inputs — they belong to the
    /// abandoned timeline — and load the checkpoint's state into the live
    /// core itself.
    ///
    /// That last step matters: the per-game `main_read_joyflags` traps
    /// verify tick continuity between the game and the last engine-loaded
    /// state, and the live core is still on the abandoned timeline until
    /// something replaces it. The whole operation runs with the emulator
    /// thread paused, so no trap can observe the half-restored state:
    /// pause → reset engine (round-state lock, uncontended) → `run_on_core`
    /// load + neutral r4 prime → resume. The one bridging tick the live
    /// core then runs ahead of the engine is display-only — the next trap
    /// fire overwrites it with the engine's chosen frame, as every frame
    /// does.
    ///
    /// Returns `false` (without touching anything) when there is nothing to
    /// restore into: no live round, an armed round, or a settled round end —
    /// the same conditions under which [`training_checkpoint`](Self::training_checkpoint)
    /// declines to capture. Restoring also invalidates the linear replay
    /// record, so the writer is finalized (its prefix stays playable) and
    /// recording stops for the rest of the match.
    ///
    /// Only sound for training: a networked peer's sim would not follow.
    pub fn restore_training_checkpoint(&self, checkpoint: &TrainingCheckpoint) -> anyhow::Result<bool> {
        let handle = self.primary_thread_handle.clone();
        let was_paused = handle.is_paused();
        handle.pause();
        let reset = self.apply_training_checkpoint(checkpoint);
        if matches!(reset, Ok(true)) {
            // Re-anchor the live core on the restored timeline. The state is
            // poised at its boundary `main_read_joyflags` with r4 unset;
            // prime it neutral so the bridging tick runs with no input.
            let state = checkpoint.state.primary.clone();
            let hooks = self.local_hooks;
            handle.run_on_core(move |mut core| {
                if let Err(e) = core.load_state(&state) {
                    log::error!("training restore: live core load failed: {e}");
                    return;
                }
                hooks.inject_joyflags_on_primary(core, 0);
            });
        }
        if !was_paused {
            handle.unpause();
        }
        reset
    }

    /// The engine-side half of a training restore: reset the rollback
    /// engine (force-reloading stepper + shadow), rewind the match RNG,
    /// drop in-flight remote inputs, finalize the replay. The caller is
    /// responsible for re-anchoring the live core and for making sure no
    /// trap can observe the intermediate state (a paused thread, or being
    /// *inside* the one trap fire that called this). Bumps the reset
    /// counter the host's dummy scripting watches.
    fn apply_training_checkpoint(&self, checkpoint: &TrainingCheckpoint) -> anyhow::Result<bool> {
        let applied = {
            let mut round_state = self.round_state.lock().unwrap();
            let Some(round) = round_state.as_mut() else {
                return Ok(false);
            };
            if !round.reset_to(&checkpoint.state, checkpoint.remote.clone())? {
                return Ok(false);
            }
            *self.rng.lock().unwrap() = checkpoint.rng.clone();
            // No in-flight remote inputs to drop: a training match's remote
            // is synchronous (RemoteSource::Training), so nothing is queued
            // between trap fires.
            self.finish_replay()?;
            true
        };
        if applied {
            self.training_resets.fetch_add(1, Ordering::Release);
        }
        Ok(applied)
    }

    /// Arm (or disarm) the training drill loop: with a checkpoint in the
    /// slot, a round that is about to end resets to it instead of ending —
    /// see [`end_round_or_cancel`](Self::end_round_or_cancel).
    pub fn set_training_round_end_reset(&self, checkpoint: Option<TrainingCheckpoint>) {
        *self.training_round_end_reset.lock().unwrap() = checkpoint;
    }

    /// Total applied training resets (manual + round-end). Monotonic;
    /// hosts watch for changes to rewind their dummy scripting.
    pub fn training_reset_count(&self) -> u32 {
        self.training_resets.load(Ordering::Acquire)
    }

    /// Applied round-end interception resets only. Hosts additionally
    /// stop an in-progress authoring take on these — the KO ended it.
    pub fn training_round_end_reset_count(&self) -> u32 {
        self.training_round_end_resets.load(Ordering::Acquire)
    }

    /// Round-end interception, called from the round-ending trap (on the
    /// emulator thread, with the live core in hand). Applies the armed
    /// checkpoint and re-anchors the live core directly — no pause /
    /// run_on_core: being inside the trap already excludes every other
    /// trap fire. Returns whether the end was intercepted.
    fn try_training_round_end_reset(&self, mut core: mgba::core::CoreMutRef) -> bool {
        let checkpoint = self.training_round_end_reset.lock().unwrap().clone();
        let Some(checkpoint) = checkpoint else {
            return false;
        };
        match self.apply_training_checkpoint(&checkpoint) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(e) => {
                log::error!("training round-end reset failed: {e:#}");
                return false;
            }
        }
        // Loading the state inside the trap means execution resumes from
        // the restored boundary the moment the trap returns — the round
        // never reaches its ending path.
        if let Err(e) = core.load_state(&checkpoint.state.primary) {
            log::error!("training round-end reset: live core load failed: {e}");
            self.cancel();
            return true;
        }
        self.local_hooks.inject_joyflags_on_primary(core, 0);
        self.training_round_end_resets.fetch_add(1, Ordering::Release);
        true
    }

    /// Closes the replay file, if one is open.
    pub fn finish_replay(&self) -> anyhow::Result<()> {
        let writer = self.replay_writer.lock().unwrap().take();
        let Some(writer) = writer else { return Ok(()) };
        writer.finish()?;
        Ok(())
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    pub fn cancelled(&self) -> tokio_util::sync::WaitForCancellationFuture<'_> {
        self.cancellation_token.cancelled()
    }

    /// Resolves the next time the live round's local input queue crosses up to
    /// [`RECONNECT_QUEUE_LENGTH`](crate::battle::RECONNECT_QUEUE_LENGTH) — a dead
    /// link. The reconnect coordinator selects on this to pause and rebuild. The
    /// signal is rising-edge, so a queue that sits above the threshold (e.g. just
    /// after a reconnect, before the resent inputs drain it) fires once, not
    /// every frame; it re-arms once the queue drops back below.
    pub fn stalled(&self) -> tokio::sync::futures::Notified<'_> {
        self.local_stall.notified()
    }

    /// Called from the primary main_read_joyflags trap when the live core
    /// reaches the round's first commit tick. Advances the shadow to its
    /// matching first commit and snapshots local state on the round.
    pub(crate) fn record_first_commit(
        &self,
        round: &mut Round,
        core: mgba::core::CoreMutRef,
        first_packet: &[u8],
    ) -> anyhow::Result<()> {
        // Snapshot the live core at its first-committed state. Done here
        // rather than at every per-game call site — the save is the same
        // for all games.
        let local_state = core.save_state()?;
        // Advance the shadow to its first-committed state and snapshot it, so the
        // session's tick-0 bundle holds both cores: a rollback to tick 0 rewinds
        // the opponent co-sim alongside the primary.
        let shadow_snapshot = {
            let mut shadow = self.shadow.lock().unwrap();
            shadow.advance_until_first_committed_state()?;
            shadow.save_state()?
        };
        round.start_session(self, local_state, first_packet, shadow_snapshot)?;
        Ok(())
    }

    /// Called from the primary round-ending trap. Tears down the in-progress
    /// round (if any), drives the shadow forward to its matching round end,
    /// and emits the `EndOfRound` marker so the peer's receive loop can
    /// disambiguate subsequent inputs. Straggler inputs for the just-ended
    /// round die in the queue: the next round drains only entries tagged
    /// with its own index.
    fn end_round(&self) -> anyhow::Result<()> {
        let settled_shadow = {
            let mut round_state = self.round_state.lock().unwrap();
            let Some(round) = round_state.take() else {
                return Ok(());
            };
            log::info!("round ended at {:x}", round.frontier());
            round.settled_shadow_snapshot().cloned()
        };
        {
            let mut shadow = self.shadow.lock().unwrap();
            // Re-anchor the shadow to the authoritative settled tick before its
            // round-end advance. The simulator may have parked it ahead on a
            // speculative tick; loading the settled snapshot reproduces the
            // forward-only position `advance_until_round_end` expects.
            if let Some(snapshot) = settled_shadow.as_ref() {
                shadow.load_state(snapshot)?;
            }
            shadow.advance_until_round_end()?;
        }
        self.local_round_idx.fetch_add(1, Ordering::Release);
        self.sender.lock().unwrap().send(&crate::net::Event::EndOfRound)?;
        Ok(())
    }

    /// [`end_round`](Self::end_round) with the uniform primary-trap error
    /// policy applied: a failure logs and cancels the match (a panic would
    /// abort the process from trap context). Takes the trap's core so a
    /// training drill loop can intercept the end and reset the rep in
    /// place instead (never armed in real PvP).
    pub(crate) fn end_round_or_cancel(&self, core: mgba::core::CoreMutRef) {
        if self.try_training_round_end_reset(core) {
            return;
        }
        if let Err(e) = self.end_round() {
            log::error!("end round failed: {e:#}");
            self.cancel();
        }
    }

    /// [`start_round`](Self::start_round) for trap context, applying the
    /// same log + cancel error policy.
    pub(crate) fn start_round_or_cancel(self: &Arc<Self>) {
        if let Err(e) = self.start_round() {
            log::error!("start round failed: {e:#}");
            self.cancel();
        }
    }

    /// Network receive loop. Pure producer: stamps each remote input with
    /// the peer-round it belongs to and queues it; the live round drains the
    /// queue each frame. Never touches round state, so it can't contend
    /// with a trap mid-fastforward. When the queue is full, `push` parks
    /// this task — we stop reading the socket and the peer's sends back up
    /// in their buffer, same flow control as before.
    pub async fn run(&self, mut receiver: Box<dyn crate::net::Receiver + Send + Sync>) -> anyhow::Result<()> {
        loop {
            match receiver.receive().await? {
                crate::net::Event::Input(input) => {
                    // Tag at receive time so an input that arrived before a
                    // later EndOfRound still attaches to its original round,
                    // not whichever round the peer is in by drain time.
                    let peer_round = self.peer_round_idx.load(Ordering::Acquire);
                    match &*self.remote_source {
                        RemoteSource::Peer(queue) => queue.push(peer_round, input).await,
                        // The receive loop never runs on a training match.
                        RemoteSource::Training(_) => {}
                    }
                }
                crate::net::Event::EndOfRound => {
                    self.peer_round_idx.fetch_add(1, Ordering::Release);
                }
            }
        }
    }

    pub(crate) fn lock_round_state(&self) -> std::sync::MutexGuard<'_, Option<Round>> {
        self.round_state.lock().unwrap()
    }

    pub(crate) fn lock_rng(&self) -> std::sync::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.rng.lock().unwrap()
    }

    pub(crate) fn match_type(&self) -> (u8, u8) {
        self.identity.match_type
    }

    pub(crate) fn is_offerer(&self) -> bool {
        self.identity.is_offerer
    }

    /// Allocates a new [`Round`] in the round_state.
    fn start_round(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut round_state = self.round_state.lock().unwrap();
        log::info!(
            "starting round: local_player_index = {}",
            self.identity.local_player_index
        );

        // Mark a new round in the replay file. The body is a stream of
        // marker-prefixed records, so no count is needed up front.
        if let Some(writer) = self.replay_writer.lock().unwrap().as_mut() {
            writer.start_round()?;
        }

        log::info!("preparing round state");

        *round_state = Some(Round::new(self));
        log::info!("round has started");
        Ok(())
    }
}
