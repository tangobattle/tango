//! Headless drill for rollback fidelity: a pair that speculated and
//! rolled back must be the pair that never speculated at all.
//!
//! This is the one stage of a live match no other probe here covers.
//! `landing_probe` restores exactly once — a landing — and `replay_probe`
//! drives playback; neither models what PvP actually does, which is
//! snapshot, run ahead on a guess, and restore, hundreds of times a
//! minute. And it is the only stage that differs *between the two
//! peers*: both simulate the same pair from the same confirmed inputs,
//! but each mispredicts on its own network timing, so their restore
//! histories never match. Anything a restore fails to carry is therefore
//! not a local glitch — it is a desync, and an intermittent one, because
//! which ticks get rolled back is a property of the wire.
//!
//! Pair A is the reference: it consumes the recording's rows straight
//! through and never snapshots. Pair B consumes the identical rows, but
//! every `--every` ticks it snapshots, runs `--depth` ticks ahead with
//! the remote seat's input *predicted* (repeat-last, the engine's own
//! predictor), then restores and re-simulates those ticks for real. The
//! two are compared every tick. B is what a peer looks like; A is what
//! that peer is supposed to be indistinguishable from.
//!
//! Comparing reference-against-churn is stronger than comparing two
//! churn schedules against each other: if B matches A under any
//! schedule, then any two peers match too, whatever they each rolled
//! back.
//!
//! `--rerun` asks the same question without pair A, and asks it of
//! every tick independently: capture, run the tick, restore, run the
//! same tick again with the same inputs, compare. A tick that answers
//! differently the second time is a rollback bug on its own terms —
//! there is no prediction to blame and no earlier divergence to have
//! poisoned it — and because nothing accumulates, one bad tick doesn't
//! hide the hundred after it. That is the mode that found the ARM9's
//! divider and square-root registers going unsaved (melonds-rs
//! d4314a5): three ticks in four hundred, each one the game reading a
//! quotient the taken-back run had overwritten.
//!
//! **The instance cookie is masked.** Two pairs in one process get
//! distinct `NDS::InstanceCookie` values (a process-wide atomic, bumped
//! per console constructed), which melonDS writes into every savestate
//! as the last field of its `NDSG` section and never loads back — it
//! answers "did this state come from me", which is what makes a foreign
//! landing rebuild its timing tables. Two pairs therefore always differ
//! in those 8 bytes per console and it means nothing. Masking it is why
//! this probe can fail on the *first* differing byte instead of doing
//! what `landing_probe` has to do and waiting for pixels.
//!
//! Usage: rollback_probe <rom.nds> <replay.tangoreplay> [ticks]
//!            [--every N] [--depth K] [--first N] [--peer 0|1]
//!            [--compare-every N] [--rerun [--repeat K]]
//!
//! `--peer` is which seat is the local player, whose input is known
//! rather than guessed. `--every 0` never speculates and `--depth 0`
//! restores without running ahead, which are the two controls a
//! divergence has to be read against; `--first` pins the tick that
//! speculates first, so a single rollback can be isolated and
//! bisected. `--compare-every` trades the tick a divergence gets
//! reported at for the serialisation cost of finding it, which is what
//! makes a whole-recording hostile run affordable. Set `FRAME_DUMP_DIR`
//! to have a frame divergence leave both sides' screens on disk.

use std::ops::Range;
use std::sync::Arc;

use tango_backend_melonds::GameSupport as _;
use tango_match::Link as _;

fn family_of(replay: &tango_replay::Replay) -> &str {
    replay
        .metadata
        .side(replay.local_player_index)
        .and_then(|s| s.game_info.as_ref())
        .map(|gi| gi.rom_family.as_str())
        .expect("replay carries no game info")
}

/// The whole-link bytes: both consoles, the air's in-flight frames, the
/// clock bounds.
fn save_link(link: &mut tango_backend_melonds::Link) -> Vec<u8> {
    let snap = link.snapshot(None).expect("link capture");
    tango_backend_melonds::link::snapshot_bytes(&snap).expect("a DS snapshot")
}

