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
//! already collected the same series, and
//! [`MatchStats::from_round_reports`] converts it at teardown.
//!
//! [`InnerState::set_battle_hp`]: crate::stepper::InnerState::set_battle_hp

use crate::stepper::BattleOutcome;

/// Bumped whenever the sidecar format changes shape; readers reject
/// other versions (and anything without the magic, e.g. the short-lived
/// JSON v1 sidecars) and recompute.
pub const FORMAT_VERSION: u32 = 2;

/// Sidecar file magic.
const MAGIC: &[u8; 4] = b"TGST";

/// Cap on stored HP points per round. HP is a step function that holds
/// for long stretches, so a few hundred points reproduce the curve
/// exactly enough for any UI-scale chart while keeping sidecars small.
pub const HP_POINTS_PER_ROUND: usize = 512;

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
    /// Decimated to at most [`HP_POINTS_PER_ROUND`] points, final
    /// sample always kept. Empty on rounds that never got past the
    /// battle intro (and for replays predating HP reporting).
    pub hp: Vec<HpPoint>,
    /// `[start, end)` tick spans during which the custom screen (chip
    /// select) was open. Empty on games whose traps don't report the
    /// flag.
    pub custom: Vec<(u32, u32)>,
}

/// One HP reading.
#[derive(Clone, Copy, Debug)]
pub struct HpPoint {
    pub tick: u32,
    pub local: u16,
    pub remote: u16,
}

impl MatchStats {
    /// Convert a live match's per-round reports (see
    /// [`crate::battle::RoundReport`]) — no re-simulation.
    pub fn from_round_reports(reports: &[crate::battle::RoundReport]) -> Self {
        let mut prev_final: Option<(u16, u16)> = None;
        Self {
            rounds: reports
                .iter()
                .map(|r| {
                    let raw: Vec<(u32, u16, u16)> = r.hp.iter().map(|s| (s.tick, s.local, s.remote)).collect();
                    let start = stale_prefix_len(prev_final, &raw);
                    prev_final = r.hp.last().map(|s| (s.local, s.remote)).or(prev_final);
                    let hp = &r.hp[start..];
                    RoundStats {
                        outcome: Some(r.outcome),
                        hp: decimate(hp.iter().map(|s| HpPoint {
                            tick: s.tick,
                            local: s.local,
                            remote: s.remote,
                        })),
                        custom: custom_spans(hp.iter().map(|s| (s.tick, s.custom))),
                    }
                })
                .collect(),
        }
    }

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
            if n_hp as usize > HP_POINTS_PER_ROUND {
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
            rounds.push(RoundStats { outcome, hp, custom });
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

/// Thin `points` to at most [`HP_POINTS_PER_ROUND`], always keeping the
/// final sample (the KO floor).
fn decimate(points: impl ExactSizeIterator<Item = HpPoint> + Clone) -> Vec<HpPoint> {
    let n = points.len();
    if n == 0 {
        return vec![];
    }
    let step = n.div_ceil(HP_POINTS_PER_ROUND).max(1);
    let last = points.clone().last().unwrap();
    let mut out: Vec<HpPoint> = points.step_by(step).collect();
    if !(n - 1).is_multiple_of(step) {
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

    let total_ticks: u32 = replay.rounds.iter().map(|r| r.len() as u32).sum();
    let mut last_reported: u32 = 0;

    let lpi = replay.local_player_index as usize;
    let mut rounds: Vec<Vec<HpPoint>> = vec![vec![]];
    let mut customs: Vec<Vec<(u32, bool)>> = vec![vec![]];
    let mut outcomes: Vec<Option<BattleOutcome>> = vec![];
    // Latest result seen for the round in progress. `round_result()`
    // clears across the round transition, so it's committed on the
    // round-index edge (or at the end of the drain for the last round).
    let mut current_result: Option<BattleOutcome> = None;
    let mut last_round_idx: u32 = 0;
    let mut frames_after_drain: u32 = 0;

    loop {
        let (total_left, abs_tick, round_idx, ended, result, tick, hp, custom) = {
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
            )
        };

        if abs_tick != last_reported {
            last_reported = abs_tick;
            on_progress(abs_tick, total_ticks);
        }

        if round_idx != last_round_idx {
            outcomes.push(current_result.take());
            rounds.push(vec![]);
            customs.push(vec![]);
            last_round_idx = round_idx;
        }
        if let Some(rr) = result {
            current_result = Some(rr.outcome);
        }
        // One point per tick: the traps re-report every tick, so a new
        // tick with a reading is exactly one sample. (`battle_hp` is
        // cleared across round transitions and stays `None` through
        // each battle intro.)
        if let Some(hp) = hp {
            let series = rounds.last_mut().unwrap();
            if series.last().map(|p| p.tick) != Some(tick) {
                series.push(HpPoint {
                    tick,
                    local: hp[lpi],
                    remote: hp[1 - lpi],
                });
                customs.last_mut().unwrap().push((tick, custom.unwrap_or(false)));
            }
        }

        if total_left == 0 && abs_tick > 0 {
            if (ended && current_result.is_some()) || frames_after_drain >= MAX_DRAIN_FRAMES {
                outcomes.push(current_result.take());
                break;
            }
            frames_after_drain += 1;
        }

        core.as_mut().run_frame();
    }

    let mut prev_final: Option<(u16, u16)> = None;
    Ok(MatchStats {
        rounds: outcomes
            .into_iter()
            .zip(rounds.into_iter().zip(customs))
            .map(|(outcome, (hp, custom))| {
                // Drop the stale intro prefix (customs were pushed in
                // lockstep with hp, so the same cut applies).
                let raw: Vec<(u32, u16, u16)> = hp.iter().map(|p| (p.tick, p.local, p.remote)).collect();
                let start = stale_prefix_len(prev_final, &raw);
                prev_final = hp.last().map(|p| (p.local, p.remote)).or(prev_final);
                RoundStats {
                    outcome,
                    hp: decimate(hp[start..].iter().copied()),
                    custom: custom_spans(custom[start.min(custom.len())..].iter().copied()),
                }
            })
            .collect(),
    })
}
