//! Match statistics and their cached on-disk form.
//!
//! [`ReplaySet::analyze`](crate::ReplaySet::analyze) re-simulates a
//! recorded replay on a headless pair and extracts [`MatchStats`]: both
//! players' HP, the chips they used, the verdicts their games
//! announced, and where the rounds fell — all from the same telemetry
//! the live engine collects. That's a full replay simulation — seconds
//! of CPU — so stats are meant to be computed once and cached in a
//! small versioned binary sidecar (`<replay>.stats`, see
//! [`MatchStats::read`]/[`MatchStats::write`]). Live matches skip the
//! re-simulation entirely: the session folds each confirmed telemetry
//! batch into the same [`StatsBuilder`] as it plays — one aggregation
//! path, whichever side of the replay boundary the samples come from.
//!
//! Everything here is one flat series per kind, tick-stamped on the
//! recording's own timebase, with [`MatchStats::rounds`] marking where
//! the rounds begin. Rounds are annotations on the match, not containers
//! of it: nothing else knows about them, so they can arrive late (a
//! running analysis publishes each boundary as it reaches it) without
//! the series it annotates being rebuilt.

/// Outcome of a single round, from this side's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde_repr::Serialize_repr, serde_repr::Deserialize_repr)]
#[repr(i8)]
pub enum BattleOutcome {
    Draw = -1,
    Loss = 0,
    Win = 1,
}

/// Bumped whenever the sidecar format changes shape — or meaning.
/// Readers reject other versions (and anything without the magic) and
/// recompute. History up to v13 lives in the log; the running note
/// stops carrying it.
// v14: chip uses are captured as telemetry events by the game pollers
// (no more sample-fold derivation), and buster events are gone — B
// presses were never worth a lane. Older sidecars recompute.
// v15: bn5 and bn5ds read their newly-found hand blocks' fire cursors
// (the same contract as every other mainline family) instead of the
// loaded-cell departure heuristic — bn5's back-to-back duplicate picks
// were invisible to v14.
// v16: the stats went flat — one series per kind over the whole match,
// with a [`Round`] marking each boundary, instead of a Vec<RoundStats>
// carrying its own slice of everything — when replays stopped recording
// round marks of their own and the telemetry became the only thing that
// knows where a round begins.
// v17: a first round starting past tick 0 now MEANS the recording
// opens on a setup section (bn6 random battle — see
// [`MatchStats::has_setup`]). The poll-derived lifecycles (bn5ds,
// exeoss) used to report round 1 wherever their pollers first noticed
// the battle; they now anchor it at the priming handoff like the
// trap-anchored families, and v16 sidecars carrying the old late
// starts would read as phantom setups.
pub const FORMAT_VERSION: u32 = 17;

/// Sidecar file magic.
const MAGIC: &[u8; 4] = b"TGST";

/// Reader-side sanity cap on stored HP points — far above any real
/// match (HP moves a few dozen times a round), it only rejects corrupt
/// counts before allocating for them. The same idea sizes the caps on
/// the other series in [`MatchStats::read`].
const MAX_HP_POINTS: usize = 65536;

/// Per-match statistics, from the local player's perspective of the
/// replay (or live session) they came from.
///
/// Every series is tick-stamped on the recording's own timebase — the
/// recording is one contiguous input stream and the stats stay on it —
/// so a [`Round`] is a marker over those series rather than a container
/// of them. A consumer that wants round `i`'s slice of a series takes
/// the ticks inside [`round_span`](Self::round_span).
#[derive(Clone, Debug, Default)]
pub struct MatchStats {
    /// The match's full HP curve, losslessly change-point encoded: the
    /// first and last sample of each round plus every sample whose
    /// `(local, remote)` pair differs from the one before it. HP holds
    /// between entries (step semantics), so the per-tick series
    /// reconstructs exactly. Empty for a match that never got past a
    /// battle intro.
    pub hp: Vec<HpPoint>,
    /// `[start, end)` tick spans during which the custom screen (chip
    /// select) was open. Empty on games whose pollers don't report the
    /// flag.
    pub custom: Vec<(u32, u32)>,
    /// Chip-use events per side (`[local, remote]`): `(tick, chip id)`
    /// as the game's poller reported them. Empty on games that report
    /// no chips.
    pub chip_uses: [Vec<(u32, u16)>; 2],
    /// Where the rounds fall, in play order — the whole of what anything
    /// knows about round structure. Grows as an analysis reaches each
    /// boundary, so a partial fold is a truthful prefix rather than a
    /// wrong answer.
    pub rounds: Vec<Round>,
}

