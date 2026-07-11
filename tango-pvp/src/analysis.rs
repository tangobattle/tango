//! Post-hoc replay analysis and its cached on-disk form.
//!
//! [`analyze`] re-simulates a recorded replay on a headless core pair
//! (playback stepper + shadow, the same machinery the viewer and the
//! golden suites drive) and extracts per-round [`MatchStats`]: the
//! outcome and both players' HP over the round, as reported by the
//! per-game traps via [`InnerState::set_battle_hp`]. That's a full
//! replay simulation — seconds of CPU — so stats are meant to be
//! computed once and cached in a small versioned binary sidecar
//! (`<replay>.stats`, see [`MatchStats::read`]/[`MatchStats::write`]).
//! Live matches skip the re-simulation entirely: the rollback engine
//! collects the same per-tick samples, and the match folds each round
//! into the same [`MatchStatsBuilder`] the moment it ends — one aggregation
//! path, whichever side of the replay boundary the samples come from.
//!
//! [`InnerState::set_battle_hp`]: crate::stepper::InnerState::set_battle_hp

use crate::stepper::BattleOutcome;

/// Bumped whenever the sidecar format changes shape — or meaning: v6
/// fixes exe45's custom spans to track the battle-pausing tactics/chip
/// screens (the old source was the non-pausing operation-gauge cycle);
/// v5 extends chip-use events to bn2/bn3/bn4/exe45 (v4 introduced them
/// for bn5/bn6; v3's HP series became lossless change-point curves
/// where v2's were decimated; bumps make older files recompute).
/// Readers reject other versions (and anything without the magic, e.g.
/// the short-lived JSON v1 sidecars) and recompute.
pub const FORMAT_VERSION: u32 = 6;

/// Sidecar file magic.
const MAGIC: &[u8; 4] = b"TGST";

/// Reader-side sanity cap on stored HP points per round — far above any
/// real round (HP changes a few dozen times), it only rejects corrupt
/// counts before allocating for them.
const MAX_HP_POINTS_PER_ROUND: usize = 65536;

/// Per-match statistics, from the local player's perspective of the
/// replay (or live session) they came from.
#[derive(Clone, Debug)]
pub struct MatchStats {
    pub rounds: Vec<RoundStats>,
}

#[derive(Clone, Debug)]
pub struct RoundStats {
    /// `None` when the recording ended before the round reached a KO.
    pub outcome: Option<BattleOutcome>,
    /// The round's full HP curve, losslessly change-point encoded: the
    /// first and last samples plus every sample whose `(local, remote)`
    /// pair differs from the one before it. HP holds between entries
    /// (step semantics), so the per-tick series reconstructs exactly.
    /// Empty on rounds that never got past the battle intro.
    pub hp: Vec<HpPoint>,
    /// `[start, end)` tick spans during which the custom screen (chip
    /// select) was open. Empty on games whose traps don't report the
    /// flag.
    pub custom: Vec<(u32, u32)>,
    /// Chip-use events per side (`[local, remote]`): `(tick, chip id)`
    /// at the moment the unit's loaded chip departed by being used.
    /// Empty on games whose traps don't report loaded chips.
    pub chip_uses: [Vec<(u32, u16)>; 2],
    /// Buster-press ticks per side (`[local, remote]`): B press edges
    /// outside the custom screen.
    pub buster: [Vec<u32>; 2],
}

pub use crate::battle::{RoundSample, NO_CHIP};
use crate::battle::{BUTTON_LOCAL_A, BUTTON_LOCAL_B, BUTTON_REMOTE_A, BUTTON_REMOTE_B, JOY_A, JOY_B};

/// Low bits of a `LoadedChip` report that carry the chip id; the rest is
/// the fire-sequence tag (see [`ChipSemantics::LoadedChip`]).
pub const CHIP_ID_MASK: u16 = 0x0fff;

