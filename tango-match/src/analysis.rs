//! Match statistics and their cached on-disk form.
//!
//! [`crate::r#match::analysis::analyze`] re-simulates a recorded replay on
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
// v9: bn1 reports chip identity now (it was buster-only), so its
// cached stats are missing every chip event they should have.
// v10: bn1's first cut read the chip-select block and capped ids at
// 0xB0 — it dropped every higher-id chip and invented one use per
// custom cycle, so v9's bn1 sidecars are wrong on both counts.
pub const FORMAT_VERSION: u32 = 10;

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
    /// as the game's own fold scores them (see [`UsageFold`]). Empty on
    /// games whose traps report no chips.
    pub chip_uses: [Vec<(u32, u16)>; 2],
    /// Buster-press ticks per side (`[local, remote]`): B press edges
    /// outside the custom screen, on games whose fold counts them.
    pub buster: [Vec<u32>; 2],
}

pub use crate::battle::{RoundSample, NO_CHIP};

/// One round's usage events, as a game's own fold derives them from the
/// round's samples (see [`UsageFold`]). Field shapes match their
/// [`RoundStats`] homes.
#[derive(Default)]
pub struct UsageEvents {
    /// Chip-use events per side (`[local, remote]`): `(tick, chip id)`.
    pub chip_uses: [Vec<(u32, u16)>; 2],
    /// Buster-press ticks per side (`[local, remote]`).
    pub buster: [Vec<u32>; 2],
}

/// A game's usage-event fold: one round's per-tick samples (and its
/// custom spans) → that round's [`UsageEvents`]. This is the seam that
/// keeps game knowledge out of the stats fold: what a chip report MEANS
/// (a loaded pick departing? an acting chip arriving? a queue sum
/// dropping?) and whether B edges are buster shots are the game's
/// business, so the game's support hands the whole derivation over as
/// one opaque function (the mgba backend's `GameSupport::usage_fold`)
/// and this crate only runs it once per round, downstream of rollback
/// settlement and the stale-intro trim — the transition detection needs
/// the round's settled sample window, which is why it can't happen at
/// the per-tick pollers (pure reads, re-run under rollback).
pub type UsageFold = Box<dyn Fn(&[RoundSample], &[(u32, u32)]) -> UsageEvents + Send + Sync>;

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
    /// This game's usage-event fold (see [`UsageFold`]).
    usage: UsageFold,
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
    pub fn new(usage: UsageFold) -> Self {
        Self {
            usage,
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
        self.rounds
            .push(fold_round(outcome, &samples, &mut self.prev_final, &self.usage));
    }

    /// The stats as they stand: the rounds folded so far, plus the round
    /// in progress folded as an undecided round. A round the recording
    /// never finished — a mid-round disconnect, a truncated replay, an
    /// in-flight analysis' current round — keeps its telemetry and
    /// simply reports no outcome; nothing is dropped for want of a
    /// verdict.
    ///
    /// A clone (cheap: change-point curves and event lists, not raw
    /// samples), so an in-flight analysis can draw a live preview, and
    /// the live teardown can hand one copy to the sidecar writer and
    /// another to the results card, while the builder stays in place.
    /// Non-mutating: the in-progress fold runs on a scratch copy of the
    /// stale-trim state, so a later [`end_round`](Self::end_round)
    /// produces the identical final round.
    pub fn snapshot(&self) -> MatchStats {
        let mut rounds = self.rounds.clone();
        if !self.current.is_empty() {
            let mut prev_final = self.prev_final;
            rounds.push(fold_round(None, &self.current, &mut prev_final, &self.usage));
        }
        MatchStats { rounds }
    }

    /// Finish, folding any round still in progress as an undecided round
    /// — the consuming twin of [`snapshot`](Self::snapshot).
    pub fn finish(mut self) -> MatchStats {
        if !self.current.is_empty() {
            self.end_round(None);
        }
        MatchStats { rounds: self.rounds }
    }
}

/// The inert aggregator, for a game whose engine derives no usage
/// events: no chip uses, no buster counting — HP, custom spans and
/// outcomes still fold as usual.
impl Default for StatsBuilder {
    fn default() -> Self {
        StatsBuilder::new(Box::new(|_, _| UsageEvents::default()))
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
    usage: &UsageFold,
) -> RoundStats {
    let raw: Vec<(u32, u16, u16)> = samples.iter().map(|s| (s.tick, s.local, s.remote)).collect();
    let start = stale_prefix_len(*prev_final, &raw);
    *prev_final = samples.last().map(|s| (s.local, s.remote)).or(*prev_final);
    let samples = &samples[start..];
    let custom = custom_spans(samples.iter().map(|s| (s.tick, s.custom)));
    let events = usage(samples, &custom);
    RoundStats {
        outcome,
        hp: compress(samples.iter().map(|s| HpPoint {
            tick: s.tick,
            local: s.local,
            remote: s.remote,
        })),
        custom,
        chip_uses: events.chip_uses,
        buster: events.buster,
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
                return Err(ReadError::ImplausibleCount {
                    what: "hp point",
                    n: n_hp,
                });
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
    samples: Vec<(u32, crate::telemetry::BattleObs)>,
    events: Vec<(u32, crate::telemetry::RoundEvent)>,
    keys_at: &mut dyn FnMut(u32) -> Option<[u32; 2]>,
) {
    let mut push = |builder: &mut StatsBuilder, tick: u32, obs: crate::telemetry::BattleObs| {
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
            local: obs.units[local_player].hp,
            remote: obs.units[1 - local_player].hp,
            custom: obs.custom[local_player],
            buttons,
            // The stats format keeps the sentinel spelling; telemetry
            // carries the absence as `None`.
            chips: [obs.units[local_player].chip, obs.units[1 - local_player].chip].map(|c| c.unwrap_or(NO_CHIP)),
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
            crate::telemetry::RoundEvent::Started => {}
            crate::telemetry::RoundEvent::Ended { outcome } => {
                builder.end_round(outcome.map(|o| orient_outcome(o, local_player)));
            }
            crate::telemetry::RoundEvent::MatchEnded => {}
        }
    }
    for (tick, obs) in samples {
        push(builder, tick, obs);
    }
}

/// Absolute player outcome → `local_player`'s perspective.
pub fn orient_outcome(o: crate::telemetry::Outcome, local_player: usize) -> BattleOutcome {
    match o {
        crate::telemetry::Outcome::Draw => BattleOutcome::Draw,
        crate::telemetry::Outcome::P0Win => {
            if local_player == 0 {
                BattleOutcome::Win
            } else {
                BattleOutcome::Loss
            }
        }
        crate::telemetry::Outcome::P1Win => {
            if local_player == 1 {
                BattleOutcome::Win
            } else {
                BattleOutcome::Loss
            }
        }
    }
}