/// One round, as a marker over [`MatchStats`]'s series.
#[derive(Clone, Copy, Debug)]
pub struct Round {
    /// The tick the game's battle-start routine finished on. Where the
    /// round ENDS isn't stored: it's the next round's `start`, or — for
    /// the last one — wherever the recording runs out, which the stats
    /// don't know and [`MatchStats::round_span`] approximates.
    pub start: u32,
    /// The verdict the game's own result code announced, and the tick
    /// the round closed on. `None` for a round still in progress and for
    /// one that was never decided (the recording ended inside it, or a
    /// live round was torn down before a KO) — to a reader those are the
    /// same thing, and there is deliberately no HP-based inference.
    pub outcome: Option<(u32, BattleOutcome)>,
}

pub use crate::battle::RoundSample;

/// One HP reading.
#[derive(Clone, Copy, Debug)]
pub struct HpPoint {
    pub tick: u32,
    pub local: u16,
    pub remote: u16,
}

impl MatchStats {
    /// The recording's section boundaries: every inter-round transition,
    /// plus — when the recording opens on a setup section
    /// ([`has_setup`](Self::has_setup)) — the setup → round 1 boundary.
    /// This is what a scrub bar draws and what a sectioned export splits
    /// on. One rule covers both: a round start at tick 0 is where the
    /// match begins rather than a transition, and every later round
    /// start is a boundary of the section before it — round or setup.
    pub fn round_marks(&self) -> Vec<u32> {
        self.rounds.iter().map(|r| r.start).filter(|&s| s > 0).collect()
    }

    /// Whether the recording opens on a setup section: ticks before the
    /// first round, holding gameplay that is not a round — bn6 random
    /// battle's interactive setup (rank select, folder review) is the
    /// one producer, every other recording's round 1 starting at tick 0
    /// (its priming walk runs to the battle). The setup spans
    /// `[0, rounds[0].start)`; it carries no telemetry, but it is real
    /// recorded play, so sectioned consumers (scrubber, export) give it
    /// a section of its own.
    pub fn has_setup(&self) -> bool {
        self.rounds.first().is_some_and(|r| r.start > 0)
    }

    /// Round `i`'s `[start, end)` tick span. Every round but the last
    /// ends where the next begins; the last runs to `match_end` — the
    /// recording's own length, which the stats don't know and the caller
    /// does.
    ///
    /// Passing it is what makes the spans add up to the same timeline
    /// however much of the match has been folded: finding a boundary
    /// splits the last span in two rather than extending the total, so
    /// anything laid out against these spans keeps every tick at the
    /// same place while an analysis runs. Without it the last round
    /// stops at its verdict, or at its final reading — as far as the
    /// stats alone can see.
    pub fn round_span(&self, i: usize, match_end: Option<u32>) -> Option<(u32, u32)> {
        let round = self.rounds.get(i)?;
        let end = match self.rounds.get(i + 1) {
            Some(next) => next.start,
            None => match match_end.or(round.outcome.map(|(tick, _)| tick)) {
                Some(end) => end,
                None => self.last_tick().map_or(round.start, |t| t + 1),
            },
        };
        Some((round.start, end.max(round.start)))
    }

