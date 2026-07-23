//! Match statistics and their cached on-disk form.
//!
//! [`crate::analysis::analyze`] re-simulates a recorded replay on
//! a headless pair and extracts per-round [`MatchStats`]: the outcome
//! and both players' HP over the round, from the same RAM-poll
//! telemetry the live engine collects. That's a full replay simulation
//! — seconds of CPU — so stats are meant to be computed once and
//! cached in a small versioned binary sidecar (`<replay>.stats`, see
//! [`MatchStats::read`]/[`MatchStats::write`]). Live matches skip the
//! re-simulation entirely: the session folds each confirmed telemetry
//! batch into the same [`StatsBuilder`] as it plays — one
//! aggregation path, whichever side of the replay boundary the samples
//! come from.

/// Outcome of a single round, from this side's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde_repr::Serialize_repr, serde_repr::Deserialize_repr)]
#[repr(i8)]
pub enum BattleOutcome {
    Draw = -1,
    Loss = 0,
    Win = 1,
}

/// Bumped whenever the sidecar format changes shape — or meaning: v7
/// adds chip events for exe45's community PvP patch (per-screen hands)
/// and drops vanilla exe45's buster events (B is a menu key there); v6
/// fixed exe45's custom spans to track the battle-pausing tactics/chip
/// screens (the old source was the non-pausing operation-gauge cycle);
/// v5 extends chip-use events to bn2/bn3/bn4/exe45 (v4 introduced them
/// for bn5/bn6; v3's HP series became lossless change-point curves
/// where v2's were decimated; bumps make older files recompute).
/// Readers reject other versions (and anything without the magic, e.g.
/// the short-lived JSON v1 sidecars) and recompute.
// v8: fold fixes — samples/events now interleave by tick (a batch
// spanning a round boundary no longer folds the next round's first
// samples into the closing one) and samples stop at the round's
// announced verdict. Older sidecars recompute.
pub const FORMAT_VERSION: u32 = 8;

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

use mgba_rollback::session::TickObserver;

pub use crate::battle::{RoundSample, NO_CHIP};

/// Low bits of a `LoadedChip` report that carry the chip id; the rest is
/// the fire-sequence tag (see [`ChipSemantics::LoadedChip`]).
pub const CHIP_ID_MASK: u16 = 0x0fff;

