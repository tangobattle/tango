use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::input::{Input, Pair, PartialInput};

use super::types::{BattleOutcome, RoundPhase, RoundResult};

type InputPair = Pair<Input, Input>;
type PartialInputPair = Pair<PartialInput, PartialInput>;

type ApplyShadowInput = Box<dyn FnMut(u32, Pair<Input, PartialInput>) -> anyhow::Result<Vec<u8>> + Sync + Send>;

type SharedRng = Arc<Mutex<rand_pcg::Mcg128Xsl64>>;
type SharedShadow = Arc<Mutex<crate::shadow::Shadow>>;

/// `local_packet`'s payload bundled with the send count at which a consumer
/// should expect to see it. Setters record `output_pairs.len()` at the time
/// of the set; consumers verify it still matches at peek to catch
/// trap-ordering bugs.
#[derive(Clone)]
pub struct LocalPacket {
    send_count: u32,
    packet: Vec<u8>,
}

/// Replay-mode-only fields. None in Fastforwarder mode, where the run is
/// scoped to a single known input window with no inter-round transitions
/// or game-driven RNG seeding.
struct ReplayExtras {
    /// Multi-round queue. When the running round ends, the next round here
    /// gets loaded automatically.
    next_rounds: VecDeque<Vec<PartialInputPair>>,
    /// Shadow emulator that re-derives the remote peer's per-tick packets
    /// from the recorded remote joyflags. Driven through its own lifecycle
    /// methods at the round boundaries (first commit / round end).
    shadow: SharedShadow,
    /// Replay's shared RNG, seeded from the replay's rng_seed and pre-advanced
    /// to match `Match::new`'s draws.
    rng: SharedRng,
    /// Per-game replay traps use this to pick the correct rng1 stream.
    is_offerer: bool,
    /// Set true when `round_start_ret` has fired for the current round.
    /// Gates first commit so RNG isn't seeded before battle init completes.
    round_active: bool,
    /// Set true at first on_first_commit for the current round; reset by
    /// `load_replay_round` between replay rounds. Per-game traps gate the
    /// first-commit hook on this so it only fires once per round.
    has_committed_this_round: bool,
    /// Set true once `advance_until_round_end` has been called on the shadow
    /// for the current round; reset on `load_replay_round`. Guards against
    /// firing the round-end advance twice (e.g. round_ending_entry1 and
    /// round_ending_entry2 both triggering it in BN1/BN2).
    shadow_round_ended: bool,
    /// Monotonic tick counter across all replay rounds. Equal to
    /// `sum(rounds[..current_round].len()) + current_tick` while a round is
    /// in progress. Used by the replay UI to drive the seek bar.
    absolute_tick: u32,
    /// Total number of input pairs across all replay rounds, computed once
    /// at construction. Used as the seek bar's max.
    total_replay_ticks: u32,
    /// Index of the round currently in progress. Increments in
    /// [`InnerState::load_replay_round`].
    current_round_index: u32,
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
    /// Replay-mode-only: tick at which the per-round first-commit hook fires
    /// (seeds RNG, sets game tick to 0, advances shadow). `u32::MAX` in
    /// Fastforwarder mode (the per-game trap gates this branch on
    /// `is_replaying()` so the value is irrelevant there).
    commit_tick: u32,
    /// Tick at which the per-game stepper trap snapshots the state into
    /// [`captured_state`]. Captured *post-peek* (after `peek_input_pair` +
    /// `set_gpr(r4, ip.local.joyflags)`), so the saved state's r4 is the
    /// local joyflags for `current_tick` — suitable both for loading into the
    /// live core and for using as the next FF's base state. `u32::MAX` in
    /// replay mode so the capture never fires.
    capture_tick: u32,
    captured_state: Option<crate::battle::CommittedState>,
    round_result: Option<RoundResult>,
    phase: RoundPhase,
    error: Option<anyhow::Error>,

    /// Replay-mode-only state. None in Fastforwarder mode.
    replay: Option<ReplayExtras>,
}