    /// The last tick any series reached.
    fn last_tick(&self) -> Option<u32> {
        [
            self.hp.last().map(|p| p.tick),
            self.custom.last().map(|&(_, end)| end),
            self.chip_uses[0].last().map(|&(t, _)| t),
            self.chip_uses[1].last().map(|&(t, _)| t),
        ]
        .into_iter()
        .flatten()
        .max()
    }
}

/// Incremental [`MatchStats`] aggregator — THE stats construction path,
/// live or offline. Feed it every confirmed tick's [`RoundSample`] and
/// chip-use event as they arrive, and mark the boundaries with
/// [`start_round`](Self::start_round) / [`end_round`](Self::end_round).
/// Everything it takes is already settled — the live engine's
/// speculative side is revoked inside the telemetry store, upstream of
/// the confirmed batches this consumes — and the replay re-simulation
/// ([`analyze`]) pushes from its playback loop through the same calls,
/// which is what keeps live stats and offline re-analysis
/// byte-equivalent.
///
/// The fold is genuinely incremental: samples land in the flat series as
/// they arrive, so a round boundary costs a tick in `round_starts` and
/// nothing else, and [`snapshot`](Self::snapshot) is a clone rather than
/// a re-fold.
#[derive(Default)]
pub struct StatsBuilder {
    stats: MatchStats,
    /// The last `(local, remote)` pair observed, whether it was recorded
    /// or trimmed. A new round's stale intro is exactly the samples
    /// repeating it — see [`Self::start_round`].
    last_pair: Option<(u16, u16)>,
    /// The last tick recorded, for closing an open custom span at a
    /// round's end.
    last_tick: Option<u32>,
    /// The last tick offered, recorded or not — the offline analyzer
    /// polls once per frame and can observe a tick twice.
    last_seen_tick: Option<u32>,
    /// A sample repeating the curve's last point, held back as the
    /// candidate final point of the round in progress: it is recorded
    /// only if nothing changes before the round closes, so each round's
    /// trace reaches its own right edge without every repeat costing an
    /// entry.
    pending: Option<HpPoint>,
    /// A custom span opened and not yet closed.
    custom_open: Option<u32>,
    /// While the round in progress is still repeating the previous
    /// round's final HP — see [`Self::start_round`].
    trimming: bool,
}