/// The decoding contract for per-tick chip reports, declared per game by
/// [`Hooks::chip_semantics`](crate::hooks::Hooks::chip_semantics).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChipSemantics {
    /// Each report is the player's loaded chip as `id | (seq << 12)`,
    /// [`NO_CHIP`] when none: the id the player will use next, tagged
    /// with a fire-sequence counter so back-to-back duplicate picks
    /// still produce a visible transition per use (bn5/bn6 report a
    /// raw cell with seq 0 — no counter is known for them). A value
    /// departing = that chip being used, EXCEPT the first departure
    /// after each custom close (the new selection landing).
    LoadedChip,
    /// Each report is the sum of the ids remaining in the player's
    /// dealt queue (exe45). The queue only ever gains chips (deals) or
    /// loses exactly the fired chip, so a drop in the sum IS a use
    /// event and the delta IS the chip id; increases are deals and are
    /// ignored. Chosen over reporting the queue head because exe45
    /// players fire from a hand in an order the head doesn't determine.
    QueueSum,
}

/// Fold a round's per-tick samples into chip-use and buster events.
///
/// [`ChipSemantics::LoadedChip`]: a loaded chip departing = that chip
/// being used — EXCEPT the first departure after each custom close,
/// which is the new selection landing on top of whatever was left (see
/// the per-game chip-block docs). [`ChipSemantics::QueueSum`]: a drop
/// in the reported sum is a use of the delta.
fn usage_events(
    samples: &[RoundSample],
    custom: &[(u32, u32)],
    semantics: ChipSemantics,
) -> ([Vec<(u32, u16)>; 2], [Vec<u32>; 2]) {
    /// Sanity bound on a chip id: `QueueSum` drops above this are queue
    /// clears (KO, round end), not uses.
    const MAX_CHIP_ID: u16 = 0x1ff;

    let mut chip_uses: [Vec<(u32, u16)>; 2] = [vec![], vec![]];
    let mut buster: [Vec<u32>; 2] = [vec![], vec![]];
    for side in 0..2 {
        let mut prev_chip = NO_CHIP;
        let mut prev_buttons = 0u8;
        // Index of the next custom span whose close hasn't had its
        // selection-load transition yet.
        let mut next_load_span = 0usize;
        let b_bit = if side == 0 { 1u8 << 1 } else { 1u8 << 3 };
        for s in samples {
            let chip = s.chips[side];
            match semantics {
                ChipSemantics::LoadedChip => {
                    if chip != prev_chip {
                        // Skip spans whose load window has fully passed
                        // (a side that picked nothing produces no load
                        // transition).
                        while next_load_span < custom.len()
                            && custom.get(next_load_span + 1).is_some_and(|&(s2, _)| s.tick >= s2)
                        {
                            next_load_span += 1;
                        }
                        // A load consumes the pending span: the selection
                        // can land while the span is still open (bn6
                        // writes the block mid-pick; bn1-3's local-only
                        // spans outlast the other side's commit) or right
                        // after its close. Anything else departing a real
                        // chip is a use.
                        let is_load = custom.get(next_load_span).is_some_and(|&(s0, _)| s.tick >= s0);
                        if is_load {
                            next_load_span += 1;
                        } else if next_load_span > 0 && prev_chip != NO_CHIP {
                            // next_load_span == 0 means no selection has
                            // landed yet — the cell still holds round-init
                            // garbage (bn5 inits it to 0 before first
                            // flipping to the sentinel), which can't be a
                            // real use.
                            chip_uses[side].push((s.tick, prev_chip & CHIP_ID_MASK));
                        }
                        prev_chip = chip;
                    }
                }
                ChipSemantics::QueueSum => {
                    if chip != NO_CHIP {
                        if prev_chip != NO_CHIP && chip < prev_chip {
                            let delta = prev_chip - chip;
                            if delta <= MAX_CHIP_ID {
                                chip_uses[side].push((s.tick, delta));
                            }
                        }
                        prev_chip = chip;
                    }
                }
            }
            if !s.custom && s.buttons & b_bit != 0 && prev_buttons & b_bit == 0 {
                buster[side].push(s.tick);
            }
            prev_buttons = s.buttons;
        }
    }
    (chip_uses, buster)
}

/// One HP reading.
#[derive(Clone, Copy, Debug)]
pub struct HpPoint {
    pub tick: u32,
    pub local: u16,
    pub remote: u16,
}