/// Where each console's `NDSG` section ends, as a range covering its
/// last 8 bytes — the instance cookie (see the module docs).
///
/// Walks the link blob's own layout (the same one `landing_probe`'s
/// `locate_link` reads): per seat, a length-prefixed console state, then
/// the two frame queues, then the seat's clock bounds. Byte offsets in
/// this blob are *not* stable tick to tick — the queues hold
/// variable-length frames — so the mask is recomputed per comparison
/// rather than cached.
fn cookie_ranges(bytes: &[u8]) -> Vec<Range<usize>> {
    let u64_at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
    let mut out = Vec::new();
    let mut at = 0usize;
    for _seat in 0..2 {
        let len = u64_at(at);
        at += 8;
        if let Some(end) = ndsg_end(&bytes[at..at + len]) {
            out.push(at + end - 8..at + end);
        }
        at += len;
        for _queue in 0..2 {
            let count = u64_at(at);
            at += 8;
            for _ in 0..count {
                at += 19 + u64_at(at + 11);
            }
        }
        // progress(8) + attached(1) + attached_at(8)
        at += 17;
    }
    out
}

/// Where the `NDSG` section's data ends within one console's savestate.
/// melonDS savestates are a 16-byte header then sections of
/// `magic(4) + length(4, header-inclusive) + reserved(8) + data`.
fn ndsg_end(state: &[u8]) -> Option<usize> {
    let mut at = 0x10usize;
    while at + 16 <= state.len() {
        let len = u32::from_le_bytes(state[at + 4..at + 8].try_into().unwrap()) as usize;
        if len < 16 || at + len > state.len() {
            return None;
        }
        if &state[at..at + 4] == b"NDSG" {
            return Some(at + len);
        }
        at += len;
    }
    None
}

/// Which part of a whole-link snapshot (melonds_rollback
/// `Snapshot::to_bytes` layout) a byte offset falls in: per seat, the
/// console's savestate (attributed to its melonDS section via
/// [`locate`]), the two air queues, or the clock bounds.
fn locate_link(bytes: &[u8], offset: usize) -> String {
    let u64_at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
    let mut at = 0usize;
    for seat in 0..2 {
        let len = u64_at(at);
        at += 8;
        if offset < at + len {
            return format!("console{seat} {}", locate(&bytes[at..at + len], offset - at));
        }
        at += len;
        for queue in ["incoming", "replies"] {
            let count = u64_at(at);
            at += 8;
            for i in 0..count {
                let end = at + 19 + u64_at(at + 11);
                if offset < end {
                    return format!("air{seat}.{queue}[{i}]+{:#x}", offset - at);
                }
                at = end;
            }
        }
        for (field, width) in [("progress", 8), ("attached", 1), ("attached_at", 8)] {
            if offset < at + width {
                return format!("air{seat}.{field}");
            }
            at += width;
        }
    }
    format!("?+{offset:#x}")
}

/// Where main RAM sits inside the `NDSG` section's data, and how much
/// of it a DS-mode console serializes.
const MAIN_RAM: usize = 4;
const MAIN_RAM_LEN: usize = 0x40_0000;

/// Which savestate section `offset` falls in, as `MAGIC+relative`.
fn locate(state: &[u8], offset: usize) -> String {
    let mut at = 0x10usize;
    while at + 16 <= state.len() {
        let len = u32::from_le_bytes(state[at + 4..at + 8].try_into().unwrap()) as usize;
        if len < 16 || at + len > state.len() {
            break;
        }
        if offset < at + len {
            let magic = String::from_utf8_lossy(&state[at..at + 4]).into_owned();
            let rel = offset - (at + 16);
            // `NDSG` opens with a config word and then the whole of main
            // RAM, so most of it names an address the game itself uses —
            // which is what a divergence in there has to be chased with.
            if magic == "NDSG" && (MAIN_RAM..MAIN_RAM + MAIN_RAM_LEN).contains(&rel) {
                return format!("MainRAM@{:#010x}", 0x0200_0000 + (rel - MAIN_RAM));
            }
            return format!("{magic}+{rel:#x}");
        }
        at += len;
    }
    format!("?+{offset:#x}")
}

