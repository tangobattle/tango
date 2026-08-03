//! Headless drill for cross-pair capture landings: a pair restored from
//! another pair's capture must play on as that pair.
//!
//! This is what [`ReplaySet::stats_reusing_playback`] does on every
//! replay a viewer opens — the statistics pair skips its own priming
//! walk by landing on the display pair's primed capture — so a landing
//! that isn't exact poisons the keyframes both pairs share, and the
//! viewer's next backward seek restores one.
//!
//! Pair A boots and walks the real priming route; pair B comes up per
//! the chosen mode and both pairs then consume the same recording's
//! input rows in lockstep, every tick comparing whole-link snapshots
//! (both consoles, the air's in-flight frames, the clock bounds) AND
//! rendered frames. **Frames are the verdict**: the wireless walk has
//! never been byte-deterministic in its scratch (`twin` mode measures
//! it — two identical walks differ in the wifi core's stale reply
//! staging and the game's own transient work areas, run to run, while
//! their frames stay identical), so the byte diff is reported as
//! diagnostic context and the probe fails only when pixels part ways.
//!
//! Set `FRAME_DUMP_DIR` to have a frame divergence leave both sides'
//! screens on disk.
//!
//! Modes:
//!
//! * `fresh` — B lands straight out of construction, having never run a
//!   frame. This is what `boot_unprimed` does, and it is the mode that
//!   caught THE seek desync: it used to part ways on screen about a
//!   minute into a recording, because melonDS kept state a savestate
//!   did not carry (the CPUs' fetch-timing scratch, and derived timing
//!   tables a load rebuilt only when the registers it could see had
//!   changed) and the JIT baked it into compiled blocks. Fixed engine
//!   side; this mode is what holds that fix to account.
//! * `warm:K` — B runs K bare frames before landing. The engine ran one
//!   such frame for a while, on the theory that a console which has
//!   never run one is not equivalent to a console that has; it only
//!   moved the symptom around, and K makes no difference now.
//! * `walked` — B walks its own prime first, then lands on A's capture:
//!   the shape every shared-store seek rests on (playback and stats
//!   pairs exchanging keyframes).
//! * `twin` — B walks its own prime and never lands: the harness's own
//!   walk-vs-walk baseline. Two independent walks differ by ~1 kB of
//!   wifi scratch while their frames stay identical, which is why the
//!   verdict below is pixels rather than bytes.
//!
//! Usage: landing_probe <rom.nds> <replay.tangoreplay> [fresh|walked|twin|warm:K] [ticks]

use std::sync::Arc;

use tango_backend_melonds::GameSupport as _;
use tango_match::Link as _;

fn family_of(replay: &tango_replay::Replay) -> &'static str {
    replay
        .metadata
        .side(replay.local_player_index)
        .and_then(|s| s.game_info.as_ref())
        .map(|gi| gi.rom_family.as_str())
        .map(|s| match s {
            "bn5ds" => "bn5ds",
            "exe5ds" => "exe5ds",
            other => panic!("not this crate's replay: {other}"),
        })
        .expect("replay carries no game info")
}

fn save_link(link: &mut tango_backend_melonds::Link) -> Vec<u8> {
    let snap = link.snapshot(None).expect("link snapshot");
    tango_backend_melonds::link::snapshot_bytes(&snap).expect("a DS snapshot")
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
                let data_len = u64_at(at + 11);
                let end = at + 19 + data_len;
                if offset < end {
                    return format!("air{seat}.{queue}[{i}]+{:#x}", offset - at);
                }
                at = end;
            }
        }
        if offset < at + 8 {
            return format!("air{seat}.progress");
        }
        at += 8;
        if offset < at + 1 {
            return format!("air{seat}.attached");
        }
        at += 1;
        if offset < at + 8 {
            return format!("air{seat}.attached_at");
        }
        at += 8;
    }
    format!("?+{offset:#x}")
}

/// Which savestate section `offset` falls in, as `MAGIC+relative`.
/// melonDS savestates are a 16-byte header then sections of
/// `magic(4) + length(4, header-inclusive) + reserved(8) + data`.
fn locate(state: &[u8], offset: usize) -> String {
    let mut at = 0x10usize;
    while at + 16 <= state.len() {
        let len = u32::from_le_bytes(state[at + 4..at + 8].try_into().unwrap()) as usize;
        if len < 16 || at + len > state.len() {
            break;
        }
        if offset < at + len {
            let magic = String::from_utf8_lossy(&state[at..at + 4]).into_owned();
            return format!("{magic}+{:#x}", offset - (at + 16));
        }
        at += len;
    }
    format!("?+{offset:#x}")
}