/// Incremental [`MatchStats`] aggregator — THE stats construction path,
/// live or offline. Feed it every simulated tick's [`RoundSample`] as it
/// happens and close each round with [`end_round`](Self::end_round). The
/// live engine pushes from its rollback world — speculative ticks
/// included, revoked again via
/// [`revoke_samples_at`](Self::revoke_samples_at) when a rollback
/// rewinds — and the replay re-simulation ([`analyze`]) pushes from its
/// playback loop. Rounds fold in play order: the stale-intro trim
/// threads each round's final HP pair into the next round's fold.
pub struct MatchStatsBuilder {
    semantics: ChipSemantics,
    prev_final: Option<(u16, u16)>,
    rounds: Vec<RoundStats>,
    /// Samples of the round in progress, in tick order.
    current: Vec<RoundSample>,
}

impl MatchStatsBuilder {
    pub fn new(semantics: ChipSemantics) -> Self {
        Self {
            semantics,
            prev_final: None,
            rounds: vec![],
            current: vec![],
        }
    }

    /// Append one simulated tick's sample to the round in progress. A
    /// sample for the same tick as the last one is ignored — the offline
    /// analyzer polls once per frame and can observe a tick twice.
    pub fn push_sample(&mut self, sample: RoundSample) {
        if self.current.last().map(|s| s.tick) != Some(sample.tick) {
            self.current.push(sample);
        }
    }

    /// Revoke in-progress samples at or after `tick` — rollback support
    /// for the live engine, whose steps push speculatively: a rewind to
    /// `tick` discards exactly what the re-sim is about to redo.
    ///
    /// Speculative-then-revoke is unavoidable, not an optimization: a
    /// step can't tell a predicted remote input from a confirmed one,
    /// and a correctly-predicted tick is *promoted* to settled without
    /// ever being re-simulated — there is no committed re-execution to
    /// sample instead. Buffering until settlement elsewhere would just
    /// move this same revocation out of sight, and flushing on the
    /// engine's confirmed-input stream would shave the round's
    /// end-animation tail (settlement trails the live core by the
    /// speculation depth, and input logging stops at the round-ending
    /// tick), breaking the byte-equivalence between live stats and
    /// offline re-analysis.
    pub fn revoke_samples_at(&mut self, tick: u32) {
        while self.current.last().is_some_and(|s| s.tick >= tick) {
            self.current.pop();
        }
    }

    /// Close the round in progress, folding its samples into a
    /// [`RoundStats`]: stale-intro trim, custom spans, chip/buster usage
    /// events, and the lossless change-point HP curve. `outcome` is
    /// `None` when the round was never decided (the recording ended
    /// mid-round, or a live round was torn down without reaching a KO).
    pub fn end_round(&mut self, outcome: Option<BattleOutcome>) {
        let samples = std::mem::take(&mut self.current);
        let raw: Vec<(u32, u16, u16)> = samples.iter().map(|s| (s.tick, s.local, s.remote)).collect();
        let start = stale_prefix_len(self.prev_final, &raw);
        self.prev_final = samples.last().map(|s| (s.local, s.remote)).or(self.prev_final);
        let samples = &samples[start..];
        let custom = custom_spans(samples.iter().map(|s| (s.tick, s.custom)));
        let (chip_uses, buster) = usage_events(samples, &custom, self.semantics);
        self.rounds.push(RoundStats {
            outcome,
            hp: compress(samples.iter().map(|s| HpPoint {
                tick: s.tick,
                local: s.local,
                remote: s.remote,
            })),
            custom,
            chip_uses,
            buster,
        });
    }

    /// The rounds folded so far — the round in progress isn't included.
    /// A clone (cheap: change-point curves and event lists, not raw
    /// samples), so the live teardown can hand one copy to the sidecar
    /// writer and another to the results card while the builder stays in
    /// place.
    pub fn snapshot(&self) -> MatchStats {
        MatchStats {
            rounds: self.rounds.clone(),
        }
    }

    /// Finish, discarding any round still in progress — callers that
    /// want it folded call [`end_round`](Self::end_round) first.
    pub fn finish(self) -> MatchStats {
        MatchStats { rounds: self.rounds }
    }
}

impl MatchStats {