/// Bundle of inputs to [`InnerState::for_replay`]. Used by both the fresh
/// [`State::new`] path (defaults for the carry-over fields) and
/// [`State::restore_replay_checkpoint`] (snapshot values).
struct ReplayInit {
    match_type: (u8, u8),
    local_player_index: u8,
    commit_tick: u32,
    rng: SharedRng,
    shadow: SharedShadow,
    is_offerer: bool,
    /// All remaining rounds, including the one currently in progress at
    /// front. The constructor pops the front and treats it as the active
    /// round; the rest become `next_rounds`.
    rounds: VecDeque<Vec<PartialInputPair>>,
    /// Inputs already played from the front round before this construction.
    /// 0 for fresh starts; non-zero when restoring a mid-round snapshot.
    inputs_consumed_in_current_round: u32,
    current_round_index: u32,
    absolute_tick: u32,
    total_replay_ticks: u32,
    current_tick_in_round: u32,
    /// Fresh start: None — local_packet is lazy-seeded by per-game stepper
    /// traps before the first send/receive consumes it (typically by
    /// reading the game's tx_packet at the first-commit point). Restore:
    /// Some — use the captured bytes from the checkpoint, since mid-round
    /// the active local_packet is whatever the previous send produced.
    local_packet_override: Option<Vec<u8>>,
    round_active: bool,
    has_committed_this_round: bool,
    shadow_round_ended: bool,
    disable_bgm: bool,
    on_round_ended: Option<Box<dyn FnOnce() + Send>>,
}

