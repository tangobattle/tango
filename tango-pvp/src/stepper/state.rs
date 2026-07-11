use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::input::{Input, PartialInput};

use super::{BattleOutcome, RoundPhase, RoundResult};

type InputPair = (Input, Input);
type PartialInputPair = (PartialInput, PartialInput);

/// Shared handle to the opponent co-sim. [`State::new_for_replay`] hands one
/// out alongside the stepper state; the seek/prefetch paths keep it to
/// snapshot and restore the shadow in lockstep with the stepper.
pub type SharedShadow = Arc<Mutex<crate::shadow::Shadow>>;

/// `local_packet`'s payload bundled with the send count at which a consumer
/// should expect to see it. Setters record `output_pairs.len()` at the time
/// of the set; consumers verify it still matches at peek to catch
/// trap-ordering bugs.
#[derive(Clone)]
pub struct LocalPacket {
    send_count: u32,
    packet: Vec<u8>,
}

/// Where replay playback is in its per-round lifecycle. One explicit machine
/// instead of the old `round_active` + `has_committed_this_round` +
/// `shadow_round_ended` booleans plus a shared `RoundPhase`:
///
/// ```text
/// AwaitingRoundStart ──round_start_ret──► AwaitingFirstCommit
/// AwaitingFirstCommit ──on_first_commit──► InRound          (+ shadow first-commit advance)
/// (pre-ending) ──set_round_ending──► RoundEnding            (+ shadow round-end advance, on this edge only)
/// RoundEnding ──set_round_ended──► RoundEnded               (bn2 defers this to match_end_ret)
/// RoundEnded ──round_start_ret──► AwaitingFirstCommit       (next queued round loads)
/// RoundEnded ──round_start_ret──► Finished                  (queue empty; game runs on, ticks frozen)
/// ```
///
/// The shadow advances fire on the *edges*, which is what absorbs BN1/BN2's
/// double `round_ending_entry` fires (second `set_round_ending` finds the
/// phase already `RoundEnding` and does nothing) — the job the old
/// `shadow_round_ended` flag did.
#[derive(Clone, Copy, PartialEq)]
enum PlaybackPhase {
    /// Game hasn't reached `round_start_ret` for this round yet. The
    /// pre-round `main_read_joyflags` fires (boot, menus, inter-round
    /// animations) return early on this.
    AwaitingRoundStart,
    /// Round started; waiting for `current_tick` to reach the commit
    /// frontier so the first-commit hook can anchor tick 0.
    AwaitingFirstCommit,
    InRound,
    RoundEnding,
    RoundEnded,
    /// The game started another round but the replay queue is empty. Ticks
    /// freeze and `is_round_ended()` stays true so the host's termination
    /// polling fires; the game keeps running underneath.
    Finished,
}

impl PlaybackPhase {
    /// True from the first commit onward — the phases where game ticks count.
    fn has_committed(self) -> bool {
        matches!(
            self,
            PlaybackPhase::InRound | PlaybackPhase::RoundEnding | PlaybackPhase::RoundEnded | PlaybackPhase::Finished
        )
    }

    /// True once `round_start_ret` has fired for the current round.
    fn round_active(self) -> bool {
        self != PlaybackPhase::AwaitingRoundStart
    }

    /// True once the round has begun ending (or has ended / finished).
    fn has_ended_or_is_ending(self) -> bool {
        matches!(
            self,
            PlaybackPhase::RoundEnding | PlaybackPhase::RoundEnded | PlaybackPhase::Finished
        )
    }
}