    /// Parse a sidecar. Errors on malformed input, a missing magic, or a
    /// version other than [`FORMAT_VERSION`] — callers treat all of these
    /// as "recompute".
    pub fn read(mut r: impl std::io::Read) -> anyhow::Result<Self> {
        fn u32_of(r: &mut impl std::io::Read) -> anyhow::Result<u32> {
            let mut b = [0u8; 4];
            r.read_exact(&mut b)?;
            Ok(u32::from_le_bytes(b))
        }
        fn u16_of(r: &mut impl std::io::Read) -> anyhow::Result<u16> {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            Ok(u16::from_le_bytes(b))
        }
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            anyhow::bail!("not a stats sidecar (bad magic)");
        }
        let version = u32_of(&mut r)?;
        if version != FORMAT_VERSION {
            anyhow::bail!("unsupported stats version {} (want {})", version, FORMAT_VERSION);
        }
        let n_rounds = u32_of(&mut r)?;
        // A best-of-3 match writes 2-3 rounds; anything huge is a
        // corrupt count, better rejected than allocated.
        if n_rounds > 64 {
            anyhow::bail!("implausible round count {}", n_rounds);
        }
        let mut rounds = Vec::with_capacity(n_rounds as usize);
        for _ in 0..n_rounds {
            let mut tag = [0u8; 1];
            r.read_exact(&mut tag)?;
            let outcome = match tag[0] as i8 {
                -2 => None,
                -1 => Some(BattleOutcome::Draw),
                0 => Some(BattleOutcome::Loss),
                1 => Some(BattleOutcome::Win),
                other => anyhow::bail!("bad outcome tag {}", other),
            };
            let n_hp = u32_of(&mut r)?;
            if n_hp as usize > MAX_HP_POINTS_PER_ROUND {
                anyhow::bail!("implausible hp point count {}", n_hp);
            }
            let mut hp = Vec::with_capacity(n_hp as usize);
            for _ in 0..n_hp {
                hp.push(HpPoint {
                    tick: u32_of(&mut r)?,
                    local: u16_of(&mut r)?,
                    remote: u16_of(&mut r)?,
                });
            }
            let n_custom = u32_of(&mut r)?;
            if n_custom > 1024 {
                anyhow::bail!("implausible custom span count {}", n_custom);
            }
            let mut custom = Vec::with_capacity(n_custom as usize);
            for _ in 0..n_custom {
                custom.push((u32_of(&mut r)?, u32_of(&mut r)?));
            }
            let mut chip_uses: [Vec<(u32, u16)>; 2] = [vec![], vec![]];
            for side in &mut chip_uses {
                let n = u32_of(&mut r)?;
                if n > 4096 {
                    anyhow::bail!("implausible chip-use count {}", n);
                }
                for _ in 0..n {
                    side.push((u32_of(&mut r)?, u16_of(&mut r)?));
                }
            }
            let mut buster: [Vec<u32>; 2] = [vec![], vec![]];
            for side in &mut buster {
                let n = u32_of(&mut r)?;
                if n > 65536 {
                    anyhow::bail!("implausible buster count {}", n);
                }
                for _ in 0..n {
                    side.push(u32_of(&mut r)?);
                }
            }
            rounds.push(RoundStats {
                outcome,
                hp,
                custom,
                chip_uses,
                buster,
            });
        }
        Ok(MatchStats { rounds })
    }

    pub fn write(&self, mut w: impl std::io::Write) -> anyhow::Result<()> {
        w.write_all(MAGIC)?;
        w.write_all(&FORMAT_VERSION.to_le_bytes())?;
        w.write_all(&(self.rounds.len() as u32).to_le_bytes())?;
        for round in &self.rounds {
            let tag: i8 = match round.outcome {
                None => -2,
                Some(o) => o as i8,
            };
            w.write_all(&tag.to_le_bytes())?;
            w.write_all(&(round.hp.len() as u32).to_le_bytes())?;
            for p in &round.hp {
                w.write_all(&p.tick.to_le_bytes())?;
                w.write_all(&p.local.to_le_bytes())?;
                w.write_all(&p.remote.to_le_bytes())?;
            }
            w.write_all(&(round.custom.len() as u32).to_le_bytes())?;
            for &(a, b) in &round.custom {
                w.write_all(&a.to_le_bytes())?;
                w.write_all(&b.to_le_bytes())?;
            }
            for side in &round.chip_uses {
                w.write_all(&(side.len() as u32).to_le_bytes())?;
                for &(t, id) in side {
                    w.write_all(&t.to_le_bytes())?;
                    w.write_all(&id.to_le_bytes())?;
                }
            }
            for side in &round.buster {
                w.write_all(&(side.len() as u32).to_le_bytes())?;
                for &t in side {
                    w.write_all(&t.to_le_bytes())?;
                }
            }
        }
        Ok(())
    }
}

