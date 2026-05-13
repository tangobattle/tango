use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::input::{Input, Pair, PartialInput};

use super::types::{BattleOutcome, RoundPhase, RoundResult};

type InputPair = Pair<Input, Input>;
type PartialInputPair = Pair<PartialInput, PartialInput>;

type ApplyShadowInput = Box<
    dyn FnMut(u32, Pair<Input, PartialInput>) -> anyhow::Result<Vec<u8>> + Sync + Send,
>;

type SharedRng = Arc<Mutex<rand_pcg::Mcg128Xsl64>>;
type SharedInputQueue = Arc<Mutex<VecDeque<InputPair>>>;

/// `local_packet`'s payload bundled with the tick at which a consumer should
/// expect to see it. Setters record `current_tick + 1`; consumers verify
/// `target_tick == current_tick` to catch trap-ordering bugs.
#[derive(Clone)]
struct LocalPacket {
    target_tick: u32,
    packet: Vec<u8>,
}

/// Replay-mode-only fields. None in Fastforwarder mode, where the run is
/// scoped to a single known input window with no inter-round transitions
/// or game-driven RNG seeding.
struct ReplayExtras {
    /// Multi-round queue. When the running round ends, the next round here
    /// gets loaded automatically.
    next_rounds: VecDeque<Vec<InputPair>>,
    /// Backing storage for the `apply_shadow_input` closure. Refilled when
    /// advancing to the next round.
    remote_inputs: SharedInputQueue,
    /// Replay's shared RNG, seeded from the replay's rng_seed and pre-advanced
    /// to match `Match::new`'s draws.
    rng: SharedRng,
    /// Per-game replay traps use this to pick the correct rng1 stream.
    is_offerer: bool,
    /// Set true when `round_start_ret` has fired for the current round.
    /// Gates first commit so RNG isn't seeded before battle init completes.
    round_active: bool,
    /// Fired when the last queued round ends.
    on_round_ended: Option<Box<dyn FnOnce() + Send>>,
}

pub struct InnerState {
    disable_bgm: bool,
    current_tick: u32,
    local_player_index: u8,
    match_type: (u8, u8),
    input_pairs: VecDeque<PartialInputPair>,
    output_pairs: Vec<InputPair>,
    apply_shadow_input: ApplyShadowInput,
    local_packet: Option<LocalPacket>,
    commit_tick: u32,
    committed_state: Option<crate::battle::CommittedState>,
    dirty_tick: u32,
    dirty_state: Option<crate::battle::CommittedState>,
    round_result: Option<RoundResult>,
    phase: RoundPhase,
    error: Option<anyhow::Error>,
    /// Set true at first set_committed_state for the current round; reset by
    /// `load_replay_round` between replay rounds. Per-game traps gate
    /// first-commit work on this so it only fires once per round.
    has_committed_this_round: bool,
    /// Monotonic tick counter across all replay rounds. Equal to
    /// `sum(rounds[..current_round].len()) + current_tick` while a round is
    /// in progress. Used by the replay UI to drive the seek bar. Always 0
    /// in Fastforwarder mode.
    absolute_tick: u32,
    /// Total number of input pairs across all replay rounds, computed once
    /// at construction. Used as the seek bar's max. 0 in Fastforwarder mode.
    total_replay_ticks: u32,
    /// Index of the round currently in progress. Increments in
    /// [`load_replay_round`]. 0 in Fastforwarder mode and for the first
    /// replay round.
    current_round_index: u32,

    /// Replay-mode-only state. None in Fastforwarder mode.
    replay: Option<ReplayExtras>,
}

