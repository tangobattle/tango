//! Offline evaluation of remote-input predictors against a replay corpus.
//!
//! The live rollback engine guesses the remote player's joyflags for ticks whose
//! real input hasn't arrived yet ([`MgbaWorld::predict`]); every wrong guess
//! discards the speculative tail, re-simulates it, and visibly pops the
//! displayed state. This harness replays recorded input streams through the
//! real [`getgud::Session`] bookkeeping over a lightweight stand-in world, so
//! candidate predictors can be compared on real matches without touching an
//! emulator core: rollback frequency, depth distribution, re-sim cost, and how
//! often the presented frame's newest tick was built on a wrong guess.
//!
//! Network model: constant lag. The remote input for tick `i` becomes available
//! `depth + 1` local ticks later, which holds the presented frame's speculation
//! depth constant at `depth` — the live equivalent of `lead − present_delay`.
//! Real links jitter around that; pinning it isolates predictor quality from
//! delay tuning.
//!
//! Every candidate is a held-button mask (`predict = last & MASK`). The
//! engine's `predict` is a pure function iterated along the speculation chain
//! (see `Session::speculate_to`), so masks are the natural expressible family;
//! anything age-dependent would need an engine API change. `oracle@d` is the
//! per-button optimal mask computed from the corpus itself at horizon `d`:
//! repeat a button iff it flips over `d` ticks less often than it's held.
//!
//! Usage: cargo run --release -p tango-pvp --example predictor-eval [-- <replay file/dir>...]
//! (defaults to ~/Documents/Tango/replays)

use std::cell::Cell;
use std::rc::Rc;

/// Speculation depths to sweep: how many predicted remote ticks are baked into
/// the presented frame. Live, this is `lead − present_delay`; a few ticks is a
/// well-tuned link, 8 is a badly undertuned one.
const DEPTHS: &[u32] = &[1, 2, 3, 4, 6, 8];

/// Ignore rounds shorter than this (menu blips, aborted rounds).
const MIN_ROUND_TICKS: usize = 64;

/// GBA keypad bits, in KEYINPUT order.
const BUTTONS: [(&str, u16); 10] = [
    ("A", 0x001),
    ("B", 0x002),
    ("Select", 0x004),
    ("Start", 0x008),
    ("Right", 0x010),
    ("Left", 0x020),
    ("Up", 0x040),
    ("Down", 0x080),
    ("R", 0x100),
    ("L", 0x200),
];

const A: u16 = 0x001;
const B: u16 = 0x002;
const DPAD: u16 = 0x0f0;
const LR: u16 = 0x300;
const ALL: u16 = 0x3ff;

/// The fixed candidate masks. Each depth additionally evaluates its own
/// corpus-derived `oracle@d` mask.
const BASE_CANDIDATES: [(&str, u16); 5] = [
    ("none (release all)", 0),
    ("A|B (current)", A | B),
    ("A|B+dpad", A | B | DPAD),
    ("A|B+L|R", A | B | LR),
    ("all (repeat last)", ALL),
];

/// Stand-in [`getgud::World`]: state is just the parked tick, `step` only
/// counts. Prediction correctness is all that matters for rollback accounting —
/// the session compares predicted joyflags against confirmed ones — so the
/// session's promote/rollback behavior over this world is identical to the live
/// engine's over mgba.
struct EvalWorld {
    mask: u16,
    parked: u32,
    steps: Rc<Cell<u64>>,
}

impl getgud::World for EvalWorld {
    type Input = tango_pvp::input::PartialInput;
    type State = u32;
    type Error = std::convert::Infallible;

    fn step(&mut self, _input: (Self::Input, Self::Input)) -> Result<getgud::RoundState, Self::Error> {
        self.parked += 1;
        self.steps.set(self.steps.get() + 1);
        Ok(getgud::RoundState::Ongoing)
    }

    fn save(&mut self) -> Result<u32, Self::Error> {
        Ok(self.parked)
    }

    fn load(&mut self, state: &u32) -> Result<(), Self::Error> {
        self.parked = *state;
        Ok(())
    }

    fn predict(&self, last: &Self::Input) -> Self::Input {
        tango_pvp::input::PartialInput {
            joyflags: last.joyflags & self.mask,
        }
    }

    fn log(&mut self, _pair: &(Self::Input, Self::Input)) {}
}