impl StatsBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a round at `tick`, recording the boundary.
    ///
    /// This also arms the stale-intro trim. The unit slots re-initialize
    /// partway into the battle intro, so until then the pollers relay
    /// whatever the slots still hold: the PREVIOUS round's final values
    /// (or, for a match's first round on games whose slots map
    /// immediately, the zeroed fresh memory). That prefix is exactly the
    /// samples equal to `last_pair` — the first differing sample IS the
    /// re-init write, and everything before it is dropped.
    ///
    /// Assumes rounds restore HP (true of every supported game's link
    /// battles): under a hypothetical carry-over rule the first
    /// round-open samples would be indistinguishable from the stale
    /// prefix.
    pub fn start_round(&mut self, tick: u32) {
        self.close_round_series();
        self.stats.rounds.push(Round {
            start: tick,
            outcome: None,
        });
        self.trimming = true;
    }

    /// Append one simulated tick's sample.
    pub fn push_sample(&mut self, sample: RoundSample) {
        if self.last_seen_tick == Some(sample.tick) {
            return;
        }
        self.last_seen_tick = Some(sample.tick);
        let pair = (sample.local, sample.remote);
        if self.trimming {
            if pair == self.last_pair.unwrap_or((0, 0)) {
                // Still the previous round's state showing through.
                self.last_pair = Some(pair);
                return;
            }
            self.trimming = false;
        }
        self.last_pair = Some(pair);
        self.last_tick = Some(sample.tick);

        let point = HpPoint {
            tick: sample.tick,
            local: sample.local,
            remote: sample.remote,
        };
        match self.stats.hp.last() {
            // A repeat: remember it as the candidate final point.
            Some(prev) if (prev.local, prev.remote) == pair => self.pending = Some(point),
            _ => {
                self.stats.hp.push(point);
                self.pending = None;
            }
        }

        match (sample.custom, self.custom_open) {
            (true, None) => self.custom_open = Some(sample.tick),
            (false, Some(start)) => {
                self.stats.custom.push((start, sample.tick));
                self.custom_open = None;
            }
            _ => {}
        }
    }

    /// Record a chip use. `side` is 0 for local, 1 for remote — already
    /// oriented by the caller.
    pub fn push_chip_use(&mut self, side: usize, tick: u32, chip: u16) {
        self.stats.chip_uses[side].push((tick, chip));
    }

    /// Close the round in progress at `tick`. `outcome` is `None` when
    /// the round was never decided (the recording ended mid-round, or a
    /// live round was torn down without reaching a KO), in which case
    /// the round keeps the `None` it opened with — an absent verdict IS
    /// the undecided answer.
    pub fn end_round(&mut self, tick: u32, outcome: Option<BattleOutcome>) {
        self.close_round_series();
        if let (Some(round), Some(outcome)) = (self.stats.rounds.last_mut(), outcome) {
            round.outcome = Some((tick, outcome));
        }
    }

    /// Settle the series at a round boundary: flush the held final point
    /// and close any custom span still open. Idempotent, so the boundary
    /// can be reached from a close, a start, or the end of the fold.
    fn close_round_series(&mut self) {
        if let Some(point) = self.pending.take() {
            self.stats.hp.push(point);
        }
        if let (Some(start), Some(last)) = (self.custom_open.take(), self.last_tick) {
            self.stats.custom.push((start, last + 1));
        }
    }

    /// The stats as they stand — including the round in progress, whose
    /// telemetry is kept and whose verdict is simply absent. Nothing is
    /// dropped for want of a boundary: a mid-round disconnect, a
    /// truncated replay and an in-flight analysis all read the same way.
    ///
    /// A clone (cheap: change-point curves and event lists, not raw
    /// samples), so an in-flight analysis can draw a live preview, and
    /// the live teardown can hand one copy to the sidecar writer and
    /// another to the results card, while the builder stays in place.
    /// Non-mutating: the boundary settle runs on the copy, so a later
    /// [`end_round`](Self::end_round) produces the identical result.
    pub fn snapshot(&self) -> MatchStats {
        let mut out = self.stats.clone();
        if let Some(point) = self.pending {
            out.hp.push(point);
        }
        if let (Some(start), Some(last)) = (self.custom_open, self.last_tick) {
            out.custom.push((start, last + 1));
        }
        out
    }

    /// Finish, settling the round still in progress — the consuming twin
    /// of [`snapshot`](Self::snapshot).
    pub fn finish(mut self) -> MatchStats {
        self.close_round_series();
        self.stats
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
        // One series after another, each a count and then its entries,
        // and the rounds last — the same order [`Self::write`] lays them
        // down. Every count is checked against a cap far above anything
        // a real match produces, so a corrupt one is rejected rather
        // than allocated for.
        let n_hp = u32_of(&mut r)?;
        if n_hp as usize > MAX_HP_POINTS {
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
        if n_custom > 8192 {
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
            if n > 8192 {
                return Err(ReadError::ImplausibleCount { what: "chip-use", n });
            }
            for _ in 0..n {
                side.push((u32_of(&mut r)?, u16_of(&mut r)?));
            }
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
            let start = u32_of(&mut r)?;
            let mut tag = [0u8; 1];
            r.read_exact(&mut tag)?;
            let outcome = match tag[0] as i8 {
                // Undecided: no verdict, and no tick to go with one.
                -2 => None,
                -1 => Some(BattleOutcome::Draw),
                0 => Some(BattleOutcome::Loss),
                1 => Some(BattleOutcome::Win),
                other => return Err(ReadError::BadOutcomeTag(other)),
            };
            let outcome = match outcome {
                Some(o) => Some((u32_of(&mut r)?, o)),
                None => None,
            };
            rounds.push(Round { start, outcome });
        }
        Ok(MatchStats {
            hp,
            custom,
            chip_uses,
            rounds,
        })
    }

    pub fn write(&self, mut w: impl std::io::Write) -> std::io::Result<()> {
        w.write_all(MAGIC)?;
        w.write_all(&FORMAT_VERSION.to_le_bytes())?;
        w.write_all(&(self.hp.len() as u32).to_le_bytes())?;
        for p in &self.hp {
            w.write_all(&p.tick.to_le_bytes())?;
            w.write_all(&p.local.to_le_bytes())?;
            w.write_all(&p.remote.to_le_bytes())?;
        }
        w.write_all(&(self.custom.len() as u32).to_le_bytes())?;
        for &(a, b) in &self.custom {
            w.write_all(&a.to_le_bytes())?;
            w.write_all(&b.to_le_bytes())?;
        }
        for side in &self.chip_uses {
            w.write_all(&(side.len() as u32).to_le_bytes())?;
            for &(t, id) in side {
                w.write_all(&t.to_le_bytes())?;
                w.write_all(&id.to_le_bytes())?;
            }
        }
        w.write_all(&(self.rounds.len() as u32).to_le_bytes())?;
        for round in &self.rounds {
            w.write_all(&round.start.to_le_bytes())?;
            let tag: i8 = match round.outcome {
                None => -2,
                Some((_, o)) => o as i8,
            };
            w.write_all(&tag.to_le_bytes())?;
            if let Some((tick, _)) = round.outcome {
                w.write_all(&tick.to_le_bytes())?;
            }
        }
        Ok(())
    }
}