/// The decoding contract for per-tick chip reports, declared per game by
/// [`GameSupport::chip_semantics`](crate::GameSupport::chip_semantics).
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
    counts_buster: bool,
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
            if counts_buster && !s.custom && s.buttons & b_bit != 0 && prev_buttons & b_bit == 0 {
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
pub struct StatsBuilder {
    semantics: ChipSemantics,
    /// Whether B-press edges are buster shots on this game/ROM (see
    /// [`GameSupport::counts_buster`](crate::GameSupport::counts_buster)).
    counts_buster: bool,
    prev_final: Option<(u16, u16)>,
    rounds: Vec<RoundStats>,
    /// Samples of the round in progress, in tick order. Ticks are
    /// session-absolute throughout — the recording is one contiguous
    /// stream and the stats stay on its timebase; round boundaries on
    /// that same timebase come from the replay's round markers, not
    /// from the stats.
    current: Vec<RoundSample>,
}

impl StatsBuilder {
    pub fn new(semantics: ChipSemantics, counts_buster: bool) -> Self {
        Self {
            semantics,
            counts_buster,
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
        self.rounds.push(fold_round(
            outcome,
            &samples,
            &mut self.prev_final,
            self.semantics,
            self.counts_buster,
        ));
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

    /// [`snapshot`](Self::snapshot) plus the round in progress folded as
    /// an undecided round — the live preview an in-flight analysis
    /// renders from. Non-mutating: the in-progress fold runs on a
    /// scratch copy of the stale-trim state, so a later
    /// [`end_round`](Self::end_round) produces the identical final
    /// round.
    pub fn preview(&self) -> MatchStats {
        let mut rounds = self.rounds.clone();
        if !self.current.is_empty() {
            let mut prev_final = self.prev_final;
            rounds.push(fold_round(
                None,
                &self.current,
                &mut prev_final,
                self.semantics,
                self.counts_buster,
            ));
        }
        MatchStats { rounds }
    }

    /// Finish, discarding any round still in progress — callers that
    /// want it folded call [`end_round`](Self::end_round) first.
    pub fn finish(self) -> MatchStats {
        MatchStats { rounds: self.rounds }
    }
}

/// One round's fold: stale-intro trim (`prev_final` threads the previous
/// round's final HP pair into the next fold), custom spans, chip/buster
/// usage events, and the lossless change-point HP curve. Shared by
/// [`StatsBuilder::end_round`] and the non-mutating
/// [`StatsBuilder::preview`].
fn fold_round(
    outcome: Option<BattleOutcome>,
    samples: &[RoundSample],
    prev_final: &mut Option<(u16, u16)>,
    semantics: ChipSemantics,
    counts_buster: bool,
) -> RoundStats {
    let raw: Vec<(u32, u16, u16)> = samples.iter().map(|s| (s.tick, s.local, s.remote)).collect();
    let start = stale_prefix_len(*prev_final, &raw);
    *prev_final = samples.last().map(|s| (s.local, s.remote)).or(*prev_final);
    let samples = &samples[start..];
    let custom = custom_spans(samples.iter().map(|s| (s.tick, s.custom)));
    let (chip_uses, buster) = usage_events(samples, &custom, semantics, counts_buster);
    RoundStats {
        outcome,
        hp: compress(samples.iter().map(|s| HpPoint {
            tick: s.tick,
            local: s.local,
            remote: s.remote,
        })),
        custom,
        chip_uses,
        buster,
    }
}

/// Why a stats sidecar failed to parse. Callers treat every variant as
/// "recompute" — the distinctions only serve logs.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("not a stats sidecar (bad magic)")]
    BadMagic,
    #[error("unsupported stats version {0} (want {FORMAT_VERSION})")]
    UnsupportedVersion(u32),
    #[error("bad outcome tag {0}")]
    BadOutcomeTag(i8),
    /// A count field beyond the reader's sanity cap — corrupt data,
    /// rejected before allocating for it.
    #[error("implausible {what} count {n}")]
    ImplausibleCount { what: &'static str, n: u32 },
}

impl MatchStats {
    /// Parse a sidecar. Errors on malformed input, a missing magic, or a
    /// version other than [`FORMAT_VERSION`] — callers treat all of these
    /// as "recompute".
    pub fn read(mut r: impl std::io::Read) -> Result<Self, ReadError> {
        fn u32_of(r: &mut impl std::io::Read) -> std::io::Result<u32> {
            let mut b = [0u8; 4];
            r.read_exact(&mut b)?;
            Ok(u32::from_le_bytes(b))
        }
        fn u16_of(r: &mut impl std::io::Read) -> std::io::Result<u16> {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            Ok(u16::from_le_bytes(b))
        }
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(ReadError::BadMagic);
        }
        let version = u32_of(&mut r)?;
        if version != FORMAT_VERSION {
            return Err(ReadError::UnsupportedVersion(version));
        }
        let n_rounds = u32_of(&mut r)?;
        // A best-of-3 match writes 2-3 rounds; anything huge is a
        // corrupt count, better rejected than allocated.
        if n_rounds > 64 {
            return Err(ReadError::ImplausibleCount {
                what: "round",
                n: n_rounds,
            });
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
                other => return Err(ReadError::BadOutcomeTag(other)),
            };
            let n_hp = u32_of(&mut r)?;
            if n_hp as usize > MAX_HP_POINTS_PER_ROUND {
                return Err(ReadError::ImplausibleCount { what: "hp point", n: n_hp });
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
                return Err(ReadError::ImplausibleCount {
                    what: "custom span",
                    n: n_custom,
                });
            }
            let mut custom = Vec::with_capacity(n_custom as usize);
            for _ in 0..n_custom {
                custom.push((u32_of(&mut r)?, u32_of(&mut r)?));
            }
            let mut chip_uses: [Vec<(u32, u16)>; 2] = [vec![], vec![]];
            for side in &mut chip_uses {
                let n = u32_of(&mut r)?;
                if n > 4096 {
                    return Err(ReadError::ImplausibleCount { what: "chip-use", n });
                }
                for _ in 0..n {
                    side.push((u32_of(&mut r)?, u16_of(&mut r)?));
                }
            }
            let mut buster: [Vec<u32>; 2] = [vec![], vec![]];
            for side in &mut buster {
                let n = u32_of(&mut r)?;
                if n > 65536 {
                    return Err(ReadError::ImplausibleCount { what: "buster", n });
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

    pub fn write(&self, mut w: impl std::io::Write) -> std::io::Result<()> {
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

// ---------------------------------------------------------------------------
// Replay re-analysis: linear re-simulation + the telemetry -> stats fold
// shared with the live session (so live stats and offline re-analysis stay
// byte-equivalent).

use crate::telemetry::{self, Telemetry};
use crate::{GameSupport, PrimeConfig};

/// Cap on priming ticks, mirroring the live engine's bound.
const MAX_PRIME_TICKS: u32 = 3600;

/// Everything [`analyze`] needs. All fields are in **absolute** player
/// order (core 0 runs player 0's game), which is how the caller should
/// orient the replay's local/remote pairs using
/// [`Replay::local_player_index`](crate::replay::Replay::local_player_index).
pub struct AnalyzeConfig<'a> {
    pub roms: [Vec<u8>; 2],
    pub saves: [Vec<u8>; 2],
    pub support: [&'a dyn GameSupport; 2],
    pub match_type: (u8, u8),
    pub rng_seed: [u8; 16],
    pub rtc: std::time::SystemTime,
    /// Which side the stats should be from the perspective of.
    pub local_player: usize,
    /// `[p0, p1]` joypad pairs, one per pair tick from session start.
    pub inputs: &'a [[u32; 2]],
    /// Chip-report semantics + buster counting for the local game (see
    /// [`Hooks::chip_semantics`](crate::hooks::Hooks::chip_semantics)).
    pub chip_semantics: crate::analysis::ChipSemantics,
    pub counts_buster: bool,
}

/// Re-simulate an SIO replay and fold its telemetry into [`MatchStats`].
/// `on_progress` is called once per simulated tick with `(done, total)`
/// and the in-flight builder; flipping `cancel` aborts with an error and
/// nothing partial.
pub fn analyze(
    config: AnalyzeConfig<'_>,
    on_progress: &mut dyn FnMut(u32, u32, &StatsBuilder),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<MatchStats, crate::Error> {
    let AnalyzeConfig {
        roms,
        saves,
        support,
        match_type,
        rng_seed,
        rtc,
        local_player,
        inputs,
        chip_semantics,
        counts_buster,
    } = config;
    let [rom0, rom1] = roms;
    let [save0, save1] = saves;

    let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
        sides: vec![
            mgba_rollback::SideOptions {
                rom: rom0,
                save: Some(save0),
            },
            mgba_rollback::SideOptions {
                rom: rom1,
                save: Some(save1),
            },
        ],
        rtc: Some(rtc),
        peripheral: mgba_rollback::Peripheral::Cable,
    })?;
    // Nothing reads the pixels — skip rasterization on both cores.
    pair.set_frameskip(0, i32::MAX);
    pair.set_frameskip(1, i32::MAX);

    let prime_config = PrimeConfig {
        match_type,
        rng_seed,
        // Presentation-only (audio is never read here), and gameplay-neutral
        // either way — see `PrimeConfig::disable_bgm`.
        disable_bgm: false,
    };
    let lifecycle = crate::telemetry::LifecycleSink::new();
    let primed = [crate::PrimedLatch::new(), crate::PrimedLatch::new()];
    // Cores own their primer traps — see [`mgba_rollback::Link::set_traps`]
    // for why any other ownership dangles at core teardown.
    pair.set_traps(0, support[0].primer_traps(&prime_config, 0, &lifecycle, &primed[0]));
    pair.set_traps(1, support[1].primer_traps(&prime_config, 1, &lifecycle, &primed[1]));

    let mut prime_ticks = 0;
    while !(primed[0].is_set() && primed[1].is_set()) {
        if prime_ticks >= MAX_PRIME_TICKS {
            return Err(crate::Error::PrimeTimeout(MAX_PRIME_TICKS));
        }
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::Error::Cancelled);
        }
        pair.tick(&[0, 0]);
        prime_ticks += 1;
    }

    let (mut observer, store) = Telemetry::new([support[0].core_poller(0), support[1].core_poller(1)], lifecycle);
    let mut builder = StatsBuilder::new(chip_semantics, counts_buster);
    let total = inputs.len() as u32;
    for (i, &keys) in inputs.iter().enumerate() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::Error::Cancelled);
        }
        let tick = i as u32 + 1;
        pair.tick(&keys);
        // Everything is final on a linear re-sim — fold as we go.
        observer.on_tick(&mut pair, tick);
        let (samples, events) = store.lock().unwrap().drain_confirmed(tick);
        fold_confirmed(&mut builder, local_player, samples, events, &mut |t| {
            (t == tick).then_some(keys)
        });
        on_progress(tick, total, &builder);
    }

    Ok(builder.finish())
}

/// Fold a batch of **confirmed** telemetry into a [`StatsBuilder`]:
/// per-tick samples become [`RoundSample`]s (with the A/B button bits
/// merged back in from the confirmed input pairs via `keys_at`), and
/// `Ended` events close rounds with their outcome oriented to
/// `local_player`. Shared by the live drive loop and [`analyze`], so the
/// two produce identical stats for the same match.
///
/// Samples and events merge in tick order, events first at a shared
/// tick: the `Ended` that closes a round at tick T must fold before T's
/// own sample — which belongs to the NEW round — is pushed, or a batch
/// spanning a round boundary would fold the next round's first samples
/// into the closing one.
pub fn fold_confirmed(
    builder: &mut StatsBuilder,
    local_player: usize,
    samples: Vec<(u32, telemetry::BattleObs)>,
    events: Vec<(u32, telemetry::RoundEvent)>,
    keys_at: &mut dyn FnMut(u32) -> Option<[u32; 2]>,
) {
    let mut push = |builder: &mut StatsBuilder, tick: u32, obs: telemetry::BattleObs| {
        let mut buttons = 0u8;
        if let Some(keys) = keys_at(tick) {
            let (lk, rk) = (keys[local_player] as u16, keys[1 - local_player] as u16);
            // KEYINPUT bit 0 = A, bit 1 = B, mirrored into the sample's
            // packed button bits.
            if lk & 0x1 != 0 {
                buttons |= crate::battle::BUTTON_LOCAL_A;
            }
            if lk & 0x2 != 0 {
                buttons |= crate::battle::BUTTON_LOCAL_B;
            }
            if rk & 0x1 != 0 {
                buttons |= crate::battle::BUTTON_REMOTE_A;
            }
            if rk & 0x2 != 0 {
                buttons |= crate::battle::BUTTON_REMOTE_B;
            }
        }
        builder.push_sample(RoundSample {
            tick,
            local: obs.hp[local_player],
            remote: obs.hp[1 - local_player],
            custom: obs.custom[local_player],
            buttons,
            chips: [obs.chips[local_player], obs.chips[1 - local_player]],
        });
    };

    let mut samples = samples.into_iter().peekable();
    for (etick, event) in events {
        while let Some(&(tick, obs)) = samples.peek() {
            if tick >= etick {
                break;
            }
            samples.next();
            push(builder, tick, obs);
        }
        match event {
            // Rounds are delimited by `Ended`; the `Started` event only
            // matters here for the merge ordering above (its tick is
            // the boundary samples must not cross).
            telemetry::RoundEvent::Started => {}
            telemetry::RoundEvent::Ended { outcome } => {
                builder.end_round(outcome.map(|o| orient_outcome(o, local_player)));
            }
            telemetry::RoundEvent::MatchEnded => {}
        }
    }
    for (tick, obs) in samples {
        push(builder, tick, obs);
    }
}

/// Absolute player outcome → `local_player`'s perspective.
pub fn orient_outcome(o: telemetry::Outcome, local_player: usize) -> BattleOutcome {
    match o {
        telemetry::Outcome::Draw => BattleOutcome::Draw,
        telemetry::Outcome::P0Win => {
            if local_player == 0 {
                BattleOutcome::Win
            } else {
                BattleOutcome::Loss
            }
        }
        telemetry::Outcome::P1Win => {
            if local_player == 1 {
                BattleOutcome::Win
            } else {
                BattleOutcome::Loss
            }
        }
    }
}