/// Aggregated results of one or more [`run_round`]s at a single (depth, mask).
#[derive(Clone)]
struct RunAgg {
    frames: u64,
    rollbacks: u64,
    /// Rollback events by discarded depth (index = depth, saturating at the
    /// top bucket). Depth never exceeds the swept speculation depth.
    depth_hist: [u64; 16],
    /// Frames whose presented tick consumed a predicted remote input that
    /// turned out wrong — "the newest thing on screen was built on a bad guess".
    wrong_tip: u64,
    /// Total `World::step` calls — 1/frame in steady state; rollback re-sims
    /// add the discarded tail on top. Proxy for emulator CPU cost.
    steps: u64,
}

impl Default for RunAgg {
    fn default() -> Self {
        Self {
            frames: 0,
            rollbacks: 0,
            depth_hist: [0; 16],
            wrong_tip: 0,
            steps: 0,
        }
    }
}

impl RunAgg {
    fn merge(&mut self, other: &RunAgg) {
        self.frames += other.frames;
        self.rollbacks += other.rollbacks;
        for (a, b) in self.depth_hist.iter_mut().zip(other.depth_hist.iter()) {
            *a += b;
        }
        self.wrong_tip += other.wrong_tip;
        self.steps += other.steps;
    }

    /// Depth of the rollback event at the given quantile (0..=1), or 0 if none.
    fn depth_quantile(&self, q: f64) -> u32 {
        let target = (self.rollbacks as f64 * q).ceil() as u64;
        let mut seen = 0;
        for (depth, &count) in self.depth_hist.iter().enumerate() {
            seen += count;
            if count > 0 && seen >= target {
                return depth as u32;
            }
        }
        0
    }

    fn depth_max(&self) -> u32 {
        self.depth_hist
            .iter()
            .rposition(|&c| c > 0)
            .map(|d| d as u32)
            .unwrap_or(0)
    }
}

/// Drive one round's input streams through a real [`getgud::Session`] at a
/// fixed speculation depth with the given predictor mask.
fn run_round(local: &[u16], remote: &[u16], depth: u32, mask: u16) -> RunAgg {
    // Remote input for tick `i` arrives at local tick `i + lag`; the presented
    // frame (present_delay 0) then carries exactly `depth` predicted ticks.
    let lag = depth as usize + 1;
    let steps = Rc::new(Cell::new(0u64));
    let mut session = getgud::Session::new(getgud::SessionParams {
        present_delay: 0,
        initial_remote: tango_pvp::input::PartialInput { joyflags: 0 },
        initial_state: 0u32,
        world: EvalWorld {
            mask,
            parked: 0,
            steps: steps.clone(),
        },
    });

    let mut agg = RunAgg::default();
    let n = local.len().min(remote.len());
    let mut delivered = 0usize;
    for t in 0..n {
        while delivered + lag <= t {
            session.add_remote_input(
                tango_pvp::input::PartialInput {
                    joyflags: remote[delivered],
                },
                0,
            );
            delivered += 1;
        }
        let (tick, tip_remote) = {
            let frame = session
                .advance(tango_pvp::input::PartialInput { joyflags: local[t] })
                .unwrap();
            (frame.tick, frame.input.1.joyflags)
        };
        agg.frames += 1;
        // The presented tick consumed pair `tick − 1`; if that pair isn't
        // confirmed yet its remote half is a prediction — check it against
        // what the peer actually pressed.
        if tick >= 1 {
            let idx = (tick - 1) as usize;
            if idx >= delivered && idx < n && tip_remote != remote[idx] {
                agg.wrong_tip += 1;
            }
        }
        let d = session.last_misprediction_depth();
        if d > 0 {
            agg.rollbacks += 1;
            agg.depth_hist[(d as usize).min(agg.depth_hist.len() - 1)] += 1;
        }
    }
    agg.steps = steps.get();
    agg
}

/// Validate the harness's arrival model against streams with known answers.
fn selftest() {
    let local = vec![0u16; 600];
    // Idle for 300 ticks, then A|B held for the rest: repeat-last eats exactly
    // one rollback (the transition), release-all mispredicts every held tick.
    let mut remote = vec![0u16; 300];
    remote.extend(std::iter::repeat(A | B).take(300));

    let repeat_all = run_round(&local, &remote, 4, ALL);
    assert_eq!(repeat_all.rollbacks, 1, "repeat-last must miss only the transition");
    let release_all = run_round(&local, &remote, 4, 0);
    assert!(release_all.rollbacks > 250, "release-all must miss every held tick");

    let idle = vec![0u16; 600];
    let on_idle = run_round(&local, &idle, 4, 0);
    assert_eq!(on_idle.rollbacks, 0, "no rollbacks on an idle stream");
    assert_eq!(on_idle.wrong_tip, 0);
}