impl InnerState {
    /// Construct an InnerState for replay playback. Wired up by [`State::new`].
    /// `start_round_index` and `start_absolute_tick` are 0 for a fresh replay,
    /// and non-zero when resuming from a snapshot mid-replay.
    /// `total_replay_ticks` is the input count across the *full* replay
    /// (not just the rounds passed in here).
    fn for_replay(
        match_type: (u8, u8),
        local_player_index: u8,
        commit_tick: u32,
        rng: SharedRng,
        is_offerer: bool,
        first_round: Vec<InputPair>,
        next_rounds: VecDeque<Vec<InputPair>>,
        start_round_index: u32,
        start_absolute_tick: u32,
        total_replay_ticks: u32,
        on_round_ended: Box<dyn FnOnce() + Send>,
    ) -> Self {
        let remote_inputs: SharedInputQueue =
            Arc::new(Mutex::new(first_round.iter().cloned().collect()));

        let apply_shadow_input: ApplyShadowInput = {
            let queue = remote_inputs.clone();
            Box::new(move |_tick, _ip| {
                let Some(ip) = queue.lock().pop_front() else {
                    anyhow::bail!("no more committed inputs");
                };
                Ok(ip.remote.packet)
            })
        };

        let local_packet = first_round.first().map(|ip| LocalPacket {
            target_tick: 0,
            packet: ip.local.packet.clone(),
        });
        let input_pairs = first_round.into_iter().map(into_partial_pair).collect();

        Self {
            disable_bgm: false,
            current_tick: 0,
            local_player_index,
            match_type,
            input_pairs,
            output_pairs: vec![],
            apply_shadow_input,
            local_packet,
            commit_tick,
            committed_state: None,
            dirty_tick: 0,
            dirty_state: None,
            round_result: None,
            phase: RoundPhase::InProgress,
            error: None,
            has_committed_this_round: false,
            absolute_tick: start_absolute_tick,
            total_replay_ticks,
            current_round_index: start_round_index,
            replay: Some(ReplayExtras {
                next_rounds,
                remote_inputs,
                rng,
                is_offerer,
                round_active: false,
                on_round_ended: Some(on_round_ended),
            }),
        }
    }

    /// Construct an InnerState for a Fastforwarder run. Wired up by
    /// [`super::Fastforwarder::fastforward`].
    pub(super) fn for_fastforward(
        match_type: (u8, u8),
        local_player_index: u8,
        input_pairs: Vec<PartialInputPair>,
        current_tick: u32,
        commit_tick: u32,
        dirty_tick: u32,
        last_local_packet: Vec<u8>,
        apply_shadow_input: ApplyShadowInput,
    ) -> Self {
        Self {
            disable_bgm: false,
            current_tick,
            local_player_index,
            match_type,
            input_pairs: input_pairs.into_iter().collect(),
            output_pairs: vec![],
            apply_shadow_input,
            local_packet: Some(LocalPacket {
                // target_tick = output_pairs.len() at this send. We start at
                // 0 (no sends yet) and the first send's check expects 0.
                target_tick: 0,
                packet: last_local_packet,
            }),
            commit_tick,
            committed_state: None,
            dirty_tick,
            dirty_state: None,
            round_result: None,
            phase: RoundPhase::InProgress,
            error: None,
            has_committed_this_round: false,
            absolute_tick: 0,
            total_replay_ticks: 0,
            current_round_index: 0,
            replay: None,
        }
    }

    // ----- error / disable_bgm / metadata -----

    pub fn take_error(&mut self) -> Option<anyhow::Error> {
        self.error.take()
    }

    pub fn set_anyhow_error(&mut self, err: anyhow::Error) {
        self.error = Some(err);
    }

    pub fn disable_bgm(&self) -> bool {
        self.disable_bgm
    }

    pub fn set_disable_bgm(&mut self, disable_bgm: bool) {
        self.disable_bgm = disable_bgm;
    }

    pub fn match_type(&self) -> (u8, u8) {
        self.match_type
    }

    pub fn local_player_index(&self) -> u8 {
        self.local_player_index
    }

    pub fn remote_player_index(&self) -> u8 {
        1 - self.local_player_index
    }

    // ----- tick / commit_tick / dirty_tick -----

    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    pub fn commit_tick(&self) -> u32 {
        self.commit_tick
    }

    pub fn dirty_tick(&self) -> u32 {
        self.dirty_tick
    }

    pub fn increment_current_tick(&mut self) {
        // Replay-mode only: suppress increments before this round's first
        // commit. The game fires round_call_jump_table_ret during boot,
        // menu transitions, and inter-round animations; we mustn't let
        // those bump current_tick past commit_tick (= 0) before the round
        // actually starts. In Fastforwarder mode, every increment counts.
        if self.replay.is_some() && !self.has_committed_this_round {
            return;
        }
        self.current_tick += 1;
        if self.replay.is_some() {
            self.absolute_tick += 1;
        }
    }

    /// Replay-mode monotonic tick counter across all queued rounds. 0 in
    /// Fastforwarder mode.
    pub fn absolute_tick(&self) -> u32 {
        self.absolute_tick
    }

    /// Total ticks across all replay rounds, computed once at construction.
    /// 0 in Fastforwarder mode.
    pub fn total_replay_ticks(&self) -> u32 {
        self.total_replay_ticks
    }

