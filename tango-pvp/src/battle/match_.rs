use std::sync::Arc;

use parking_lot::Mutex as PlMutex;
use tokio::sync::{watch, Mutex};

use super::round::Round;
use super::types::{MatchIdentity, ReplayConfig, RoundState};

/// Outcome of a single attempt to attach a remote input to the current round.
enum Attach {
    Added,
    Dropped,
    WaitForProgress,
}

/// Connection-level state for a single PvP match.
pub struct Match {
    shadow: Arc<PlMutex<crate::shadow::Shadow>>,
    rom: Vec<u8>,
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    sender: Arc<Mutex<Box<dyn crate::net::Sender + Send + Sync>>>,
    rng: Mutex<rand_pcg::Mcg128Xsl64>,
    cancellation_token: tokio_util::sync::CancellationToken,
    identity: MatchIdentity,
    round_state: Mutex<RoundState>,
    primary_thread_handle: mgba::thread::Handle,
    /// Bumped whenever the round lifecycle advances (start_round, first
    /// commit, end_round). The network receive loop awaits changes on this
    /// to know when it can hand a remote input off to the in-progress round.
    round_progress: watch::Sender<u64>,
    replay_writer: Arc<PlMutex<Option<crate::replay::Writer>>>,
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
            round_state: Mutex::new(RoundState {
                number: 0,
                round: None,
            }),
            primary_thread_handle,
            round_progress,
            replay_writer: Arc::new(PlMutex::new(replay.writer)),
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

    /// Picks the per-match local_player_index. Both peers must call this with
    /// the same shared RNG state at the same point in the protocol so they end
    /// up on opposite sides. Advances the RNG by one draw.
    pub fn pick_local_player_index(rng: &mut rand_pcg::Mcg128Xsl64, is_offerer: bool) -> u8 {
        use rand::Rng;
        let did_polite_win = rng.gen::<bool>();
        if did_polite_win == is_offerer { 0 } else { 1 }
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
    /// and wakes the network receive loop so any straggler inputs for the
    /// just-ended round can be dropped.
    pub fn end_round(&self) -> anyhow::Result<()> {
        let mut round_state = self.round_state.blocking_lock();
        if round_state.round.is_none() {
            return Ok(());
        }
        round_state.end_round()?;
        self.shadow.lock().advance_until_round_end()?;
        self.bump_round_progress();
        Ok(())
    }

    fn bump_round_progress(&self) {
        self.round_progress.send_modify(|n| *n += 1);
    }

    /// Network receive loop: pulls remote inputs off the receiver and queues
    /// them into the in-progress round, blocking on round-progress changes
    /// until the round exists and has its first committed state.
    pub async fn run(&self, mut receiver: Box<dyn crate::net::Receiver + Send + Sync>) -> anyhow::Result<()> {
        let mut progress = self.round_progress.subscribe();
        loop {
            let input = receiver.receive().await?;
            self.deliver_remote_input(&mut progress, input).await?;
        }
    }

    /// Wait until either we can attach `input` to the in-progress round or we
    /// can confidently drop it (round is already over). Awaits round-progress
    /// changes between attempts.
    async fn deliver_remote_input(
        &self,
        progress: &mut watch::Receiver<u64>,
        input: crate::net::Input,
    ) -> anyhow::Result<()> {
        loop {
            // Borrow-and-update marks the current value as "seen" so the next
            // `changed().await` only fires on a genuinely later state.
            progress.borrow_and_update();

            match self.try_attach_remote_input(&input).await? {
                Attach::Added | Attach::Dropped => return Ok(()),
                Attach::WaitForProgress => {}
            }

            if progress.changed().await.is_err() {
                // Sender dropped — match is shutting down.
                return Ok(());
            }
        }
    }

    async fn try_attach_remote_input(&self, input: &crate::net::Input) -> anyhow::Result<Attach> {
        let mut round_state = self.round_state.lock().await;
        let current_number = round_state.number;

        // Round number drifted past us, or the input arrived after the round
        // was torn down (round.is_none() but round_state.number hasn't yet
        // been incremented for the next round): drop on the floor.
        if current_number > input.round_number
            || (current_number == input.round_number && round_state.round.is_none())
        {
            log::info!("dropping input for finished round {}", input.round_number);
            return Ok(Attach::Dropped);
        }

        if current_number != input.round_number {
            // Input is for a future round we haven't started yet.
            return Ok(Attach::WaitForProgress);
        }

        let Some(round) = round_state.round.as_mut() else {
            return Ok(Attach::WaitForProgress);
        };
        if !round.has_committed_state() {
            // Round started but hasn't reached its first commit.
            return Ok(Attach::WaitForProgress);
        }

        if !round.can_add_remote_input() {
            anyhow::bail!("remote overflowed our input buffer");
        }
        round.add_remote_input(crate::input::PartialInput {
            joyflags: input.joyflags as u16,
        });
        Ok(Attach::Added)
    }

    pub fn lock_round_state(&self) -> tokio::sync::MutexGuard<'_, RoundState> {
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

    /// Allocates a new [`Round`] in the round_state, fills `input_delay`
    /// frames of zero-input padding into its queue, and bumps round_progress
    /// so the network receive loop wakes up to (re-)evaluate.
    pub async fn start_round(self: &Arc<Self>) -> anyhow::Result<()> {
        let mut round_state = self.round_state.lock().await;
        round_state.number += 1;
        log::info!("starting round: local_player_index = {}", self.identity.local_player_index);

        // Mark a new round in the replay file. The body is a stream of
        // marker-prefixed records, so no count is needed up front.
        if let Some(writer) = self.replay_writer.lock().as_mut() {
            writer.start_round()?;
        }

        log::info!("preparing round state");

        const MAX_QUEUE_LENGTH: usize = 300;
        let mut iq = crate::input::PairQueue::new(MAX_QUEUE_LENGTH, self.identity.input_delay);
        log::info!("filling {} ticks of input delay", self.identity.input_delay);

        {
            let mut sender = self.sender.lock().await;
            for _ in 0..self.identity.input_delay {
                iq.add_local_input(crate::input::PartialInput { joyflags: 0 });
                sender
                    .send(&crate::net::Input {
                        round_number: round_state.number,
                        joyflags: 0,
                    })
                    .await?;
            }
        }

        round_state.round = Some(Round::new(self, round_state.number, iq)?);
        self.bump_round_progress();
        log::info!("round has started");
        Ok(())
    }
}
