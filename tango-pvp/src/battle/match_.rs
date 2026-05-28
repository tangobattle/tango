use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::Mutex as PlMutex;
use tokio::sync::{watch, Mutex};

use super::present::PresentationBuffer;
use super::round::Round;
use super::throttler::Throttler;
use super::types::{MatchIdentity, ReplayConfig};

/// Builds a fresh per-round throttler on demand. Owned by the Match so
/// the round-start path can construct a new throttler for each round
/// without the caller having to know which strategy is in use.
pub type ThrottlerFactory = Box<dyn Fn() -> Box<dyn Throttler> + Send + Sync>;

/// Connection-level state for a single PvP match.
pub struct Match {
    shadow: Arc<PlMutex<crate::shadow::Shadow>>,
    rom: Vec<u8>,
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    sender: Arc<Mutex<Box<dyn crate::net::Sender + Send + Sync>>>,
    rng: Mutex<rand_pcg::Mcg128Xsl64>,
    cancellation_token: tokio_util::sync::CancellationToken,
    identity: MatchIdentity,
    round_state: Mutex<Option<Round>>,
    primary_thread_handle: mgba::thread::Handle,
    /// Bumped whenever the round lifecycle advances (start_round, first
    /// commit, end_round). The network receive loop awaits changes on this
    /// to know when it can hand a remote input off to the in-progress round.
    round_progress: watch::Sender<u64>,
    /// Count of local `end_round` calls. Each remote Input is tagged with
    /// `peer_round_idx` at receive time; on attach we compare against this
    /// counter so a stale tail from a round we've already closed gets
    /// dropped and a peer who's raced ahead is held back until we catch up.
    local_round_idx: AtomicU32,
    /// Count of `EndOfRound` packets received from the peer. Loaded at
    /// receive time to stamp each Input with the round it belongs to.
    peer_round_idx: AtomicU32,
    replay_writer: Arc<PlMutex<Option<crate::replay::Writer>>>,
    /// Per-round throttler factory. Re-invoked at every `start_round`
    /// so the active strategy can be swapped between rounds via
    /// [`Self::set_throttler_factory`].
    throttler_factory: PlMutex<ThrottlerFactory>,
    /// Live → display hand-off. The live `Round` publishes the FF's delayed
    /// `present_state` here each frame; the display core renders it.
    presentation: Arc<PlMutex<PresentationBuffer>>,
    /// Shared input delay in frames: `min(local_frame_delay, peer_frame_delay)`.
    /// Both peers compute the same value, apply it symmetrically (local input
    /// ring + remote-queue prefill in the `Round`), and so each side's rollback
    /// depth drops by this much. Part of the deterministic simulation — fixed
    /// for the whole match.
    input_delay: u32,
    /// Local presentation delay in frames: `local_frame_delay − input_delay`.
    /// The leftover of this side's requested delay that couldn't be cashed in as
    /// shared input delay; realized purely locally by the `Round` rendering
    /// `frontier − presentation_delay`. Never touches the netcode or the wire.
    presentation_delay: u32,
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
        throttler_factory: ThrottlerFactory,
        input_delay: u32,
        presentation_delay: u32,
    ) -> Arc<Self> {
        let (round_progress, _) = watch::channel(0);
        Arc::new(Self {
            shadow: Arc::new(PlMutex::new(shadow)),
            local_hooks,
            rom,
            sender: Arc::new(Mutex::new(sender)),
            rng: Mutex::new(rng),
            cancellation_token,
            identity,
            round_state: Mutex::new(None),
            primary_thread_handle,
            round_progress,
            local_round_idx: AtomicU32::new(0),
            peer_round_idx: AtomicU32::new(0),
            replay_writer: Arc::new(PlMutex::new(replay.writer)),
            throttler_factory: PlMutex::new(throttler_factory),
            presentation: Arc::new(PlMutex::new(PresentationBuffer::new())),
            input_delay,
            presentation_delay,
        })
    }

    /// Replace the throttler factory. The factory is used at every
    /// future `start_round`; if `apply_to_current_round` is true and
    /// a round is in flight, also swap the live round's throttler now
    /// (resetting its accumulated state — the new strategy starts fresh).
    pub async fn set_throttler_factory(&self, factory: ThrottlerFactory, apply_to_current_round: bool) {
        let live_throttler = if apply_to_current_round { Some(factory()) } else { None };
        *self.throttler_factory.lock() = factory;
        if let Some(throttler) = live_throttler {
            if let Some(round) = self.round_state.lock().await.as_mut() {
                round.set_throttler(throttler);
            }
        }
    }

    pub(super) fn build_throttler(&self) -> Box<dyn Throttler> {
        (self.throttler_factory.lock())()
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

    pub(super) fn shadow_handle(&self) -> Arc<PlMutex<crate::shadow::Shadow>> {
        self.shadow.clone()
    }

    pub(super) fn sender_handle(&self) -> Arc<Mutex<Box<dyn crate::net::Sender + Send + Sync>>> {
        self.sender.clone()
    }

    pub(super) fn replay_writer_handle(&self) -> Arc<PlMutex<Option<crate::replay::Writer>>> {
        self.replay_writer.clone()
    }

    pub(super) fn primary_thread_handle(&self) -> mgba::thread::Handle {
        self.primary_thread_handle.clone()
    }

    pub(super) fn presentation_handle(&self) -> Arc<PlMutex<PresentationBuffer>> {
        self.presentation.clone()
    }

    pub(super) fn input_delay(&self) -> u32 {
        self.input_delay
    }

    pub(super) fn presentation_delay(&self) -> u32 {
        self.presentation_delay
    }

    /// Shared presentation buffer for the display side. The PvP session hands
    /// this to the display core so it can render the delayed `present_state`
    /// the live core publishes each frame.
    pub fn presentation(&self) -> Arc<PlMutex<PresentationBuffer>> {
        self.presentation.clone()
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
        let writer = self.replay_writer.lock().take();
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
    /// matching first commit, snapshots local state on the round, and wakes
    /// the network receive loop.
    pub fn record_first_commit(
        &self,
        round: &mut Round,
        local_state: Box<mgba::state::State>,
        first_packet: &[u8],
    ) -> anyhow::Result<()> {
        self.shadow.lock().advance_until_first_committed_state()?;
        round.set_first_committed_state(local_state, first_packet);
        self.bump_round_progress();
        Ok(())
    }

    /// Called from the primary round-ending trap. Tears down the in-progress
    /// round (if any), drives the shadow forward to its matching round end,
    /// emits the `EndOfRound` marker so the peer's receive loop can
    /// disambiguate subsequent inputs, and wakes our own receive loop so
    /// any straggler inputs for the just-ended round can be dropped.
    pub fn end_round(&self) -> anyhow::Result<()> {
        {
            let mut round_state = self.round_state.blocking_lock();
            let Some(round) = round_state.take() else {
                return Ok(());
            };
            log::info!("round ended at {:x}", round.current_tick());
        }
        self.shadow.lock().advance_until_round_end()?;
        // Bump BEFORE sending so a racing remote-Input arrival is compared
        // against the up-to-date local_round_idx.
        self.local_round_idx.fetch_add(1, Ordering::Release);
        let sender = self.sender.clone();
        crate::sync::block_on(async move { sender.lock().await.send_end_of_round().await })?;
        self.bump_round_progress();
        Ok(())
    }

    fn bump_round_progress(&self) {
        self.round_progress.send_modify(|n| *n += 1);
    }

    /// Network receive loop: pulls events off the receiver and either
    /// queues remote inputs into the in-progress round or bumps the
    /// `peer_round_idx` counter (on `EndOfRound`). Blocks on round-progress
    /// changes when the round isn't ready to accept inputs yet.
    pub async fn run(&self, mut receiver: Box<dyn crate::net::Receiver + Send + Sync>) -> anyhow::Result<()> {
        let mut progress = self.round_progress.subscribe();
        loop {
            match receiver.receive().await? {
                crate::net::Event::Input(input) => {
                    // Tag at receive time so a held input that arrived
                    // before a later EndOfRound still attaches to its
                    // original round, not whichever round the peer is
                    // in by the time the deliver loop wakes up.
                    let peer_round = self.peer_round_idx.load(Ordering::Acquire);
                    self.deliver_remote_input(&mut progress, input, peer_round).await?;
                }
                crate::net::Event::EndOfRound => {
                    self.peer_round_idx.fetch_add(1, Ordering::Release);
                    // Wake the deliver loop so any held inputs whose peer
                    // round now matches a future local round get re-checked,
                    // and stale-round drops happen promptly.
                    self.bump_round_progress();
                }
            }
        }
    }

    /// Wait until we can attach `input` to the in-progress round, or drop
    /// it if it belongs to a round we've already closed. Awaits
    /// round-progress changes between attempts.
    async fn deliver_remote_input(
        &self,
        progress: &mut watch::Receiver<u64>,
        input: crate::net::Input,
        peer_round: u32,
    ) -> anyhow::Result<()> {
        loop {
            // Borrow-and-update marks the current value as "seen" so the next
            // `changed().await` only fires on a genuinely later state.
            progress.borrow_and_update();

            if self.try_attach_remote_input(&input, peer_round).await? {
                return Ok(());
            }

            if progress.changed().await.is_err() {
                // Sender dropped — match is shutting down.
                return Ok(());
            }
        }
    }

    /// Decide what to do with `input`, tagged with the peer's `peer_round`
    /// at receive time:
    /// - peer ended their round before we ended ours (`peer_round < local`):
    ///   stale tail, drop.
    /// - peer is in our current round (`peer_round == local`) and round
    ///   state is ready: attach.
    /// - otherwise: hold, wait for round progress.
    async fn try_attach_remote_input(&self, input: &crate::net::Input, peer_round: u32) -> anyhow::Result<bool> {
        let local_round = self.local_round_idx.load(Ordering::Acquire);
        if peer_round < local_round {
            // Tail-of-previous-round input that arrived after we already
            // ended round-N locally. The round it belonged to is gone;
            // discard rather than poisoning round-N+1's queue.
            return Ok(true);
        }
        if peer_round > local_round {
            // Peer raced ahead into a future round; hold until our local
            // end_round catches up.
            return Ok(false);
        }

        let mut round_state = self.round_state.lock().await;
        let Some(round) = round_state.as_mut() else {
            // Either before the first round has started or between rounds —
            // wait for the next round to spin up before delivering the input.
            return Ok(false);
        };
        if !round.has_committed_state() {
            // Round started but hasn't reached its first commit.
            return Ok(false);
        }

        if !round.can_add_remote_input() {
            anyhow::bail!("remote overflowed our input buffer");
        }
        round.add_remote_input(input.clone());
        Ok(true)
    }

    pub fn lock_round_state(&self) -> tokio::sync::MutexGuard<'_, Option<Round>> {
        self.round_state.blocking_lock()
    }

    pub fn lock_rng(&self) -> tokio::sync::MutexGuard<'_, rand_pcg::Mcg128Xsl64> {
        self.rng.blocking_lock()
    }

    pub fn match_type(&self) -> (u8, u8) {
        self.identity.match_type
    }

    pub fn is_offerer(&self) -> bool {
        self.identity.is_offerer
    }

    /// Allocates a new [`Round`] in the round_state and bumps round_progress
    /// so the network receive loop wakes up to (re-)evaluate.
    pub async fn start_round(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut round_state = self.round_state.lock().await;
        log::info!(
            "starting round: local_player_index = {}",
            self.identity.local_player_index
        );

        // Mark a new round in the replay file. The body is a stream of
        // marker-prefixed records, so no count is needed up front.
        if let Some(writer) = self.replay_writer.lock().as_mut() {
            writer.start_round()?;
        }

        log::info!("preparing round state");

        const MAX_QUEUE_LENGTH: usize = 120;
        let iq = crate::input::PairQueue::new(MAX_QUEUE_LENGTH);

        *round_state = Some(Round::new(self, iq)?);
        self.bump_round_progress();
        log::info!("round has started");
        Ok(())
    }
}