    /// Index of the round currently in progress (0-based).
    pub fn current_round_index(&self) -> u32 {
        self.current_round_index
    }

    // ----- input pair queue -----

    pub fn peek_input_pair(&self) -> Option<&PartialInputPair> {
        self.input_pairs.front()
    }

    pub fn pop_input_pair(&mut self) -> Option<PartialInputPair> {
        self.input_pairs.pop_front()
    }

    pub fn input_pairs_left(&self) -> usize {
        self.input_pairs.len()
    }

    /// Inputs remaining across the current round and all queued future rounds.
    pub fn total_input_pairs_left(&self) -> usize {
        let queued = self
            .replay
            .as_ref()
            .map_or(0, |r| r.next_rounds.iter().map(|round| round.len()).sum());
        self.input_pairs.len() + queued
    }

    // ----- local packet (this emulator's tx_packet from the previous tick) -----

    pub fn set_local_packet(&mut self, packet: Vec<u8>) {
        // Tag the stored packet with the current send count
        // (= output_pairs.len(), which is incremented by apply_shadow_input
        // earlier in this send's trap). The consumer at the next send will
        // see send count = same value and the check matches.
        //
        // We deliberately do NOT use self.current_tick here: in games where
        // the frame layout fires `round_call_jump_table_ret` more than once
        // per send (e.g. BN3), `current_tick` advances by more than 1 between
        // sends and the old `current_tick + 1` formula no longer matches the
        // consumer side.
        self.local_packet = Some(LocalPacket {
            target_tick: self.output_pairs.len() as u32,
            packet,
        });
    }

    pub fn peek_local_packet(&mut self) -> Option<&[u8]> {
        self.local_packet.as_ref().map(|p| p.packet.as_slice())
    }

    /// Verify the buffered local packet was queued for the current send
    /// (i.e. `set_local_packet` was called once between this peek and the
    /// previous one). Per-game stepper traps call this before consuming the
    /// packet to catch missing trap calls.
    ///
    /// The "tick" being compared is the per-send counter (output_pairs.len()),
    /// not the per-frame current_tick — `apply_shadow_input` pushes to
    /// output_pairs once per send, so checking against its length is a clean
    /// proxy for "send number" that's independent of how many
    /// `round_call_jump_table_ret` fires the game emits between sends.
    pub fn check_local_packet_at_current_tick(&self) -> anyhow::Result<()> {
        if let Some(p) = self.local_packet.as_ref() {
            let expected = self.output_pairs.len() as u32;
            if p.target_tick != expected {
                anyhow::bail!(
                    "local packet send mismatch: stored for send {}, current send {}",
                    p.target_tick,
                    expected,
                );
            }
        }
        Ok(())
    }

    // ----- shadow input -----

    pub fn apply_shadow_input(
        &mut self,
        input: Pair<Input, PartialInput>,
    ) -> anyhow::Result<Vec<u8>> {
        let remote_packet = (self.apply_shadow_input)(self.current_tick, input.clone())?;
        self.output_pairs.push(Pair {
            local: input.local,
            remote: input.remote.with_packet(remote_packet.clone()),
        });
        Ok(remote_packet)
    }

    // ----- committed / dirty save snapshots -----

    pub fn set_committed_state(&mut self, state: Box<mgba::state::State>) {
        let p = self.local_packet.clone().expect("local packet");
        let expected = self.output_pairs.len() as u32;
        if p.target_tick != expected {
            panic!(
                "local packet send mismatch at commit: stored for send {}, current send {}",
                p.target_tick, expected,
            );
        }
        self.committed_state = Some(crate::battle::CommittedState {
            tick: self.current_tick,
            state,
            packet: p.packet,
        });
        self.has_committed_this_round = true;
    }

    pub fn take_committed_state(&mut self) -> Option<crate::battle::CommittedState> {
        self.committed_state.take()
    }

    /// True iff a committed state has been captured for the current run.
    pub(super) fn has_committed_state_snapshot(&self) -> bool {
        self.committed_state.is_some()
    }

    /// True iff a dirty state has been captured for the current run.
    pub(super) fn has_dirty_state_snapshot(&self) -> bool {
        self.dirty_state.is_some()
    }