/// Fold a batch of **confirmed** telemetry into a [`StatsBuilder`]:
/// per-tick samples become [`RoundSample`]s, `RoundStarted`/`RoundEnded`
/// events open and close rounds (with their outcome oriented to
/// `local_player`), and `ChipUsed` events land on their side's lane.
/// Shared by the live drive loop and [`analyze`], so the two produce
/// identical stats for the same match.
///
/// Samples and events merge in tick order, events first at a shared
/// tick: the `RoundEnded` that closes a round at tick T must fold before
/// T's own sample — which belongs to the NEW round — is pushed, or a
/// batch spanning a round boundary would fold the next round's first
/// samples into the closing one.
pub fn fold_confirmed(
    builder: &mut StatsBuilder,
    local_player: usize,
    samples: Vec<(u32, crate::telemetry::BattleObs)>,
    events: Vec<(u32, crate::telemetry::Event)>,
) {
    let push = |builder: &mut StatsBuilder, tick: u32, obs: crate::telemetry::BattleObs| {
        builder.push_sample(RoundSample {
            tick,
            local: obs.units[local_player].hp,
            remote: obs.units[1 - local_player].hp,
            custom: obs.custom[local_player],
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
            crate::telemetry::Event::RoundStarted => builder.start_round(etick),
            crate::telemetry::Event::RoundEnded { outcome } => {
                builder.end_round(etick, outcome.map(|o| orient_outcome(o, local_player)));
            }
            crate::telemetry::Event::MatchEnded => {}
            crate::telemetry::Event::ChipUsed { player, chip } => {
                builder.push_chip_use(if player == local_player { 0 } else { 1 }, etick, chip);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{Event, Outcome, UnitObs};

    fn obs(p0: u16, p1: u16, custom: bool) -> crate::telemetry::BattleObs {
        crate::telemetry::BattleObs {
            units: [
                UnitObs { hp: p0, tile: (1, 2) },
                UnitObs { hp: p1, tile: (4, 2) },
            ],
            custom: [custom, custom],
        }
    }

    /// A two-round match, folded exactly as the live drive loop and the
    /// offline re-simulation both fold it: samples and events interleaved
    /// in tick order, events first at a shared tick.
    fn two_round_match() -> MatchStats {
        let mut b = StatsBuilder::new();
        // Round 1 opens with the priming baseline at tick 0.
        fold_confirmed(&mut b, 0, vec![], vec![(0, Event::RoundStarted)]);
        let mut samples = vec![];
        for tick in 1..=4 {
            // The intro repeats the zeroed slots until the unit structs
            // re-init at tick 3.
            samples.push((tick, if tick < 3 { obs(0, 0, false) } else { obs(100, 100, false) }));
        }
        samples.push((5, obs(100, 90, true)));
        samples.push((6, obs(80, 90, true)));
        samples.push((7, obs(80, 0, false)));
        fold_confirmed(&mut b, 0, samples, vec![(6, Event::ChipUsed { player: 0, chip: 42 })]);
        // Round 2 starts at 10, closing round 1 with p0's win.
        fold_confirmed(
            &mut b,
            0,
            vec![
                // The slots still hold round 1's final pair until 12.
                (10, obs(80, 0, false)),
                (11, obs(80, 0, false)),
                (12, obs(100, 100, false)),
                (13, obs(100, 40, false)),
            ],
            vec![
                (10, Event::RoundEnded { outcome: Some(Outcome::P0Win) }),
                (10, Event::RoundStarted),
            ],
        );
        b.finish()
    }

    #[test]
    fn the_fold_marks_rounds_over_one_flat_series() {
        let stats = two_round_match();
        assert_eq!(stats.rounds.len(), 2);
        assert_eq!(stats.rounds[0].start, 0);
        assert_eq!(stats.rounds[0].outcome, Some((10, BattleOutcome::Win)));
        assert_eq!(stats.rounds[1].start, 10);
        // Round 2 never closed — the fold ended inside it.
        assert_eq!(stats.rounds[1].outcome, None);
        assert_eq!(stats.round_marks(), vec![10]);

        // The HP curve is one series across both rounds. The stale intro
        // prefixes (ticks 1-2 repeating the zero pair, ticks 10-11
        // repeating round 1's final pair) are gone from it, and so is
        // tick 4, which only repeated tick 3 — HP holds between entries,
        // so dropping a repeat loses nothing.
        let curve: Vec<_> = stats.hp.iter().map(|p| (p.tick, p.local, p.remote)).collect();
        assert_eq!(
            curve,
            vec![
                (3, 100, 100),
                (5, 100, 90),
                (6, 80, 90),
                (7, 80, 0),
                (12, 100, 100),
                (13, 100, 40),
            ]
        );
        // Custom span and chip use land on the same timebase, unsegmented.
        assert_eq!(stats.custom, vec![(5, 7)]);
        assert_eq!(stats.chip_uses[0], vec![(6, 42)]);
        assert!(stats.chip_uses[1].is_empty());
    }

    /// A recording that opens on a setup section (bn6 random battle: the
    /// interactive rank/folder phase before round 1) marks the setup →
    /// round 1 boundary too, so sectioned consumers split there; a
    /// recording whose round 1 starts at tick 0 reports no setup and its
    /// marks stay the inter-round transitions alone.
    #[test]
    fn a_late_first_round_is_a_setup_section() {
        let no_setup = two_round_match();
        assert!(!no_setup.has_setup());
        assert_eq!(no_setup.round_marks(), vec![10]);

        let mut b = StatsBuilder::new();
        fold_confirmed(
            &mut b,
            0,
            vec![(300, obs(100, 100, false)), (301, obs(100, 90, false))],
            vec![(275, Event::RoundStarted)],
        );
        let stats = b.finish();
        assert!(stats.has_setup());
        assert_eq!(stats.round_marks(), vec![275]);
    }

    /// Rounds are markers, so a span is settled by its neighbours plus
    /// the recording's length — and the spans tile the whole recording
    /// however much of it has been folded, which is what keeps a chart
    /// drawn against them from reflowing mid-analysis.
    #[test]
    fn round_spans_tile_the_recording() {
        let stats = two_round_match();
        assert_eq!(stats.round_span(0, Some(200)), Some((0, 10)));
        assert_eq!(stats.round_span(1, Some(200)), Some((10, 200)));
        assert_eq!(stats.round_span(2, Some(200)), None);
        let (_, total_end) = stats.round_span(stats.rounds.len() - 1, Some(200)).unwrap();
        assert_eq!(total_end, 200);

        // Mid-analysis, with only round 1 found, that round holds the
        // whole timeline — so finding round 2 splits this span rather
        // than extending the total.
        let mut b = StatsBuilder::new();
        fold_confirmed(&mut b, 0, vec![], vec![(0, Event::RoundStarted)]);
        let partial = b.snapshot();
        assert_eq!(partial.round_span(0, Some(200)), Some((0, 200)));

        // Without a length, the last round stops at its verdict — or, with
        // none, at the last thing observed.
        assert_eq!(stats.round_span(0, None), Some((0, 10)));
        assert_eq!(stats.round_span(1, None), Some((10, 14)));
    }

    /// The local player's perspective picks which unit is "local", and
    /// the verdict is oriented to match.
    #[test]
    fn the_fold_orients_to_the_local_player() {
        let mut b = StatsBuilder::new();
        fold_confirmed(&mut b, 1, vec![], vec![(0, Event::RoundStarted)]);
        fold_confirmed(
            &mut b,
            1,
            vec![(1, obs(100, 60, false))],
            vec![(2, Event::RoundEnded { outcome: Some(Outcome::P0Win) })],
        );
        let stats = b.finish();
        assert_eq!(stats.hp[0].local, 60);
        assert_eq!(stats.hp[0].remote, 100);
        assert_eq!(stats.rounds[0].outcome, Some((2, BattleOutcome::Loss)));
    }

    /// A snapshot mid-fold must equal what finishing produces, or a live
    /// preview and the cached sidecar would disagree about the same match.
    #[test]
    fn a_snapshot_matches_the_finished_fold() {
        let mut b = StatsBuilder::new();
        fold_confirmed(&mut b, 0, vec![], vec![(0, Event::RoundStarted)]);
        fold_confirmed(
            &mut b,
            0,
            vec![(1, obs(100, 100, true)), (2, obs(100, 100, true)), (3, obs(90, 100, true))],
            vec![],
        );
        let snap = b.snapshot();
        let done = b.finish();
        assert_eq!(
            snap.hp.iter().map(|p| p.tick).collect::<Vec<_>>(),
            done.hp.iter().map(|p| p.tick).collect::<Vec<_>>()
        );
        assert_eq!(snap.custom, done.custom);
        assert_eq!(snap.rounds.len(), done.rounds.len());
    }

    #[test]
    fn the_sidecar_roundtrips() {
        let stats = two_round_match();
        let mut buf = Vec::new();
        stats.write(&mut buf).unwrap();
        let back = MatchStats::read(&buf[..]).unwrap();
        assert_eq!(
            back.hp.iter().map(|p| (p.tick, p.local, p.remote)).collect::<Vec<_>>(),
            stats.hp.iter().map(|p| (p.tick, p.local, p.remote)).collect::<Vec<_>>()
        );
        assert_eq!(back.custom, stats.custom);
        assert_eq!(back.chip_uses, stats.chip_uses);
        assert_eq!(
            back.rounds.iter().map(|r| (r.start, r.outcome)).collect::<Vec<_>>(),
            stats.rounds.iter().map(|r| (r.start, r.outcome)).collect::<Vec<_>>()
        );
    }

    /// A sidecar from the per-round era is rejected rather than
    /// misparsed, so the host recomputes it.
    #[test]
    fn an_older_sidecar_is_rejected() {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&15u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 32]);
        assert!(matches!(
            MatchStats::read(&buf[..]),
            Err(ReadError::UnsupportedVersion(15))
        ));
    }
}