/// State for [`Mode::Replay`]: multi-round playback from boot, with
/// game-driven RNG seeding at each round's first commit and shadow lifecycle
/// driven at the round boundaries.
struct ReplayExtras {
    /// Per-round lifecycle. See [`PlaybackPhase`].
    phase: PlaybackPhase,
    /// Tick at which the per-round first-commit hook fires (seeds RNG, sets
    /// game tick to 0, advances shadow). Compared against `current_tick` by
    /// [`InnerState::needs_replay_first_commit`].
    commit_frontier: u32,
    /// Multi-round queue. When the running round ends, the next round here
    /// gets loaded automatically.
    next_rounds: VecDeque<Vec<PartialInputPair>>,
    /// Shadow emulator that re-derives the remote peer's per-tick packets
    /// from the recorded remote joyflags. Driven through its own lifecycle
    /// methods at the round boundaries (first commit / round end).
    shadow: SharedShadow,
    /// Replay's match RNG, seeded from the replay's rng_seed and pre-advanced
    /// to match `Match::new`'s draws. Only ever drawn from under the
    /// [`InnerState`] lock (per-game stepper traps).
    rng: rand_pcg::Mcg128Xsl64,
    /// Per-game replay traps use this to pick the correct rng1 stream.
    is_offerer: bool,
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

/// The boundary recorded when the FF reaches `capture_tick`: just the tick and
/// this side's outgoing packet. The matching mgba state is *not* stored — the
/// core is left parked at the boundary and the caller materializes it on demand
/// via [`Stepper::save`](super::Stepper::save), so a rollback that re-steps a
/// whole tail saves only the one state it keeps. Returned to the caller as part
/// of [`StepperResult`](super::StepperResult).
pub struct CapturedBoundary {
    /// The tick the boundary is poised at (the tick the game is about to process
    /// next, one past the tick just simulated).
    pub tick: u32,
    /// This side's outgoing link packet at the boundary — seeds the next step's
    /// link exchange.
    pub packet: Vec<u8>,
}

/// State for [`Mode::Fastforward`]: a single re-sim run over a known input
/// window, ending in a boundary capture at `capture_tick`.
struct FastforwardExtras {
    /// Tick at which the per-game stepper trap halts the core at the boundary,
    /// recording [`Self::captured`]. The input window is exhausted by then, so the
    /// halting `main_read_joyflags` finds no pair to peek and leaves the core
    /// poised at the start of `current_tick` with r4 left unset (the matching mgba
    /// state is materialized on demand by
    /// [`Stepper::save`](super::Stepper::save)). The consumer supplies the local
    /// joyflags: the live core via `Hooks::inject_joyflags_on_primary`,
    /// the next FF by re-priming r4 at its first `main_read_joyflags`.
    capture_tick: u32,
    captured: Option<CapturedBoundary>,
    /// Round-ending progression within this single re-sim window. The FF path
    /// flows through the same `set_round_ending` / `is_round_ending` trap
    /// gates as replay; [`InnerState::round_result`] is how the result
    /// reaches the rollback engine.
    ending: RoundPhase,
}

/// Which of the stepper's two jobs this state is driving. The per-game stepper
/// traps serve both; they branch on [`InnerState::is_replaying`] and the
/// mode-specific predicates ([`InnerState::needs_replay_first_commit`],
/// [`InnerState::at_capture_tick`]) rather than inspecting this directly.
enum Mode {
    /// Replay playback from boot: one or more recorded rounds, no rollback.
    /// Built by [`State::new`] and [`State::restore_replay_checkpoint`].
    Replay(ReplayExtras),
    /// One [`Stepper::step`](super::Stepper::step) for the rollback engine:
    /// re-simulate exactly one known input window from a committed state and
    /// capture the boundary at the end. Built by
    /// [`InnerState::for_fastforward`]. No inter-round transitions or
    /// game-driven RNG seeding happen here.
    Fastforward(FastforwardExtras),
}

pub struct InnerState {
    disable_bgm: bool,
    current_tick: u32,
    local_player_index: u8,
    match_type: (u8, u8),
    input_pairs: VecDeque<PartialInputPair>,
    output_pairs: Vec<InputPair>,
    packet_source: Arc<dyn super::RemotePacketSource>,
    local_packet: Option<LocalPacket>,
    round_result: Option<RoundResult>,
    /// Latest per-tick HP report from the per-game traps (see
    /// [`super::BattleHp`]). Overwritten every reporting tick; stays `None`
    /// for games without HP offsets.
    battle_hp: Option<super::BattleHp>,
    /// Latest per-tick custom-screen report from the per-game traps —
    /// `Some(true)` while either player has the chip-select open. Stays
    /// `None` for games without the flag offsets wired.
    battle_custom: Option<bool>,
    /// Latest per-tick loaded-chip report from the per-game traps —
    /// each player's queued chip id (0xFFFF = none). Stays `None` for
    /// games without the chip offsets wired.
    battle_chips: Option<[u16; 2]>,
    error: Option<anyhow::Error>,
    mode: Mode,
}

/// Bundle of inputs to [`InnerState::for_replay`]. Used by both the fresh
/// [`State::new`] path (defaults for the carry-over fields) and
/// [`State::restore_replay_checkpoint`] (snapshot values).
struct ReplayInit {
    match_type: (u8, u8),
    local_player_index: u8,
    commit_frontier: u32,
    rng: rand_pcg::Mcg128Xsl64,
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
    /// Fresh start: [`PlaybackPhase::AwaitingRoundStart`]. Restore: derived
    /// from the checkpoint's `has_committed_this_round` — never an ending
    /// phase, so the shadow round-end advance can re-fire (the shadow
    /// snapshot is restored alongside by the seek path).
    phase: PlaybackPhase,
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
            commit_frontier,
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
            phase,
            disable_bgm,
            on_round_ended,
        } = init;