    /// Consumes self into a Fastforwarder result. Panics if either the
    /// committed or dirty state hasn't been set yet — callers must check via
    /// [`InnerState::has_committed_state_snapshot`] / [`has_dirty_state_snapshot`]
    /// first.
    pub(super) fn into_fastforward_result(self) -> super::fastforwarder::FastforwardResult {
        super::fastforwarder::FastforwardResult {
            committed_state: self.committed_state.expect("committed state"),
            dirty_state: self.dirty_state.expect("dirty state"),
            round_result: self.round_result,
            output_pairs: self.output_pairs,
        }
    }

    pub fn set_dirty_state(&mut self, state: Box<mgba::state::State>) {
        let p = self.local_packet.clone().expect("local packet");
        let expected = self.output_pairs.len() as u32;
        if p.target_tick != expected {
            panic!(
                "local packet send mismatch at dirty: stored for send {}, current send {}",
                p.target_tick, expected,
            );
        }
        self.dirty_state = Some(crate::battle::CommittedState {
            tick: self.current_tick,
            state,
            packet: p.packet,
        });
    }

    // ----- round phase / outcome -----

    pub fn set_round_result(&mut self, outcome: BattleOutcome) {
        self.round_result = Some(RoundResult {
            tick: self.current_tick,
            outcome,
        });
    }

    pub fn round_result(&self) -> Option<RoundResult> {
        self.round_result
    }

    pub fn set_round_ending(&mut self) {
        self.phase = RoundPhase::Ending;
    }

    pub fn set_round_ended(&mut self) {
        self.phase = RoundPhase::Ended;
        // Replay-mode only: fire on_round_ended when the last queued round
        // ends. In Fastforwarder mode there's no callback to run.
        if let Some(replay) = self.replay.as_mut() {
            if replay.next_rounds.is_empty() {
                if let Some(callback) = replay.on_round_ended.take() {
                    callback();
                }
            }
        }
    }

    pub fn is_round_ending(&self) -> bool {
        self.phase == RoundPhase::Ending || self.phase == RoundPhase::Ended
    }

    pub fn is_round_ended(&self) -> bool {
        self.phase == RoundPhase::Ended
    }

    // ----- replay-mode accessors -----
    //
    // These return Option / sensible Fastforwarder-mode defaults so per-game
    // stepper traps can use them unconditionally.

    /// Returns the replay-mode RNG, if this stepper is in replay mode.
    pub fn replay_rng(&self) -> Option<&SharedRng> {
        self.replay.as_ref().map(|r| &r.rng)
    }

    pub fn replay_is_offerer(&self) -> bool {
        self.replay.as_ref().is_some_and(|r| r.is_offerer)
    }

    /// True iff the current round has had its first commit. Used by per-game
    /// stepper traps to gate per-frame work that would otherwise diverge from
    /// the game's tick during boot / inter-round animations in replay mode.
    pub fn has_committed_this_round(&self) -> bool {
        self.has_committed_this_round
    }

    /// True iff round_start_ret has fired for the current round. In FF mode
    /// this is always true (FF resumes from a known committed state).
    pub fn round_active(&self) -> bool {
        self.replay.as_ref().map_or(true, |r| r.round_active)
    }

    // ----- replay-mode round transitions -----

    /// Called by per-game replay traps from round_start_ret. If a previous
    /// round just ended and another round is queued, load it. For the first
    /// round (phase still InProgress), this is a no-op since the round was
    /// loaded in [`State::new`]. Either way, marks the round as active so the
    /// first-commit gate in main_read_joyflags can fire.
    ///
    /// No-op in Fastforwarder mode.
    pub fn advance_to_next_replay_round_if_pending(&mut self) {
        if self.replay.is_none() {
            return;
        }
        let next_round = if self.phase == RoundPhase::Ended {
            self.replay.as_mut().unwrap().next_rounds.pop_front()
        } else {
            None
        };
        match next_round {
            Some(round_inputs) => self.load_replay_round(round_inputs),
            None => self.replay.as_mut().unwrap().round_active = true,
        }
    }

    /// Resets per-round state and loads the given round's inputs. The
    /// shared remote_inputs queue (held by `apply_shadow_input`) is refilled
    /// with the new round's remote inputs.
    fn load_replay_round(&mut self, round_inputs: Vec<InputPair>) {
        self.current_tick = 0;
        self.current_round_index += 1;
        self.local_packet = round_inputs.first().map(|ip| LocalPacket {
            target_tick: 0,
            packet: ip.local.packet.clone(),
        });

        {
            let replay = self.replay.as_mut().expect("load_replay_round in FF mode");
            let mut q = replay.remote_inputs.lock();
            q.clear();
            q.extend(round_inputs.iter().cloned());
            drop(q);
            replay.round_active = true;
        }

        self.input_pairs = round_inputs.into_iter().map(into_partial_pair).collect();
        self.output_pairs = vec![];
        self.committed_state = None;
        self.dirty_state = None;
        self.round_result = None;
        self.phase = RoundPhase::InProgress;
        self.has_committed_this_round = false;
    }
}