/// Differing byte runs outside the masked ranges, attributed through
/// [`locate_link`], with both sides' values.
fn diff_runs(a: &[u8], b: &[u8], mask: &[Range<usize>], max: usize) -> (usize, Vec<String>) {
    let masked = |i: usize| mask.iter().any(|r| r.contains(&i));
    let differs = |i: usize| a[i] != b[i] && !masked(i);
    let n = a.len().min(b.len());
    let total = (0..n).filter(|&i| differs(i)).count();
    let mut runs = Vec::new();
    let mut i = 0usize;
    while i < n && runs.len() < max {
        if !differs(i) {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && differs(i) && i - start < 16 {
            i += 1;
        }
        let hex = |s: &[u8]| s.iter().map(|v| format!("{v:02x}")).collect::<String>();
        runs.push(format!(
            "{}({}B) a={} b={}",
            locate_link(a, start),
            i - start,
            hex(&a[start..i]),
            hex(&b[start..i])
        ));
    }
    (total, runs)
}

fn flag<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|v| v.parse().unwrap_or_else(|_| panic!("bad value for {name}")))
        .unwrap_or(default)
}

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let f = std::fs::File::open(&args[1]).expect("replay unreadable");
    let replay = tango_replay::Replay::decode(std::io::BufReader::new(f)).expect("replay undecodable");
    let limit: u32 = args.get(2).filter(|a| !a.starts_with("--")).map(|s| s.parse().unwrap()).unwrap_or(u32::MAX);
    let every: u32 = flag(&args, "--every", 7);
    let depth: u32 = flag(&args, "--depth", 4);
    let peer: usize = flag(&args, "--peer", 0);
    // Which tick speculates first; `--every` counts from there. Pinning
    // it isolates a single rollback, which is how a divergence gets
    // attributed to one.
    let first: u32 = flag(&args, "--first", 0);
    // `--every 0` never speculates: the harness's own baseline, which
    // must be byte-identical for any divergence below to mean anything.
    // `--depth 0` snapshots and restores without running ahead — the
    // sharpest question there is, since it asks what a restore alone
    // fails to carry.
    assert!(peer < 2, "--peer is a seat: 0 or 1");
    // `--rerun` drops pair A and asks the narrower question directly:
    // is one tick's execution a pure function of the state a snapshot
    // captured? Each tick runs `--repeat` times from its own capture
    // and the runs are compared against each other, so every tick is
    // judged independently instead of the first divergence poisoning
    // everything after it.
    let rerun = args.iter().any(|x| x == "--rerun");
    let repeat: u32 = flag(&args, "--repeat", 2);
    // Serialising two whole links costs more than ticking them, so a
    // long hostile run compares at intervals instead. A divergence
    // never heals, so the only thing a wider interval loses is the
    // precision of the tick it gets reported at.
    let compare_every: u32 = flag(&args, "--compare-every", 1);
    assert!(compare_every > 0, "--compare-every must be positive");

    let inputs: Arc<Vec<[tango_match::HostInput; 2]>> = Arc::new(
        replay
            .inputs
            .iter()
            .map(|&row| {
                row.map(|input| tango_match::HostInput {
                    keys: input.keys as u32,
                    touch: input.touch.map(|(x, y)| (x as u16, y as u16)),
                })
            })
            .collect(),
    );
    let total = (inputs.len() as u32).min(limit);
    let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);
    let support = match family_of(&replay) {
        "bn5ds" => &tango_gamesupport_bn5ds::pvp::US,
        _ => &tango_gamesupport_bn5ds::pvp::JP,
    };
    let saves = [Some(replay.srams[0].as_slice()), Some(replay.srams[1].as_slice())];
    println!(
        "replay: {} ticks, comparing {total} (mispredict every {every} ticks, {depth} deep, local seat {peer})",
        inputs.len()
    );

    // Both pairs walk the real prime. Neither lands on the other: this
    // probe is about what a *restore* fails to carry, so the two must
    // start from states they each reached themselves — which the walk's
    // own determinism guarantees (`landing_probe twin`).
    let started = std::time::Instant::now();
    let mut a = tango_backend_melonds::Link::new(&rom, saves, replay.rtc_time()).expect("pair A boot");
    support.prime(&mut a, match_type, replay.rng_seed, None).expect("pair A prime");
    let mut b = tango_backend_melonds::Link::new(&rom, saves, replay.rtc_time()).expect("pair B boot");
    support.prime(&mut b, match_type, replay.rng_seed, None).expect("pair B prime");
    println!("both pairs primed in {:.1?}", started.elapsed());

    // The baseline. Anything differing before a single rollback has
    // happened is the walk, not the churn, and this probe has nothing to
    // say about it — bail rather than report someone else's bug as ours.
    let (state_a, state_b) = (save_link(&mut a), save_link(&mut b));
    let (base, runs) = diff_runs(&state_a, &state_b, &cookie_ranges(&state_a), 8);
    if base != 0 {
        println!("BASELINE DIFFERS in {base} unmasked bytes before any rollback: {runs:?}");
        println!("(that is a walk-determinism failure, not a rollback one — run landing_probe twin)");
        std::process::exit(2);
    }
    println!("baseline: byte-identical after priming (cookie masked)");

    // The map, rather than the verdict: every tick answered on its own,
    // so nothing accumulates and one bad tick doesn't hide the rest.
    if rerun {
        let mut bad = 0u32;
        for tick in 0..total {
            let row = inputs[tick as usize];
            let snap = b.snapshot(None).expect("capture");
            b.tick(row);
            let once = save_link(&mut b);
            let mut differed = 0;
            let mut last = Vec::new();
            for _ in 1..repeat {
                b.restore(&snap).expect("rollback");
                b.tick(row);
                last = save_link(&mut b);
                if last != once {
                    differed += 1;
                }
            }
            if differed != 0 {
                bad += 1;
                let (count, runs) = diff_runs(&once, &last, &[], 6);
                println!(
                    "  tick {tick}: {differed}/{} re-executions differ, last in {count} bytes: {runs:?}",
                    repeat - 1
                );
            }
            if (tick + 1) % 2000 == 0 {
                println!("  tick {}: {bad} non-reproducing so far ({:.1?})", tick + 1, started.elapsed());
            }
        }
        println!("rerun: {bad} of {total} ticks did not reproduce ({:.1?})", started.elapsed());
        std::process::exit(if bad == 0 { 0 } else { 1 });
    }

    let mut rollbacks = 0u32;
    let mut frames_diverged = false;
    for tick in 0..total {
        let row = inputs[tick as usize];

        // B speculates: it does not know the remote seat's input yet, so
        // it repeats the last one it confirmed — the engine's own
        // predictor — runs ahead, and takes it back. A never does this.
        // Everything B commits below is the confirmed row, exactly what
        // A commits, so the two are only ever separated by the fact that
        // B's cores have executed speculative ticks and A's have not.
        if every > 0 && tick >= first && (tick - first) % every == 0 && tick + depth <= total {
            let snap = b.snapshot(None).expect("speculation capture");
            let mut guess = if tick == 0 {
                [tango_match::HostInput::default(); 2]
            } else {
                inputs[tick as usize - 1]
            };
            for ahead in 0..depth {
                guess[peer] = inputs[(tick + ahead) as usize][peer];
                b.tick(guess);
            }
            b.restore(&snap).expect("rollback");
            rollbacks += 1;
        }

        a.tick(row);
        b.tick(row);

        if (tick + 1) % compare_every == 0 || tick + 1 == total {
            let (state_a, state_b) = (save_link(&mut a), save_link(&mut b));
            let (count, runs) = diff_runs(&state_a, &state_b, &cookie_ranges(&state_a), 12);
            if count != 0 {
                println!(
                    "DIVERGED by tick {} after {rollbacks} rollbacks: {count} bytes, first runs {runs:?}",
                    tick + 1
                );
                println!("({:.1?})", started.elapsed());
                std::process::exit(1);
            }
        }

        if !frames_diverged {
            for seat in 0..2 {
                let (fa, fb) = (a.side(seat).frame(), b.side(seat).frame());
                if fa != fb {
                    frames_diverged = true;
                    println!("FRAMES diverged at tick {}, console {seat}", tick + 1);
                    if let Ok(dir) = std::env::var("FRAME_DUMP_DIR") {
                        for (tag, fb) in [("a", &fa), ("b", &fb)] {
                            if let Some(fb) = fb {
                                image::RgbaImage::from_raw(512, 192, fb.clone())
                                    .expect("framebuffer shape")
                                    .save(format!("{dir}/rollback-t{}-s{seat}-{tag}.png", tick + 1))
                                    .expect("frame dump");
                            }
                        }
                    }
                }
            }
        }

        if (tick + 1) % 2000 == 0 {
            println!("  tick {}: identical, {rollbacks} rollbacks ({:.1?})", tick + 1, started.elapsed());
        }
    }

    if frames_diverged {
        std::process::exit(1);
    }
    println!(
        "SUCCESS: byte-identical through {total} ticks across {rollbacks} rollbacks ({:.1?})",
        started.elapsed()
    );
}