/// One round's two input streams, as stored in the replay (which side is
/// "local" doesn't matter here — the sweep runs both directions).
struct Round {
    p1: Vec<u16>,
    p2: Vec<u16>,
}

struct Corpus {
    rounds: Vec<Round>,
    files: usize,
    matches: usize,
    undecodable: usize,
}

/// Load every replay under the given paths, deduplicating the two per-match
/// perspective files by RNG seed (both sides record identical input pairs).
fn load_corpus(paths: &[std::path::PathBuf]) -> Corpus {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            let entries = match std::fs::read_dir(path) {
                Ok(entries) => entries,
                Err(e) => {
                    eprintln!("skipping {}: {}", path.display(), e);
                    continue;
                }
            };
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "tangoreplay") {
                    files.push(p);
                }
            }
        } else {
            files.push(path.clone());
        }
    }

    let mut corpus = Corpus {
        rounds: Vec::new(),
        files: files.len(),
        matches: 0,
        undecodable: 0,
    };
    let mut seen = std::collections::HashSet::<[u8; 16]>::new();
    for path in &files {
        let replay = match std::fs::File::open(path)
            .map_err(std::io::Error::from)
            .and_then(|f| tango_pvp::replay::Replay::decode(f))
        {
            Ok(replay) => replay,
            Err(e) => {
                eprintln!("undecodable {}: {}", path.display(), e);
                corpus.undecodable += 1;
                continue;
            }
        };
        if !seen.insert(replay.rng_seed) {
            continue;
        }
        corpus.matches += 1;
        for range in replay.round_ranges() {
            let round = &replay.inputs[range];
            if round.len() < MIN_ROUND_TICKS {
                continue;
            }
            corpus.rounds.push(Round {
                p1: round.iter().map(|(l, _)| l.joyflags).collect(),
                p2: round.iter().map(|(_, r)| r.joyflags).collect(),
            });
        }
    }
    corpus
}

/// Per-(depth, button) prediction-error counts over every stream: repeating the
/// button is wrong when it flips across the horizon; predicting it released is
/// wrong when it's held at the target tick. Whichever errs less belongs in the
/// mask at that horizon.
#[derive(Clone, Copy, Default)]
struct BitStat {
    flips: u64,
    held: u64,
    samples: u64,
}

fn bit_stats(rounds: &[Round]) -> Vec<[BitStat; 10]> {
    let mut stats = vec![[BitStat::default(); 10]; DEPTHS.len()];
    for round in rounds {
        for stream in [&round.p1, &round.p2] {
            for (di, &d) in DEPTHS.iter().enumerate() {
                let d = d as usize;
                if stream.len() <= d {
                    continue;
                }
                for t in 0..stream.len() - d {
                    let (x, y) = (stream[t], stream[t + d]);
                    for (bi, &(_, bit)) in BUTTONS.iter().enumerate() {
                        let s = &mut stats[di][bi];
                        s.samples += 1;
                        s.flips += ((x ^ y) & bit != 0) as u64;
                        s.held += (y & bit != 0) as u64;
                    }
                }
            }
        }
    }
    stats
}

fn oracle_mask(stats: &[BitStat; 10]) -> u16 {
    BUTTONS
        .iter()
        .enumerate()
        .filter(|(bi, _)| stats[*bi].flips < stats[*bi].held)
        .map(|(_, &(_, bit))| bit)
        .sum()
}

fn mask_label(mask: u16) -> String {
    if mask == 0 {
        return "∅".to_string();
    }
    BUTTONS
        .iter()
        .filter(|&&(_, bit)| mask & bit != 0)
        .map(|&(name, _)| name)
        .collect::<Vec<_>>()
        .join("|")
}