/// Drop the packet payload, keeping only the joyflags from each input pair.
fn into_partial_pair(ip: InputPair) -> PartialInputPair {
    Pair {
        local: PartialInput { joyflags: ip.local.joyflags },
        remote: PartialInput { joyflags: ip.remote.joyflags },
    }
}

/// Snapshot of the bits of [`InnerState`] needed to resume a replay at a
/// specific point. Capture via [`State::capture_replay_checkpoint`], restore
/// via [`State::restore_replay_checkpoint`]. The mgba core state must be
/// captured / loaded by the caller alongside this — without a matching
/// `mgba::state::State` the restored stepper will desync immediately.
#[derive(Clone)]
pub struct ReplayCheckpoint {
    pub absolute_tick: u32,
    pub current_round_index: u32,
    pub current_tick_in_round: u32,
    pub has_committed_this_round: bool,
    pub rng_state: rand_pcg::Mcg128Xsl64,
    /// `(target_tick, packet_bytes)` of the buffered local_packet at capture
    /// time. Captured directly so restore can rebuild the exact LocalPacket
    /// without trying to derive it from `current_tick_in_round` and the
    /// recorded round (which is game-frame-ordering-specific and produces
    /// the wrong index for games whose frame layout shifts ticks relative
    /// to send_and_receive).
    pub local_packet: Option<(u32, Vec<u8>)>,
    /// Number of input pairs consumed from the current round so far. Restore
    /// drops this many entries from the front of the round when rebuilding
    /// the input queue. Captured directly because it isn't always equal to
    /// `current_tick_in_round` — games whose frame layout puts multiple tick
    /// increments between input pairs (e.g. BN3) consume one pair per
    /// `send_and_receive`, not one per stepper tick.
    pub inputs_consumed: u32,
}

/// Shared handle to the [`InnerState`]. Per-game traps clone this and lock
/// it inside their closures via [`State::lock_inner`].
#[derive(Clone)]
pub struct State(pub(super) Arc<Mutex<Option<InnerState>>>);

impl State {
    /// Construct a replay-mode stepper state covering one or more rounds.
    /// Rounds are played back in order; `set_round_ended` advances to the
    /// next one until the queue is empty, then fires `on_round_ended`.
    ///
    /// `start_round_index` and `start_absolute_tick` are normally 0. Pass
    /// non-zero values to resume mid-replay from a snapshot — the caller
    /// is responsible for also load_state'ing the matching mgba snapshot
    /// onto the core, and for passing only `rounds[start_round_index..]`
    /// here. `total_replay_ticks` is the input-pair count across the
    /// *full* replay (used by the seek bar).
    pub fn new(
        match_type: (u8, u8),
        local_player_index: u8,
        rounds: Vec<Vec<InputPair>>,
        commit_tick: u32,
        rng_seed: [u8; 16],
        is_offerer: bool,
        start_round_index: u32,
        start_absolute_tick: u32,
        total_replay_ticks: u32,
        on_round_ended: Box<dyn FnOnce() + Send>,
    ) -> State {
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(rng_seed);
        // Match::new advances the shared rng by one bool draw before any
        // game traps fire (the polite-win pick). Stay in sync.
        let _ = rand::Rng::gen::<bool>(&mut rng);
        let rng = Arc::new(Mutex::new(rng));

        let mut rounds = VecDeque::from(rounds);
        let first_round = rounds.pop_front().unwrap_or_default();

        let inner = InnerState::for_replay(
            match_type,
            local_player_index,
            commit_tick,
            rng,
            is_offerer,
            first_round,
            rounds,
            start_round_index,
            start_absolute_tick,
            total_replay_ticks,
            on_round_ended,
        );

        State(Arc::new(Mutex::new(Some(inner))))
    }