impl InnerState {
    /// Construct an InnerState for replay playback. Used by both the fresh
    /// [`State::new`] path and [`State::restore_replay_checkpoint`].
    fn for_replay(init: ReplayInit) -> Self {
        let ReplayInit {
            match_type,
            local_player_index,
            commit_tick,
            rng,
            shadow,
            is_offerer,
            mut rounds,
            inputs_consumed_in_current_round,
            current_round_index,
            absolute_tick,
            total_replay_ticks,
            current_tick_in_round,
            local_packet_override,
            round_active,
            has_committed_this_round,
            shadow_round_ended,
            disable_bgm,
            on_round_ended,
        } = init;

        let current_round = rounds.pop_front().unwrap_or_default();
        let consumed = (inputs_consumed_in_current_round as usize).min(current_round.len());
        let input_pairs: VecDeque<PartialInputPair> = current_round[consumed..].iter().cloned().collect();
        let next_rounds = rounds;

        // The remote packet for each tick is re-derived by running the
        // shadow emulator over the recorded remote joyflag (carried in
        // `ip.remote.joyflags`). The shadow handles its own per-tick
        // bookkeeping; this closure is just the bridge from the stepper's
        // `apply_shadow_input` call into the Shadow handle.
        let apply_shadow_input: ApplyShadowInput = {
            let shadow = shadow.clone();
            Box::new(move |tick, ip| shadow.lock().apply_input(tick, ip))
        };

        // send_count = 0 either way: fresh starts at tick 0 with empty
        // output_pairs, and restore resets output_pairs to empty too — both
        // sides of the send-counter check start fresh.
        let local_packet = local_packet_override.map(|packet| LocalPacket { send_count: 0, packet });

        Self {
            disable_bgm,
            current_tick: current_tick_in_round,
            local_player_index,
            match_type,
            input_pairs,
            output_pairs: vec![],
            apply_shadow_input,
            local_packet,
            commit_tick,
            // Replay mode never runs the FF capture branch.
            capture_tick: u32::MAX,
            captured_state: None,
            round_result: None,
            phase: RoundPhase::InProgress,
            error: None,
            replay: Some(ReplayExtras {
                next_rounds,
                shadow,
                rng,
                is_offerer,
                round_active,
                has_committed_this_round,
                shadow_round_ended,
                absolute_tick,
                total_replay_ticks,
                current_round_index,
                on_round_ended,
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
        capture_tick: u32,
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
                send_count: 0,
                packet: last_local_packet,
            }),
            // FF mode bypasses the replay-mode first-commit hook (the trap
            // branch is gated on `is_replaying()`), so `commit_tick` is unused.
            commit_tick: u32::MAX,
            capture_tick,
            captured_state: None,
            round_result: None,
            phase: RoundPhase::InProgress,
            error: None,
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

    // ----- tick / commit_tick / capture_tick -----

    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    pub fn commit_tick(&self) -> u32 {
        self.commit_tick
    }

    pub fn capture_tick(&self) -> u32 {
        self.capture_tick
    }

    pub fn increment_current_tick(&mut self) {
        // Replay-mode only: suppress increments before this round's first
        // commit. The game fires round_call_jump_table_ret during boot,
        // menu transitions, and inter-round animations; we mustn't let
        // those bump current_tick past commit_tick (= 0) before the round
        // actually starts. In Fastforwarder mode, every increment counts.
        if let Some(replay) = self.replay.as_mut() {
            if !replay.has_committed_this_round {
                return;
            }
            replay.absolute_tick += 1;
        }
        self.current_tick += 1;
    }

    /// Replay-mode monotonic tick counter across all queued rounds. 0 in
    /// Fastforwarder mode.
    pub fn absolute_tick(&self) -> u32 {
        self.replay.as_ref().map_or(0, |r| r.absolute_tick)
    }

    /// Total ticks across all replay rounds, computed once at construction.
    /// 0 in Fastforwarder mode.
    pub fn total_replay_ticks(&self) -> u32 {
        self.replay.as_ref().map_or(0, |r| r.total_replay_ticks)
    }

    /// Index of the round currently in progress (0-based). 0 in
    /// Fastforwarder mode.
    pub fn current_round_index(&self) -> u32 {
        self.replay.as_ref().map_or(0, |r| r.current_round_index)
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
            send_count: self.output_pairs.len() as u32,
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
            if p.send_count != expected {
                anyhow::bail!(
                    "local packet send mismatch: stored for send {}, current send {}",
                    p.send_count,
                    expected,
                );
            }
        }
        Ok(())
    }

    // ----- shadow input -----

    pub fn apply_shadow_input(&mut self, input: Pair<Input, PartialInput>) -> anyhow::Result<Vec<u8>> {
        let remote_packet = (self.apply_shadow_input)(self.current_tick, input.clone())?;
        self.output_pairs.push(Pair {
            local: input.local,
            remote: input.remote.with_packet(remote_packet.clone()),
        });
        Ok(remote_packet)
    }

    /// Per-tick output pairs accumulated since the last round transition.
    /// `load_replay_round` resets this to empty between replay rounds, so
    /// callers that need a full-replay record must drain it before the
    /// stepper observes the next round-start. Used by the replay regression
    /// harness to build a per-tick remote-packet digest.
    pub fn output_pairs(&self) -> &[Pair<Input, Input>] {
        &self.output_pairs
    }

    // ----- state capture -----

    /// Replay-mode only: per-round first-commit side effects (advance the
    /// shadow up to its matching first-committed snapshot so subsequent
    /// `apply_input` calls have a valid round_state). The local-packet
    /// `send_count` check guards against trap-ordering bugs. No state is
    /// stored here — the FF's single state capture happens at
    /// [`Self::capture_tick`] via [`Self::set_captured_state`].
    pub fn on_first_commit(&mut self) {
        let p = self.local_packet.clone().expect("local packet");
        let expected = self.output_pairs.len() as u32;
        if p.send_count != expected {
            panic!(
                "local packet send mismatch at first commit: stored for send {}, current send {}",
                p.send_count, expected,
            );
        }
        let shadow_to_advance = self.replay.as_mut().and_then(|replay| {
            let needs_advance = !replay.has_committed_this_round;
            replay.has_committed_this_round = true;
            needs_advance.then(|| replay.shadow.clone())
        });
        if let Some(shadow) = shadow_to_advance {
            if let Err(e) = shadow.lock().advance_until_first_committed_state() {
                self.error = Some(e);
            }
        }
    }

    /// Capture the FF's single state snapshot. Called by per-game stepper traps
    /// when `current_tick == capture_tick()` (post-peek, so r4 is set to the
    /// peeked input's local joyflags).
    pub fn set_captured_state(&mut self, state: Box<mgba::state::State>) {
        let p = self.local_packet.clone().expect("local packet");
        let expected = self.output_pairs.len() as u32;
        if p.send_count != expected {
            panic!(
                "local packet send mismatch at capture: stored for send {}, current send {}",
                p.send_count, expected,
            );
        }
        self.captured_state = Some(crate::battle::CommittedState {
            tick: self.current_tick,
            state,
            packet: p.packet,
        });
    }

    /// True iff the FF state snapshot has been captured. The Fastforwarder
    /// outer loop exits when this flips to true.
    pub(super) fn has_captured_state(&self) -> bool {
        self.captured_state.is_some()
    }

    /// Consumes self into a Fastforwarder result. Panics if the state hasn't
    /// been captured yet — callers must check [`Self::has_captured_state`] first.
    pub(super) fn into_fastforward_result(self) -> super::fastforwarder::FastforwardResult {
        super::fastforwarder::FastforwardResult {
            state: self.captured_state.expect("captured state"),
            round_result: self.round_result,
            output_pairs: self.output_pairs,
        }
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
        // Replay-mode only: drive the shadow forward to its matching
        // round-end snapshot now. This mirrors Match::end_round in PvP,
        // which calls shadow.advance_until_round_end from its
        // round_ending_entry trap. Guarded so the multi-entry games
        // (BN1/BN2 fire two round_ending_entry traps) only advance once.
        let shadow_to_advance = self.replay.as_mut().and_then(|replay| {
            if replay.shadow_round_ended {
                None
            } else {
                replay.shadow_round_ended = true;
                Some(replay.shadow.clone())
            }
        });
        if let Some(shadow) = shadow_to_advance {
            if let Err(e) = shadow.lock().advance_until_round_end() {
                self.error = Some(e);
            }
        }
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

    pub fn is_replaying(&self) -> bool {
        self.replay.is_some()
    }

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
    /// Always false in Fastforwarder mode.
    pub fn has_committed_this_round(&self) -> bool {
        self.replay.as_ref().is_some_and(|r| r.has_committed_this_round)
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

    /// Resets per-round state and loads the given round's inputs.
    fn load_replay_round(&mut self, round_inputs: Vec<PartialInputPair>) {
        self.current_tick = 0;
        // local_packet is lazy-seeded by per-game traps before the first
        // send/receive in the new round — the game's tx_packet will hold
        // the bg byte etc. set during the inter-round comm-menu phase.
        self.local_packet = None;

        {
            let replay = self.replay.as_mut().expect("load_replay_round in FF mode");
            replay.round_active = true;
            replay.current_round_index += 1;
            replay.has_committed_this_round = false;
            replay.shadow_round_ended = false;
        }

        self.input_pairs = round_inputs.into_iter().collect();
        self.output_pairs = vec![];
        // Replay mode never sets captured_state, so resetting here is just
        // defensive; included for symmetry with the other per-round fields.
        self.captured_state = None;
        self.round_result = None;
        self.phase = RoundPhase::InProgress;
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
    pub local_packet: Option<LocalPacket>,
    pub inputs_consumed: u32,
}

/// Full replay-playback snapshot. Bundles the stepper's [`ReplayCheckpoint`]
/// with the matching stepper-core `mgba::state::State` and the shadow side's
/// [`shadow::ShadowSnapshot`] so a single load restores both cores together.
/// Restoring only the stepper would leave the shadow at its pre-seek tick
/// and feed misaligned packets through the subsequent apply_input chain.
#[derive(Clone)]
pub struct ReplaySnapshot {
    pub checkpoint: ReplayCheckpoint,
    pub mgba_state: Box<mgba::state::State>,
    pub shadow_snapshot: crate::shadow::ShadowSnapshot,
}

/// Shared handle to the [`InnerState`]. Per-game traps clone this and lock
/// it inside their closures via [`State::lock_inner`].
#[derive(Clone)]
pub struct State(pub(super) Arc<Mutex<Option<InnerState>>>);

impl State {
    /// Construct a replay-mode stepper state covering one or more rounds.
    /// Rounds are played back in order; `set_round_ended` advances to the
    /// next one until the queue is empty, then fires `on_round_ended`.
    /// `total_replay_ticks` is the input-pair count across the full replay
    /// (used by the seek bar). `shadow` is the shadow emulator that
    /// re-derives the remote peer's per-tick packets from the recorded
    /// remote joyflags.
    pub fn new(
        match_type: (u8, u8),
        local_player_index: u8,
        rounds: Vec<Vec<PartialInputPair>>,
        commit_tick: u32,
        rng_seed: [u8; 16],
        is_offerer: bool,
        total_replay_ticks: u32,
        shadow: SharedShadow,
        on_round_ended: Box<dyn FnOnce() + Send>,
    ) -> State {
        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(rng_seed);
        // Match::new advances the shared rng by one bool draw before any
        // game traps fire (the polite-win pick). Stay in sync.
        let _ = rand::Rng::gen::<bool>(&mut rng);
        let rng = Arc::new(Mutex::new(rng));

        let inner = InnerState::for_replay(ReplayInit {
            match_type,
            local_player_index,
            commit_tick,
            rng,
            shadow,
            is_offerer,
            rounds: VecDeque::from(rounds),
            inputs_consumed_in_current_round: 0,
            current_round_index: 0,
            absolute_tick: 0,
            total_replay_ticks,
            current_tick_in_round: 0,
            local_packet_override: None,
            round_active: false,
            has_committed_this_round: false,
            shadow_round_ended: false,
            disable_bgm: false,
            on_round_ended: Some(on_round_ended),
        });

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
            absolute_tick: replay.absolute_tick,
            current_round_index: replay.current_round_index,
            current_tick_in_round: inner.current_tick,
            has_committed_this_round: replay.has_committed_this_round,
            rng_state,
            local_packet: inner.local_packet.clone(),
            inputs_consumed: inner.output_pairs.len() as u32,
        })
    }

    /// Swap the inner state with one positioned at `checkpoint`. The caller
    /// must also load the matching mgba state onto the core; without it the
    /// stepper will desync immediately. `rounds` must be the same full round
    /// list originally passed to [`State::new`]. The existing `on_round_ended`
    /// callback and shadow handle are preserved across the swap.
    pub fn restore_replay_checkpoint(
        &self,
        checkpoint: &ReplayCheckpoint,
        rounds: &[Vec<PartialInputPair>],
    ) -> anyhow::Result<()> {
        let mut guard = self.0.lock();
        let prev = guard.as_mut().ok_or_else(|| anyhow::anyhow!("stepper state missing"))?;
        let prev_replay = prev
            .replay
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("not in replay mode"))?;

        let on_round_ended = prev_replay.on_round_ended.take();
        let is_offerer = prev_replay.is_offerer;
        let total_replay_ticks = prev_replay.total_replay_ticks;
        let shadow = prev_replay.shadow.clone();
        let match_type = prev.match_type;
        let local_player_index = prev.local_player_index;
        let commit_tick = prev.commit_tick;
        let disable_bgm = prev.disable_bgm;

        let round_idx = checkpoint.current_round_index as usize;
        if round_idx >= rounds.len() {
            anyhow::bail!(
                "checkpoint round_index {} out of range (have {} rounds)",
                round_idx,
                rounds.len()
            );
        }

        let rounds_from_current: VecDeque<Vec<PartialInputPair>> = rounds[round_idx..].iter().cloned().collect();
        let rng = Arc::new(Mutex::new(checkpoint.rng_state.clone()));

        let new_inner = InnerState::for_replay(ReplayInit {
            match_type,
            local_player_index,
            commit_tick,
            rng,
            shadow,
            is_offerer,
            rounds: rounds_from_current,
            inputs_consumed_in_current_round: checkpoint.inputs_consumed,
            current_round_index: checkpoint.current_round_index,
            absolute_tick: checkpoint.absolute_tick,
            total_replay_ticks,
            current_tick_in_round: checkpoint.current_tick_in_round,
            local_packet_override: checkpoint.local_packet.as_ref().map(|lp| lp.packet.clone()),
            round_active: true,
            has_committed_this_round: checkpoint.has_committed_this_round,
            // The shadow handle is shared across the swap; if the previous
            // run already advanced shadow past the round-end for this
            // round, leave that flag set so we don't double-advance.
            shadow_round_ended: false,
            disable_bgm,
            on_round_ended,
        });

        *guard = Some(new_inner);
        Ok(())
    }
}