/// Differing byte runs between two whole-link snapshots, attributed
/// through [`locate_link`], with both sides' values — the whole
/// picture, since a divergence onset is typically only dozens of bytes.
fn diff_offsets_link(a: &[u8], b: &[u8], max: usize) -> Vec<String> {
    if a.len() != b.len() {
        println!("  (snapshot lengths differ: {} vs {})", a.len(), b.len());
    }
    let total = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
    println!("  ({total} bytes differ in all)");
    let mut runs: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < a.len().min(b.len()) && runs.len() < max {
        if a[i] == b[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < a.len().min(b.len()) && a[i] != b[i] && i - start < 16 {
            i += 1;
        }
        let hex = |s: &[u8]| s.iter().map(|v| format!("{v:02x}")).collect::<String>();
        runs.push(format!(
            "{}({}B) a={} b={}",
            locate_link(a, start),
            i - start,
            hex(&a[start..i]),
            hex(&b[start..i]),
        ));
    }
    runs
}

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let f = std::fs::File::open(&args[1]).expect("replay unreadable");
    let replay = tango_replay::Replay::decode(std::io::BufReader::new(f)).expect("replay undecodable");
    let mode = args.get(2).cloned().unwrap_or_else(|| "fresh".to_string());
    let limit: u32 = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(u32::MAX);

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
    println!("replay: {} ticks, comparing {total}", inputs.len());

    // Pair A: the display pair's boot — walked through the real prime.
    let started = std::time::Instant::now();
    let mut a = tango_backend_melonds::Link::new(&rom, saves, replay.rtc_time()).expect("pair A boot");
    support.prime(&mut a, match_type, replay.rng_seed, &tango_match::telemetry::EventSink::new(), None).expect("pair A prime");
    println!("pair A primed in {:.1?}", started.elapsed());

    // Pair B: landed on A's capture — fresh (the old boot_unprimed
    // shape), after a walk of its own (the shared-store seek shape), or
    // after K bare warm-up ticks (`warm:K`, for bisecting what execution
    // history the landing fails to carry).
    let snap = a.snapshot(None).expect("pair A capture");
    let mut b = tango_backend_melonds::Link::new(&rom, saves, replay.rtc_time()).expect("pair B boot");
    match mode.as_str() {
        "fresh" => b.restore(&snap).expect("pair B landing"),
        "walked" => {
            support.prime(&mut b, match_type, replay.rng_seed, &tango_match::telemetry::EventSink::new(), None).expect("pair B prime");
            b.restore(&snap).expect("pair B landing");
        }
        // No landing at all: B walks its own prime and the lockstep
        // compares two independent walks — the harness's own
        // determinism, which everything above assumes.
        "twin" => support
            .prime(&mut b, match_type, replay.rng_seed, &tango_match::telemetry::EventSink::new(), None)
            .expect("pair B prime"),
        warm if warm.starts_with("warm:") => {
            let ticks: u32 = warm[5..].parse().expect("warm tick count");
            for _ in 0..ticks {
                b.tick([tango_match::HostInput::default(); 2]);
            }
            b.restore(&snap).expect("pair B landing");
        }
        other => panic!("unknown mode {other:?}: expected fresh, walked, twin or warm:K"),
    }

    // Immediately after the landing, the two pairs must serialize
    // identically — if they already differ here, the landing itself is
    // what loses state (rollback-probe rule: prove restore fidelity
    // first). Whole-link bytes: consoles, the air's in-flight frames,
    // the clock bounds.
    let mut state_a = save_link(&mut a);
    let mut state_b = save_link(&mut b);
    if state_a != state_b {
        let offs = diff_offsets_link(&state_a, &state_b, 8);
        println!("DIVERGED at landing: first offsets {offs:?}");
    }

    // Lockstep: same rows into both pairs, whole-link bytes compared
    // per tick. The first divergence is reported in full and the walk
    // keeps going — the trajectory (does it grow? does it reach the
    // pixels?) is what separates a stale scratch byte from a real
    // desync.
    let mut diverged = false;
    let mut frames_diverged = false;
    for tick in 0..total {
        let row = inputs[tick as usize];
        a.tick(row);
        b.tick(row);
        state_a = save_link(&mut a);
        state_b = save_link(&mut b);
        if state_a != state_b && !diverged {
            diverged = true;
            let offs = diff_offsets_link(&state_a, &state_b, 12);
            println!("DIVERGED at tick {}: first offsets {offs:?}", tick + 1);
        }
        if !frames_diverged {
            for seat in 0..2 {
                let fa = a.side(seat).frame();
                let fb = b.side(seat).frame();
                if fa != fb {
                    frames_diverged = true;
                    println!("FRAMES diverged at tick {}, console {seat}", tick + 1);
                    // Leave the divergent pair on disk where a human can
                    // see WHAT differs — a wrong layer names its buffer.
                    if let Ok(dir) = std::env::var("FRAME_DUMP_DIR") {
                        for (tag, fb) in [("a", &fa), ("b", &fb)] {
                            if let Some(fb) = fb {
                                image::RgbaImage::from_raw(512, 192, fb.clone())
                                    .expect("framebuffer shape")
                                    .save(format!("{dir}/diverge-t{}-s{seat}-{tag}.png", tick + 1))
                                    .expect("frame dump");
                            }
                        }
                    }
                }
            }
        }
        if (tick + 1) % 600 == 0 {
            let count = state_a.iter().zip(state_b.iter()).filter(|(x, y)| x != y).count();
            println!(
                "  tick {}: link diff {count} bytes, frames {} ({:.1?})",
                tick + 1,
                if frames_diverged { "DIVERGED" } else { "identical" },
                started.elapsed()
            );
        }
    }
    // Scratch variance in the savestate is the walk's normal (see the
    // module docs and `twin` mode); pixels parting ways is the failure.
    if frames_diverged {
        std::process::exit(1);
    }
    println!(
        "SUCCESS: frames identical through {total} ticks{} ({:.1?})",
        if diverged {
            " (savestate scratch differed — see above)"
        } else {
            ", savestates byte-identical"
        },
        started.elapsed()
    );
}