    pub fn lock_inner(&self) -> parking_lot::MappedMutexGuard<'_, InnerState> {
        parking_lot::MutexGuard::map(self.0.lock(), |s| s.as_mut().unwrap())
    }

    /// Capture a checkpoint of replay-mode stepper state for later restore.
    /// Returns None outside replay mode or before round_start_ret has fired.
    pub fn capture_replay_checkpoint(&self) -> Option<ReplayCheckpoint> {
        let inner = self.lock_inner();
        let replay = inner.replay.as_ref()?;
        if !replay.round_active {
            return None;
        }
        let rng_state = replay.rng.lock().clone();
        Some(ReplayCheckpoint {
            absolute_tick: inner.absolute_tick,
            current_round_index: inner.current_round_index,
            current_tick_in_round: inner.current_tick,
            has_committed_this_round: inner.has_committed_this_round,
            rng_state,
            local_packet: inner.local_packet.as_ref().map(|p| (p.target_tick, p.packet.clone())),
            inputs_consumed: inner.output_pairs.len() as u32,
        })
    }

    /// Swap the inner state with one positioned at `checkpoint`. The caller
    /// must also load the matching mgba state onto the core; without it the
    /// stepper will desync immediately. `rounds` must be the same full round
    /// list originally passed to [`State::new`]. The existing `on_round_ended`
    /// callback is preserved across the swap.
    pub fn restore_replay_checkpoint(
        &self,
        checkpoint: &ReplayCheckpoint,
        rounds: &[Vec<InputPair>],
    ) -> anyhow::Result<()> {
        let mut guard = self.0.lock();
        let prev = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("stepper state missing"))?;
        let prev_replay = prev
            .replay
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("not in replay mode"))?;

        let on_round_ended = prev_replay.on_round_ended.take();
        let is_offerer = prev_replay.is_offerer;
        let match_type = prev.match_type;
        let local_player_index = prev.local_player_index;
        let commit_tick = prev.commit_tick;
        let total_replay_ticks = prev.total_replay_ticks;
        let disable_bgm = prev.disable_bgm;

        let round_idx = checkpoint.current_round_index as usize;
        if round_idx >= rounds.len() {
            anyhow::bail!(
                "checkpoint round_index {} out of range (have {} rounds)",
                round_idx,
                rounds.len()
            );
        }
        let current_round = &rounds[round_idx];
        let next_rounds: VecDeque<Vec<InputPair>> =
            rounds[round_idx + 1..].iter().cloned().collect();

        let consumed = (checkpoint.inputs_consumed as usize).min(current_round.len());
        let remaining_inputs: Vec<InputPair> = current_round[consumed..].to_vec();

        let remote_inputs: SharedInputQueue =
            Arc::new(Mutex::new(remaining_inputs.iter().cloned().collect()));
        let apply_shadow_input: ApplyShadowInput = {
            let queue = remote_inputs.clone();
            Box::new(move |_tick, _ip| {
                let Some(ip) = queue.lock().pop_front() else {
                    anyhow::bail!("no more committed inputs");
                };
                Ok(ip.remote.packet)
            })
        };

        // Restore the captured local_packet bytes. target_tick is reset to
        // 0 because output_pairs is reset to empty on restore — both
        // sides of the send-counter check are starting fresh, and the
        // captured bytes are still the right thing to feed at the next
        // send.
        let local_packet = checkpoint
            .local_packet
            .as_ref()
            .map(|(_captured_target_tick, packet)| LocalPacket {
                target_tick: 0,
                packet: packet.clone(),
            });

        let input_pairs: VecDeque<PartialInputPair> =
            remaining_inputs.into_iter().map(into_partial_pair).collect();

        let rng = Arc::new(Mutex::new(checkpoint.rng_state.clone()));

        let new_inner = InnerState {
            disable_bgm,
            current_tick: checkpoint.current_tick_in_round,
            local_player_index,
            match_type,
            input_pairs,
            output_pairs: vec![],
            apply_shadow_input,
            local_packet,
            commit_tick,
            committed_state: None,
            dirty_tick: 0,
            dirty_state: None,
            round_result: None,
            phase: RoundPhase::InProgress,
            error: None,
            has_committed_this_round: checkpoint.has_committed_this_round,
            absolute_tick: checkpoint.absolute_tick,
            total_replay_ticks,
            current_round_index: checkpoint.current_round_index,
            replay: Some(ReplayExtras {
                next_rounds,
                remote_inputs,
                rng,
                is_offerer,
                round_active: true,
                on_round_ended,
            }),
        };

        *guard = Some(new_inner);
        Ok(())
    }
}