/// Length of a round's stale sample prefix. The unit slots re-initialize
/// partway into the battle intro, so until then the traps relay whatever
/// the slots still hold: the PREVIOUS round's final values (or, for the
/// first round on games whose slots map immediately, the zeroed fresh
/// memory). That prefix is exactly the samples equal to the previous
/// round's final `(local, remote)` pair — the first differing sample IS
/// the re-init write. `prev_final` is `None` for a match's first round,
/// where the stale state is the zero pair (a live round never starts at
/// 0–0). Public so the live results card cuts the same prefix from its
/// raw round reports.
///
/// Assumes rounds restore HP (true of every supported game's link
/// battles): under a hypothetical carry-over rule the first round-open
/// samples would be indistinguishable from the stale prefix.
pub fn stale_prefix_len(prev_final: Option<(u16, u16)>, hp: &[(u32, u16, u16)]) -> usize {
    let stale = prev_final.unwrap_or((0, 0));
    hp.iter()
        .take_while(|&&(_, local, remote)| (local, remote) == stale)
        .count()
}

/// Fold a per-tick custom-screen stream into `[start, end)` spans.
fn custom_spans(ticks: impl Iterator<Item = (u32, bool)>) -> Vec<(u32, u32)> {
    let mut spans: Vec<(u32, u32)> = vec![];
    let mut open: Option<u32> = None;
    let mut last_tick = 0;
    for (tick, custom) in ticks {
        last_tick = tick;
        match (custom, open) {
            (true, None) => open = Some(tick),
            (false, Some(start)) => {
                spans.push((start, tick));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        spans.push((start, last_tick + 1));
    }
    spans
}

/// Change-point encode `points`: keep the first and last samples plus
/// every sample whose `(local, remote)` pair moved. Lossless — HP holds
/// between entries, so the dropped samples are byte-identical repeats.
fn compress(points: impl Iterator<Item = HpPoint>) -> Vec<HpPoint> {
    let mut out: Vec<HpPoint> = vec![];
    let mut pending: Option<HpPoint> = None;
    for p in points {
        match out.last() {
            Some(prev) if (prev.local, prev.remote) == (p.local, p.remote) => {
                // A repeat: remember it as the candidate final sample.
                pending = Some(p);
            }
            _ => {
                out.push(p);
                pending = None;
            }
        }
    }
    if let Some(last) = pending {
        out.push(last);
    }
    out
}

/// Cap on frames to keep simulating after the last recorded input while
/// waiting for the final round's `round_end_*` trap — 10 s of game time
/// covers every end-of-round animation; genuinely incomplete replays
/// give up and report what they have.
const MAX_DRAIN_FRAMES: u32 = 600;

/// Re-simulate `replay` end-to-end and collect [`MatchStats`]. Drives
/// the same replay-mode stepper + shadow pair as the viewer, headless
/// and unthrottled with rasterization off; expect it to take seconds of
/// CPU per minute of match, so run it on a worker and cache the result.
/// `on_progress` receives `(ticks simulated, total recorded ticks)`
/// every simulated tick.
pub fn analyze(
    replay: &crate::replay::Replay,
    local_rom: &[u8],
    remote_rom: &[u8],
    local_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    remote_hooks: &'static (dyn crate::hooks::Hooks + Send + Sync),
    on_progress: &mut dyn FnMut(u32, u32),
) -> anyhow::Result<MatchStats> {
    let mut core = mgba::core::Core::new_gba("tango", &mgba::core::Options { ..Default::default() })?;
    core.enable_video_buffer();
    core.as_mut()
        .load_rom(mgba::vfile::VFile::from_vec(local_rom.to_vec()))?;
    core.as_mut()
        .load_save(mgba::vfile::VFile::from_vec(replay.local_sram.clone()))?;
    // Pin the cart RTC to the recorded clock so RTC-reading games
    // (exe45) simulate the same values they saw live.
    core.set_rtc_fixed(replay.rtc_time());
    core.as_mut().reset();
    // Nothing reads the pixels — skip rasterization, like the PvP
    // re-sim stepper does. Set after reset(), which zeroes frameskip.
    core.as_mut().gba_mut().set_frameskip(i32::MAX);

    let (stepper_state, _shadow) =
        crate::stepper::State::new_for_replay(replay, remote_rom, remote_hooks, Box::new(|| {}))?;
    local_hooks.install_on_stepper(&mut core, stepper_state.clone());

    let semantics = local_hooks.chip_semantics();
    let total_ticks: u32 = replay.rounds.iter().map(|r| r.len() as u32).sum();
    let mut last_reported: u32 = 0;

    let lpi = replay.local_player_index as usize;
    // The same incremental aggregation a live match performs: push each
    // simulated tick's sample, close each round as it ends.
    let mut builder = MatchStatsBuilder::new(semantics);
    // Latest result seen for the round in progress. `round_result()`
    // clears across the round transition, so it's committed on the
    // round-index edge (or at the end of the drain for the last round).
    let mut current_result: Option<BattleOutcome> = None;
    let mut last_round_idx: u32 = 0;
    let mut frames_after_drain: u32 = 0;

    loop {
        let (total_left, abs_tick, round_idx, ended, result, tick, hp, custom, chips) = {
            let inner = stepper_state.lock_inner();
            (
                inner.total_input_pairs_left(),
                inner.absolute_tick(),
                inner.current_round_index(),
                inner.is_round_ended(),
                inner.round_result(),
                inner.current_tick(),
                inner.battle_hp(),
                inner.custom_screen(),
                inner.loaded_chips(),
            )
        };

        if abs_tick != last_reported {
            last_reported = abs_tick;
            on_progress(abs_tick, total_ticks);
        }

        if round_idx != last_round_idx {
            builder.end_round(current_result.take());
            last_round_idx = round_idx;
        }
        if let Some(rr) = result {
            current_result = Some(rr.outcome);
        }
        // The traps re-report every tick, so a new tick with a reading is
        // exactly one sample — the builder drops repeat polls of the same
        // tick. (`battle_hp` is cleared across round transitions and
        // stays `None` through each battle intro.)
        if let Some(hp) = hp {
            // Buttons come straight off the recorded input pairs
            // (already `(local, remote)`-oriented): the sample at
            // `tick` reflects the state after the pair at `tick - 1`
            // was applied, matching the live path's labeling of "the
            // pair that produced the sampled state".
            let mut buttons = 0u8;
            if let Some((local, remote)) = replay
                .rounds
                .get(round_idx as usize)
                .and_then(|r| r.get((tick as usize).wrapping_sub(1)))
            {
                buttons |= if local.joyflags & JOY_A != 0 { BUTTON_LOCAL_A } else { 0 };
                buttons |= if local.joyflags & JOY_B != 0 { BUTTON_LOCAL_B } else { 0 };
                buttons |= if remote.joyflags & JOY_A != 0 {
                    BUTTON_REMOTE_A
                } else {
                    0
                };
                buttons |= if remote.joyflags & JOY_B != 0 {
                    BUTTON_REMOTE_B
                } else {
                    0
                };
            }
            let chips = chips.unwrap_or([NO_CHIP; 2]);
            builder.push_sample(RoundSample {
                tick,
                local: hp[lpi],
                remote: hp[1 - lpi],
                custom: custom.unwrap_or(false),
                buttons,
                chips: [chips[lpi], chips[1 - lpi]],
            });
        }

        if total_left == 0 && abs_tick > 0 {
            if (ended && current_result.is_some()) || frames_after_drain >= MAX_DRAIN_FRAMES {
                builder.end_round(current_result.take());
                break;
            }
            frames_after_drain += 1;
        }

        core.as_mut().run_frame();
    }

    Ok(builder.finish())
}