        let current_round = rounds.pop_front().unwrap_or_default();
        let consumed = (inputs_consumed_in_current_round as usize).min(current_round.len());
        let input_pairs: VecDeque<PartialInputPair> = current_round[consumed..].iter().cloned().collect();
        let next_rounds = rounds;

        // The remote packet for each tick is re-derived by running the
        // shadow emulator over the recorded remote joyflag (carried in
        // `ip.remote.joyflags`); the shared Shadow handle is the
        // [`RemotePacketSource`](super::RemotePacketSource).
        let packet_source: Arc<dyn super::RemotePacketSource> = shadow.clone();

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
            packet_source,
            local_packet,
            round_result: None,
            battle_hp: None,
            battle_custom: None,
            battle_chips: None,
            error: None,
            mode: Mode::Replay(ReplayExtras {
                phase,
                commit_frontier,
                next_rounds,
                shadow,
                rng,
                is_offerer,
                absolute_tick,
                total_replay_ticks,
                current_round_index,
                on_round_ended,
            }),
        }
    }

    /// Construct an InnerState for a Fastforwarder run. Wired up by
    /// [`Stepper::step`](super::Stepper::step).
    pub(super) fn for_fastforward(
        match_type: (u8, u8),
        local_player_index: u8,
        inputs: Vec<PartialInputPair>,
        current_tick: u32,
        last_local_packet: Vec<u8>,
        packet_source: Arc<dyn super::RemotePacketSource>,
        disable_bgm: bool,
    ) -> Self {
        // Run `inputs` one tick each, then capture at the boundary tick (one past
        // the last applied). By then the input window is exhausted, so the
        // capturing `main_read_joyflags` finds no pair to peek and snapshots
        // poised at the start of that tick with r4 left unset — the consumer
        // injects the boundary tick's local joyflags.
        let capture_tick = current_tick + inputs.len() as u32;
        let input_pairs: VecDeque<PartialInputPair> = inputs.into_iter().collect();
        Self {
            disable_bgm,
            current_tick,
            local_player_index,
            match_type,
            input_pairs,
            output_pairs: vec![],
            packet_source,
            local_packet: Some(LocalPacket {
                // target_tick = output_pairs.len() at this send. We start at
                // 0 (no sends yet) and the first send's check expects 0.
                send_count: 0,
                packet: last_local_packet,
            }),
            round_result: None,
            battle_hp: None,
            battle_custom: None,
            battle_chips: None,
            error: None,
            mode: Mode::Fastforward(FastforwardExtras {
                capture_tick,
                captured: None,
                ending: RoundPhase::InProgress,
            }),
        }
    }

    // ----- mode access -----

    fn replay(&self) -> Option<&ReplayExtras> {
        match &self.mode {
            Mode::Replay(replay) => Some(replay),
            Mode::Fastforward(_) => None,
        }
    }

    fn replay_mut(&mut self) -> Option<&mut ReplayExtras> {
        match &mut self.mode {
            Mode::Replay(replay) => Some(replay),
            Mode::Fastforward(_) => None,
        }
    }

    fn fastforward(&self) -> Option<&FastforwardExtras> {
        match &self.mode {
            Mode::Fastforward(ff) => Some(ff),
            Mode::Replay(_) => None,
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

    // ----- tick / first-commit / capture predicates -----

    pub fn current_tick(&self) -> u32 {
        self.current_tick
    }

    /// True iff this is a replay whose current round is active but hasn't
    /// committed yet, and `current_tick` has reached the round's commit
    /// frontier. Per-game `main_read_joyflags` traps gate the first-commit
    /// hook ([`Self::on_first_commit`]) on this. Always false in
    /// Fastforwarder mode.
    pub fn needs_replay_first_commit(&self) -> bool {
        self.replay().is_some_and(|replay| {
            replay.phase == PlaybackPhase::AwaitingFirstCommit && self.current_tick == replay.commit_frontier
        })
    }

    /// True iff this is a Fastforwarder run that has reached its boundary
    /// tick — the input window is exhausted and the per-game trap should
    /// record the boundary ([`Self::capture`]) and halt the core there.
    /// Always false in replay mode.
    pub fn at_capture_tick(&self) -> bool {
        self.fastforward()
            .is_some_and(|ff| self.current_tick == ff.capture_tick)
    }

    pub fn increment_current_tick(&mut self) {
        // Replay-mode only: suppress increments before this round's first
        // commit. The game fires round_call_jump_table_ret during boot,
        // menu transitions, and inter-round animations; we mustn't let
        // those bump current_tick past commit_frontier (= 0) before the round
        // actually starts. In Fastforwarder mode, every increment counts.
        if let Mode::Replay(replay) = &mut self.mode {
            if !replay.phase.has_committed() {
                return;
            }
            replay.absolute_tick += 1;
        }
        self.current_tick += 1;
    }

    /// Replay-mode monotonic tick counter across all queued rounds. 0 in
    /// Fastforwarder mode.
    pub fn absolute_tick(&self) -> u32 {
        self.replay().map_or(0, |r| r.absolute_tick)
    }

    /// Total ticks across all replay rounds, computed once at construction.
    /// 0 in Fastforwarder mode.
    pub fn total_replay_ticks(&self) -> u32 {
        self.replay().map_or(0, |r| r.total_replay_ticks)
    }

    /// Index of the round currently in progress (0-based). 0 in
    /// Fastforwarder mode.
    pub fn current_round_index(&self) -> u32 {
        self.replay().map_or(0, |r| r.current_round_index)
    }

    /// True iff this is a replay still awaiting its very first round's start
    /// (the initial `AwaitingRoundStart` phase, before `round_start_ret`).
    /// This is the only round where the phase is `AwaitingRoundStart`, so the
    /// bn1 `round_start_entry` RNG seed gates on it to fire once per match —
    /// rounds 2+ inherit the game's carried-over rng. Always false in
    /// Fastforwarder mode.
    pub fn is_awaiting_round_start(&self) -> bool {
        self.replay()
            .is_some_and(|replay| replay.phase == PlaybackPhase::AwaitingRoundStart)
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
            .replay()
            .map_or(0, |r| r.next_rounds.iter().map(|round| round.len()).sum());
        self.input_pairs.len() + queued
    }

    /// Cumulative input pairs consumed across all rounds so far — the
    /// replay's recorded-frame index. Unlike [`Self::absolute_tick`], it
    /// does *not* advance during the input-less inter-round animation (no
    /// pair is popped then), so it maps statically onto the recorded round
    /// lengths. This is the seek bar's coordinate: its round marks are just
    /// cumulative round lengths, and the playhead reads exactly here. 0 in
    /// Fastforwarder mode.
    pub fn inputs_consumed(&self) -> u32 {
        self.total_replay_ticks()
            .saturating_sub(self.total_input_pairs_left() as u32)
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

    /// Resolve the remote packet for the current tick by co-simulating the
    /// shadow, recording the confirmed pair in `output_pairs`. On failure
    /// the error goes straight into the stepper's error channel — which the
    /// drive loops abort on — and this returns `None`; the per-game trap
    /// just ends its fire.
    pub fn apply_shadow_input(&mut self, input: (Input, PartialInput)) -> Option<Vec<u8>> {
        match self.packet_source.resolve(input.clone()) {
            Ok(remote_packet) => {
                let (local, remote) = input;
                self.output_pairs
                    .push((local, remote.with_packet(remote_packet.clone())));
                Some(remote_packet)
            }
            Err(e) => {
                self.set_anyhow_error(e);
                None
            }
        }
    }

    /// Per-tick output pairs accumulated since the last round transition.
    /// `load_replay_round` resets this to empty between replay rounds, so
    /// callers that need a full-replay record must drain it before the
    /// stepper observes the next round-start. Used by the replay regression
    /// harness to build a per-tick remote-packet digest.
    pub fn output_pairs(&self) -> &[(Input, Input)] {
        &self.output_pairs
    }

    // ----- state capture -----

    /// Replay-mode only: per-round first-commit side effects (advance the
    /// shadow up to its matching first-committed snapshot so subsequent
    /// `apply_input` calls have a valid round_state). The local-packet
    /// `send_count` check guards against trap-ordering bugs. No state is
    /// stored here — the FF's single state capture happens at
    /// [`Self::capture_tick`] via [`Self::capture`].
    pub fn on_first_commit(&mut self) {
        let p = self.local_packet.clone().expect("local packet");
        let expected = self.output_pairs.len() as u32;
        if p.send_count != expected {
            panic!(
                "local packet send mismatch at first commit: stored for send {}, current send {}",
                p.send_count, expected,
            );
        }
        let shadow_to_advance = self.replay_mut().and_then(|replay| {
            let needs_advance = !replay.phase.has_committed();
            if needs_advance {
                replay.phase = PlaybackPhase::InRound;
            }
            needs_advance.then(|| replay.shadow.clone())
        });
        if let Some(shadow) = shadow_to_advance {
            if let Err(e) = shadow.lock().unwrap().advance_until_first_committed_state() {
                self.set_anyhow_error(e);
            }
        }
    }

    /// Record the FF boundary. Called by per-game stepper traps when
    /// `current_tick == capture_tick()`, poised at the start of the tick with r4
    /// (local joyflags) left unset. Stores only the tick and outgoing packet; the
    /// matching mgba state is materialized on demand by
    /// [`Stepper::save`](super::Stepper::save) off the core, which the trap leaves
    /// parked exactly here.
    pub fn capture(&mut self) {
        let p = self.local_packet.clone().expect("local packet");
        let expected = self.output_pairs.len() as u32;
        if p.send_count != expected {
            panic!(
                "local packet send mismatch at capture: stored for send {}, current send {}",
                p.send_count, expected,
            );
        }
        let Mode::Fastforward(ff) = &mut self.mode else {
            panic!("capture is Fastforwarder-mode-only");
        };
        ff.captured = Some(CapturedBoundary {
            tick: self.current_tick,
            packet: p.packet,
        });
    }

    /// True iff the FF boundary has been recorded. The Fastforwarder outer loop
    /// exits when this flips to true. Per-game stepper traps
    /// (`copy_input_data_entry` in particular) gate on this to skip work after
    /// capture: `run_loop`'s cycle budget often spills past the trap-fire point
    /// that recorded the boundary, and any `apply_shadow_input` call from that
    /// spill would double-advance the shadow for the captured tick — the bug that
    /// desync'd BN4/5/EXE45.
    pub fn has_captured_snapshot(&self) -> bool {
        self.fastforward().is_some_and(|ff| ff.captured.is_some())
    }

    /// Consumes self into a Fastforwarder result. Panics if not in
    /// Fastforwarder mode or the boundary hasn't been recorded yet — callers
    /// must check [`Self::has_captured_snapshot`] first.
    pub(super) fn into_stepper_result(self) -> super::StepperResult {
        let Mode::Fastforward(ff) = self.mode else {
            panic!("into_stepper_result is Fastforwarder-mode-only");
        };
        super::StepperResult {
            boundary: ff.captured.expect("captured boundary"),
            round_result: self.round_result,
            hp: self.battle_hp,
            custom: self.battle_custom,
            chips: self.battle_chips,
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

    /// Report both players' HP for the tick being simulated. Called by
    /// per-game per-tick traps on games with known HP offsets; skipping the
    /// call on ticks where the unit structs aren't valid (battle intro) just
    /// leaves the previous report standing.
    pub fn set_battle_hp(&mut self, hp: super::BattleHp) {
        self.battle_hp = Some(hp);
    }

    /// The most recent per-tick HP report, if the current round's traps have
    /// made one (see [`Self::set_battle_hp`]). Replay analysis polls this
    /// per frame.
    pub fn battle_hp(&self) -> Option<super::BattleHp> {
        self.battle_hp
    }

    /// Report whether the custom screen (chip select) is open this tick.
    /// Called by per-game per-tick traps on games with known flag offsets.
    pub fn set_custom_screen(&mut self, open: bool) {
        self.battle_custom = Some(open);
    }

    /// The most recent per-tick custom-screen report, if the current
    /// round's traps have made one (see [`Self::set_custom_screen`]).
    pub fn custom_screen(&self) -> Option<bool> {
        self.battle_custom
    }

    /// Report each player's loaded chip id for this tick (0xFFFF = none).
    /// Called by per-game per-tick traps on games with known chip offsets.
    pub fn set_loaded_chips(&mut self, chips: [u16; 2]) {
        self.battle_chips = Some(chips);
    }

    /// The most recent per-tick loaded-chip report, if the current
    /// round's traps have made one (see [`Self::set_loaded_chips`]).
    pub fn loaded_chips(&self) -> Option<[u16; 2]> {
        self.battle_chips
    }

    pub fn set_round_ending(&mut self) {
        let shadow_to_advance = match &mut self.mode {
            Mode::Fastforward(ff) => {
                ff.ending = RoundPhase::Ending;
                None
            }
            Mode::Replay(replay) => {
                // The shadow round-end advance fires on the edge into
                // RoundEnding, which is what makes a second fire a no-op —
                // the multi-entry games (BN1/BN2 fire two round_ending_entry
                // traps) must only advance the shadow once. This mirrors
                // Match::end_round in PvP, which calls
                // shadow.advance_until_round_end from its round_ending_entry
                // trap.
                if replay.phase.has_ended_or_is_ending() {
                    None
                } else {
                    replay.phase = PlaybackPhase::RoundEnding;
                    Some(replay.shadow.clone())
                }
            }
        };
        if let Some(shadow) = shadow_to_advance {
            if let Err(e) = shadow.lock().unwrap().advance_until_round_end() {
                self.set_anyhow_error(e);
            }
        }
    }

    pub fn set_round_ended(&mut self) {
        match &mut self.mode {
            Mode::Fastforward(ff) => {
                ff.ending = RoundPhase::Ended;
            }
            Mode::Replay(replay) => {
                // Finished is sticky so the terminal state stays obvious.
                // (Not load-bearing: dropping back to RoundEnded would just
                // be absorbed into Finished again at the next
                // round_start_ret, since the queue is empty by then.)
                if replay.phase != PlaybackPhase::Finished {
                    replay.phase = PlaybackPhase::RoundEnded;
                }
                // Fire on_round_ended when the last queued round ends.
                if replay.next_rounds.is_empty() {
                    if let Some(callback) = replay.on_round_ended.take() {
                        callback();
                    }
                }
            }
        }
    }

    pub fn is_round_ending(&self) -> bool {
        match &self.mode {
            Mode::Fastforward(ff) => ff.ending != RoundPhase::InProgress,
            Mode::Replay(replay) => replay.phase.has_ended_or_is_ending(),
        }
    }

    pub fn is_round_ended(&self) -> bool {
        match &self.mode {
            Mode::Fastforward(ff) => ff.ending == RoundPhase::Ended,
            Mode::Replay(replay) => {
                matches!(replay.phase, PlaybackPhase::RoundEnded | PlaybackPhase::Finished)
            }
        }
    }

    // ----- replay-mode accessors -----
    //
    // These return Option / sensible Fastforwarder-mode defaults so per-game
    // stepper traps can use them unconditionally.

    pub fn is_replaying(&self) -> bool {
        matches!(self.mode, Mode::Replay(_))
    }

    /// Returns the replay-mode RNG, if this stepper is in replay mode.
    pub fn replay_rng_mut(&mut self) -> Option<&mut rand_pcg::Mcg128Xsl64> {
        self.replay_mut().map(|r| &mut r.rng)
    }

    pub fn replay_is_offerer(&self) -> bool {
        self.replay().is_some_and(|r| r.is_offerer)
    }

    /// True iff the current round has had its first commit. Used by per-game
    /// stepper traps to gate per-frame work that would otherwise diverge from
    /// the game's tick during boot / inter-round animations in replay mode.
    /// Always false in Fastforwarder mode.
    pub fn has_committed_this_round(&self) -> bool {
        self.replay().is_some_and(|r| r.phase.has_committed())
    }

    /// True iff round_start_ret has fired for the current round. In FF mode
    /// this is always true (FF resumes from a known committed state).
    pub fn round_active(&self) -> bool {
        self.replay().is_none_or(|r| r.phase.round_active())
    }

    // ----- replay-mode round transitions -----

    /// Called by per-game replay traps from round_start_ret.
    ///
    /// - `AwaitingRoundStart` (first round; inputs already loaded by
    ///   [`State::new`]): advance to `AwaitingFirstCommit` so the
    ///   first-commit gate in main_read_joyflags can fire.
    /// - `RoundEnded`: load the next queued round, or freeze into
    ///   `Finished` if the queue is empty.
    /// - Anything else (spurious mid-round fire): no-op.
    ///
    /// No-op in Fastforwarder mode.
    pub fn advance_to_next_replay_round_if_pending(&mut self) {
        let Some(replay) = self.replay_mut() else {
            return;
        };
        match replay.phase {
            PlaybackPhase::AwaitingRoundStart => {
                replay.phase = PlaybackPhase::AwaitingFirstCommit;
            }
            PlaybackPhase::RoundEnded => match replay.next_rounds.pop_front() {
                Some(round_inputs) => self.load_replay_round(round_inputs),
                None => replay.phase = PlaybackPhase::Finished,
            },
            _ => {}
        }
    }

    /// Resets per-round state, loads the given round's inputs, and arms the
    /// new round at `AwaitingFirstCommit`.
    fn load_replay_round(&mut self, round_inputs: Vec<PartialInputPair>) {
        self.current_tick = 0;
        // local_packet is lazy-seeded by per-game traps before the first
        // send/receive in the new round — the game's tx_packet will hold
        // the bg byte etc. set during the inter-round comm-menu phase.
        self.local_packet = None;

        {
            let replay = self.replay_mut().expect("load_replay_round in FF mode");
            replay.phase = PlaybackPhase::AwaitingFirstCommit;
            replay.current_round_index += 1;
        }

        self.input_pairs = round_inputs.into_iter().collect();
        self.output_pairs = vec![];
        self.round_result = None;
        // Traps only report HP while the unit slots are live, so without
        // this the previous round's final reading would leak into the next
        // round's intro window. Same for the custom-screen flag and the
        // loaded chips.
        self.battle_hp = None;
        self.battle_custom = None;
        self.battle_chips = None;
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
    /// Cumulative inputs consumed across all rounds at capture — the
    /// recorded-frame index this snapshot sits at. The seek/snapshot
    /// machinery keys on this so snapshots, the chase target, and the
    /// seek bar all share one scale (see [`InnerState::inputs_consumed`]),
    /// and the restored round's consume cursor is derived from it (minus
    /// the completed rounds' lengths). There is deliberately no separate
    /// in-round cursor field: `output_pairs.len()` restarts at zero on
    /// every restore, so a checkpoint captured mid-seek-chase would
    /// record a poisoned cursor and strand the playhead when loaded.
    pub frame_index: u32,
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
    /// The core's native-format video buffer at capture time. Blitted
    /// straight into the display framebuffer as an instant, emulation-free
    /// preview while the user drags the scrub bar.
    pub framebuffer: Vec<u8>,
}

/// Shared handle to the [`InnerState`]. Per-game traps clone this and lock
/// it inside their closures via [`State::lock_inner`].
#[derive(Clone)]
pub struct State(pub(super) Arc<Mutex<Option<InnerState>>>);

impl State {
    /// Build the playback pair for a recorded replay: the opponent co-sim
    /// (a [`Shadow`](crate::shadow::Shadow) over `remote_rom`, which
    /// re-derives the peer's per-tick packets from the recorded remote
    /// joyflags) and a replay-mode stepper state covering all recorded
    /// rounds. Everything else — match type, player index, RNG seed, tick
    /// totals — is derived from the replay itself.
    ///
    /// This is the one way every playback consumer starts (viewer, export,
    /// prefetch, the golden suite). Rounds play back in order;
    /// `set_round_ended` advances to the next until the queue is empty,
    /// then `on_round_ended` fires.
    pub fn new_for_replay(
        replay: &crate::replay::Replay,
        remote_rom: &[u8],
        remote_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
        on_round_ended: Box<dyn FnOnce() + Send>,
    ) -> anyhow::Result<(State, SharedShadow)> {
        let shadow = crate::shadow::Shadow::new_for_replay(remote_rom, replay, remote_hooks)?;
        let shadow = Arc::new(Mutex::new(shadow));

        let total_replay_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum();
        let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);

        use rand::SeedableRng;
        let mut rng = rand_pcg::Mcg128Xsl64::from_seed(replay.rng_seed);
        // Match::new advances the shared rng by one bool draw before any
        // game traps fire (the polite-win pick). Route through the same
        // function so the draw can't drift out of sync.
        let _ = crate::battle::Match::pick_local_player_index(&mut rng, replay.is_offerer);

        let inner = InnerState::for_replay(ReplayInit {
            match_type,
            local_player_index: replay.local_player_index,
            // Rounds commit at their first tick.
            commit_frontier: 0,
            rng,
            shadow: shadow.clone(),
            is_offerer: replay.is_offerer,
            rounds: VecDeque::from(replay.rounds.clone()),
            inputs_consumed_in_current_round: 0,
            current_round_index: 0,
            absolute_tick: 0,
            total_replay_ticks,
            current_tick_in_round: 0,
            local_packet_override: None,
            phase: PlaybackPhase::AwaitingRoundStart,
            disable_bgm: false,
            on_round_ended: Some(on_round_ended),
        });

        Ok((State(Arc::new(Mutex::new(Some(inner)))), shadow))
    }

    pub fn lock_inner(&self) -> InnerStateGuard<'_> {
        InnerStateGuard {
            guard: self.0.lock().unwrap(),
        }
    }

    /// Capture a checkpoint of replay-mode stepper state for later restore.
    /// Returns None outside replay mode or before round_start_ret has fired.
    pub fn capture_replay_checkpoint(&self) -> Option<ReplayCheckpoint> {
        let inner = self.lock_inner();
        let replay = inner.replay()?;
        if !replay.phase.round_active() {
            return None;
        }
        Some(ReplayCheckpoint {
            absolute_tick: replay.absolute_tick,
            current_round_index: replay.current_round_index,
            current_tick_in_round: inner.current_tick,
            has_committed_this_round: replay.phase.has_committed(),
            rng_state: replay.rng.clone(),
            local_packet: inner.local_packet.clone(),
            frame_index: inner.inputs_consumed(),
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
        let mut guard = self.0.lock().unwrap();
        let prev = guard.as_mut().ok_or_else(|| anyhow::anyhow!("stepper state missing"))?;
        let prev_replay = prev.replay_mut().ok_or_else(|| anyhow::anyhow!("not in replay mode"))?;

        let on_round_ended = prev_replay.on_round_ended.take();
        let is_offerer = prev_replay.is_offerer;
        let total_replay_ticks = prev_replay.total_replay_ticks;
        let shadow = prev_replay.shadow.clone();
        let commit_frontier = prev_replay.commit_frontier;
        let match_type = prev.match_type;
        let local_player_index = prev.local_player_index;
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

        // The in-round consume cursor is the recorded-frame index minus the
        // completed rounds' lengths — completed rounds always drain fully
        // before a transition, so the identity holds by construction.
        let consumed_before_round: u32 = rounds[..round_idx].iter().map(|r| r.len() as u32).sum();

        let new_inner = InnerState::for_replay(ReplayInit {
            match_type,
            local_player_index,
            commit_frontier,
            rng: checkpoint.rng_state.clone(),
            shadow,
            is_offerer,
            rounds: rounds_from_current,
            inputs_consumed_in_current_round: checkpoint.frame_index.saturating_sub(consumed_before_round),
            current_round_index: checkpoint.current_round_index,
            absolute_tick: checkpoint.absolute_tick,
            total_replay_ticks,
            current_tick_in_round: checkpoint.current_tick_in_round,
            local_packet_override: checkpoint.local_packet.as_ref().map(|lp| lp.packet.clone()),
            // Never an ending phase: the seek path restores the shadow
            // snapshot alongside this checkpoint, so the shadow round-end
            // advance must be allowed to (re-)fire when playback reaches the
            // round's end again.
            phase: if checkpoint.has_committed_this_round {
                PlaybackPhase::InRound
            } else {
                PlaybackPhase::AwaitingFirstCommit
            },
            disable_bgm,
            on_round_ended,
        });

        *guard = Some(new_inner);
        Ok(())
    }
}

/// MutexGuard wrapper that derefs straight to [`InnerState`]. The state is
/// stored as `Mutex<Option<InnerState>>` so `restore_replay_checkpoint` can
/// swap the whole inner; everyday callers don't care about the Option and
/// expect a direct &mut InnerState.
pub struct InnerStateGuard<'a> {
    guard: MutexGuard<'a, Option<InnerState>>,
}

impl std::ops::Deref for InnerStateGuard<'_> {
    type Target = InnerState;
    fn deref(&self) -> &InnerState {
        self.guard.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for InnerStateGuard<'_> {
    fn deref_mut(&mut self) -> &mut InnerState {
        self.guard.as_mut().unwrap()
    }
}
