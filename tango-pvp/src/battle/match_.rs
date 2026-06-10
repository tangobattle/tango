use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as SyncMutex};

use tokio::sync::Mutex;

use super::round::{Round, MAX_QUEUE_LENGTH};
use super::{MatchIdentity, ReplayConfig};

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
    pub local_frame_advantage: i16,
    /// The peer's most recently reported view of the same, from their side.
    pub remote_frame_advantage: i16,
    /// Speculative frames the most recent step discarded and re-simulated
    /// because a confirmed remote input contradicted the prediction.
    pub misprediction_depth: u32,
}

/// Connection-level state for a single PvP match.
pub struct Match {
    shadow: Arc<SyncMutex<crate::shadow::Shadow>>,
    rom: Vec<u8>,
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    sender: Arc<Mutex<Box<dyn crate::net::Sender + Send + Sync>>>,
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
    primary_thread_handle: mgba::thread::Handle,
    /// Handoff queue from the net receive task to the live round.
    remote_inputs: Arc<RemoteInputs>,
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
}

impl Match {
    pub fn new(
        rom: Vec<u8>,
        local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        primary_thread_handle: mgba::thread::Handle,
        sender: Box<dyn crate::net::Sender + Send + Sync>,
        cancellation_token: tokio_util::sync::CancellationToken,
        rng: rand_pcg::Mcg128Xsl64,
        shadow: crate::shadow::Shadow,
        identity: MatchIdentity,
        replay: ReplayConfig,
        frame_delay: Arc<AtomicU32>,
    ) -> Arc<Self> {
        Arc::new(Self {
            shadow: Arc::new(SyncMutex::new(shadow)),
            local_hooks,
            rom,
            sender: Arc::new(Mutex::new(sender)),
            rng: SyncMutex::new(rng),
            cancellation_token,
            identity,
            round_state: SyncMutex::new(None),
            primary_thread_handle,
            remote_inputs: Arc::default(),
            local_round_idx: AtomicU32::new(0),
            peer_round_idx: AtomicU32::new(0),
            replay_writer: Arc::new(SyncMutex::new(replay.writer)),
            frame_delay,
        })
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

    pub(super) fn sender_handle(&self) -> Arc<Mutex<Box<dyn crate::net::Sender + Send + Sync>>> {
        self.sender.clone()
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

    pub(super) fn remote_inputs_handle(&self) -> Arc<RemoteInputs> {
        self.remote_inputs.clone()
    }

    /// The local round counter a [`Round`] created right now belongs to.
    pub(super) fn current_local_round_idx(&self) -> u32 {
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

    pub fn cancelled(&self) -> tokio_util::sync::WaitForCancellationFuture<'_> {
        self.cancellation_token.cancelled()
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
        let sender = self.sender.clone();
        crate::sync::block_on(async move { sender.lock().await.send(&crate::net::Event::EndOfRound).await })?;
        Ok(())
    }

    /// [`end_round`](Self::end_round) with the uniform primary-trap error
    /// policy applied: a failure logs and cancels the match (a panic would
    /// abort the process from trap context).
    pub(crate) fn end_round_or_cancel(&self) {
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
                    self.remote_inputs.push(peer_round, input).await;
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