fn main() {
    selftest();

    let mut paths: Vec<std::path::PathBuf> = std::env::args().skip(1).map(Into::into).collect();
    if paths.is_empty() {
        let home = std::env::var_os("HOME").expect("no replay paths given and HOME unset");
        paths.push(std::path::PathBuf::from(home).join("Documents/Tango/replays"));
    }

    let corpus = load_corpus(&paths);
    let total_ticks: usize = corpus.rounds.iter().map(|r| r.p1.len()).sum();
    let fps = tango_pvp::battle::EXPECTED_FPS as f64;
    println!(
        "corpus: {} files → {} matches ({} undecodable), {} rounds, {:.1}M ticks (~{:.1} h of play)",
        corpus.files,
        corpus.matches,
        corpus.undecodable,
        corpus.rounds.len(),
        total_ticks as f64 / 1e6,
        total_ticks as f64 / fps / 3600.0,
    );
    if corpus.rounds.is_empty() {
        return;
    }

    // Pass 1: per-button flip-vs-held rates per horizon, and the oracle masks.
    let stats = bit_stats(&corpus.rounds);
    println!("\nper-button behavior (flip% over horizon d vs held% at target tick; repeat wins when flip < held):");
    print!("{:<8}{:>8}", "button", "held%");
    for &d in DEPTHS {
        print!("{:>9}", format!("flip@{}", d));
    }
    println!();
    for (bi, &(name, _)) in BUTTONS.iter().enumerate() {
        let held = stats[0][bi].held as f64 / stats[0][bi].samples.max(1) as f64;
        print!("{:<8}{:>7.2}%", name, held * 100.0);
        for stat in stats.iter().map(|s| &s[bi]) {
            print!("{:>8.2}%", stat.flips as f64 / stat.samples.max(1) as f64 * 100.0);
        }
        println!();
    }
    let oracles: Vec<u16> = stats.iter().map(|s| oracle_mask(s)).collect();
    for (di, &d) in DEPTHS.iter().enumerate() {
        println!("oracle@{}: {}", d, mask_label(oracles[di]));
    }

    // Pass 2: sweep every (depth, candidate) over every round, both directions,
    // through the real session. Work-steal rounds across threads.
    let n_cands = BASE_CANDIDATES.len() + 1;
    let results = std::sync::Mutex::new(vec![vec![RunAgg::default(); n_cands]; DEPTHS.len()]);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let n_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    std::thread::scope(|scope| {
        for _ in 0..n_threads {
            scope.spawn(|| {
                let mut mine = vec![vec![RunAgg::default(); n_cands]; DEPTHS.len()];
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(round) = corpus.rounds.get(i) else { break };
                    for (local, remote) in [(&round.p1, &round.p2), (&round.p2, &round.p1)] {
                        for (di, &d) in DEPTHS.iter().enumerate() {
                            for ci in 0..n_cands {
                                let mask = if ci < BASE_CANDIDATES.len() {
                                    BASE_CANDIDATES[ci].1
                                } else {
                                    oracles[di]
                                };
                                mine[di][ci].merge(&run_round(local, remote, d, mask));
                            }
                        }
                    }
                }
                let mut results = results.lock().unwrap();
                for (di, row) in mine.iter().enumerate() {
                    for (ci, agg) in row.iter().enumerate() {
                        results[di][ci].merge(agg);
                    }
                }
            });
        }
    });
    let results = results.into_inner().unwrap();

    for (di, &d) in DEPTHS.iter().enumerate() {
        println!("\ndepth {} (presented frame carries {} predicted remote ticks):", d, d);
        println!(
            "  {:<22}{:>10}{:>7}{:>6}{:>6}{:>11}{:>11}",
            "predictor", "roll/min", "p50", "p95", "max", "steps/fr", "tip-wrong%"
        );
        for ci in 0..n_cands {
            let agg = &results[di][ci];
            let name = if ci < BASE_CANDIDATES.len() {
                BASE_CANDIDATES[ci].0.to_string()
            } else {
                format!("oracle@{} ({})", d, mask_label(oracles[di]))
            };
            let minutes = agg.frames as f64 / fps / 60.0;
            println!(
                "  {:<22}{:>10.1}{:>7}{:>6}{:>6}{:>11.3}{:>10.2}%",
                name,
                agg.rollbacks as f64 / minutes,
                agg.depth_quantile(0.5),
                agg.depth_quantile(0.95),
                agg.depth_max(),
                agg.steps as f64 / agg.frames.max(1) as f64,
                agg.wrong_tip as f64 / agg.frames.max(1) as f64 * 100.0,
            );
        }
    }
}
