//! Single-pair rollback-determinism probe: primes a pair, then walks a
//! deterministic input schedule while continuously exercising the
//! session's save/load cadence — every few ticks, load a snapshot from
//! a few ticks back, re-run the same inputs, and require every re-run
//! tick's digest to match the first pass's. A mismatch means
//! `Pair::load` is lossy (re-simulation diverges from the original),
//! which is exactly what shows up as a cross-peer desync at settled
//! ticks in netplay.
//!
//! Usage: pvp_rollback_probe <rom> <save> [<rom2> <save2>] [ticks]

use std::collections::VecDeque;

use tango_gamesupport_bn2::pvp;
use tango_backend_mgba::GameSupport as _;

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"AE2E" => &pvp::PVP_AE2E_00,
        b"AE2J" => &pvp::SIO_AE2J_00_AC,
        code => panic!("not a bn2 rom (code {:02x?})", code),
    }
}

/// Deterministic wiggle, distinct per core, idle through the intro.
fn keys_at(t: u32) -> [u32; 2] {
    let mut keys = [0u32; 2];
    if t > 240 {
        for (i, k) in keys.iter_mut().enumerate() {
            *k = ((t / 5).wrapping_mul(2654435761) >> i) & 0x3f3;
        }
    }
    keys
}

fn main() {
    env_logger::init();
    mgba::log::install_default_logger();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (rom0, save0) = (
        std::fs::read(&args[0]).expect("rom0"),
        std::fs::read(&args[1]).expect("save0"),
    );
    let (rom1, save1) = if args.len() >= 4 && !args[2].chars().all(|c| c.is_ascii_digit()) {
        (
            std::fs::read(&args[2]).expect("rom1"),
            std::fs::read(&args[3]).expect("save1"),
        )
    } else {
        (rom0.clone(), save0.clone())
    };
    let max_ticks: u32 = args.last().and_then(|s| s.parse().ok()).unwrap_or(1200);

    let (pvp0, pvp1) = (pvp_for(&rom0), pvp_for(&rom1));
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
        rtc: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000)),
        peripheral: mgba_rollback::Peripheral::Cable,
    })
    .unwrap();

    let config = tango_backend_mgba::PrimeConfig {
        match_type: (0, 0),
        rng_seed: *b"sio-probe-seed!!",
        disable_bgm: false,
    };
    let lifecycle = tango_match::telemetry::LifecycleSink::new();
    let primed = [tango_backend_mgba::PrimedLatch::new(), tango_backend_mgba::PrimedLatch::new()];
    pair.set_traps(0, pvp0.primer_traps(&config, 0, &lifecycle, &primed[0]));
    pair.set_traps(1, pvp1.primer_traps(&config, 1, &lifecycle, &primed[1]));
    let mut prime_ticks = 0u32;
    while !(primed[0].is_set() && primed[1].is_set()) {
        pair.tick(&[0, 0]);
        prime_ticks += 1;
        assert!(prime_ticks < 3600, "priming wedged");
    }
    println!("primed in {prime_ticks} ticks");

    // Sliding window of (tick, snapshot); digests of the first pass per
    // tick. Depth covers the deepest rollback below.
    const WINDOW: usize = 12;
    let mut window: VecDeque<(u32, mgba_rollback::Snapshot)> = VecDeque::new();
    let mut digests: Vec<u32> = vec![0]; // digests[t] = after tick t; t=0 = start
    let start = pair.save().unwrap();
    digests[0] = start.digest();
    window.push_back((0, start));

    let mut t = 0u32;
    let mut checked = 0u64;
    while t < max_ticks {
        t += 1;
        pair.tick(&keys_at(t));
        let snap = pair.save().unwrap();
        digests.push(snap.digest());

        // Idempotence: save∘load must reproduce the same serialized state.
        pair.load(&snap).unwrap();
        let resnap = pair.save().unwrap();
        if resnap.digest() != snap.digest() {
            println!("SAVE/LOAD NOT IDEMPOTENT at tick {t}:");
            report_diff(&snap, &resnap);
            std::process::exit(1);
        }

        window.push_back((t, snap));
        while window.len() > WINDOW {
            window.pop_front();
        }

        // Session-like cadence: every 3 ticks, roll back 1..=8 ticks and
        // re-run — every re-run tick must reproduce the recorded digest.
        if t % 3 == 0 && t > 8 {
            let depth = 1 + (t / 3) % 8;
            let back = t - depth;
            let (snap_tick, snap) = window
                .iter()
                .find(|(wt, _)| *wt == back)
                .unwrap_or_else(|| panic!("window miss at {back}"));
            pair.load(snap).unwrap();
            let mut rt = *snap_tick;
            while rt < t {
                rt += 1;
                pair.tick(&keys_at(rt));
                let d = pair.save().unwrap().digest();
                checked += 1;
                if d != digests[rt as usize] {
                    println!(
                        "LOSSY LOAD: first divergent re-run tick {rt} (rolled back {depth} from {t}); {checked} re-run ticks checked before this"
                    );
                    let rerun = pair.save().unwrap();
                    if let Some((_, orig)) = window.iter().find(|(wt, _)| *wt == rt) {
                        report_diff(orig, &rerun);
                        // Also show what was loaded: the rollback origin.
                        if let Some((_, loaded)) = window.iter().find(|(wt, _)| *wt == back) {
                            for i in 0..2 {
                                println!(
                                    "loaded snapshot (tick {back}) driver{i} blob: {:02x?}",
                                    loaded.driver_blob(i)
                                );
                            }
                        }
                    }
                    std::process::exit(1);
                }
            }
        }
    }
    println!("PASS: {max_ticks} ticks, {checked} re-run ticks digest-checked, no divergence");
}

/// Report which digest components differ between two snapshots of the
/// same tick.
fn report_diff(a: &mgba_rollback::Snapshot, b: &mgba_rollback::Snapshot) {
    for i in 0..2 {
        let (sa, sb) = (a.core_state(i), b.core_state(i));
        for r in 0..16 {
            if sa.gpr(r) != sb.gpr(r) {
                println!("  core{i} r{r}: {:08x} -> {:08x}", sa.gpr(r), sb.gpr(r));
            }
        }
        if sa.cpsr() != sb.cpsr() {
            println!("  core{i} cpsr: {:08x} -> {:08x}", sa.cpsr(), sb.cpsr());
        }
        for (name, ra, rb) in [("wram", sa.wram(), sb.wram()), ("iwram", sa.iwram(), sb.iwram())] {
            let diffs: Vec<usize> = ra
                .iter()
                .zip(rb.iter())
                .enumerate()
                .filter(|(_, (x, y))| x != y)
                .map(|(o, _)| o)
                .collect();
            if !diffs.is_empty() {
                let head: Vec<String> = diffs
                    .iter()
                    .take(8)
                    .map(|&o| format!("+{o:05x}: {:02x}->{:02x}", ra[o], rb[o]))
                    .collect();
                println!(
                    "  core{i} {name}: {} bytes differ; first: {}",
                    diffs.len(),
                    head.join(", ")
                );
            }
        }
        if a.driver_blob(i) != b.driver_blob(i) {
            println!("  driver{i} blob differs:");
            println!("    a: {:02x?}", a.driver_blob(i));
            println!("    b: {:02x?}", b.driver_blob(i));
        }
    }
}
