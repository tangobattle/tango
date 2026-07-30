//! Headless round/match lifecycle probe: prime a lockstep pair, force
//! KOs, and drive the games' REAL battle-set flow — battles chained by
//! the game itself under tri-battle, then the set teardown — tracing
//! every telemetry lifecycle event and every primer/diagnostic trap
//! firing along the way.
//!
//! bn3's comm battle is one battle for kinds 0-2 (light/mid/heavyweight)
//! and a best-of-three SET for kind 3 (tri-battle): mid-set, the round
//! teardown chains straight into the next battle (`round_start_ret`
//! re-fires the same tick the previous round ends — no menus between),
//! and only the set-deciding battle's teardown takes the exit path
//! through `match_end_ret`. The probe KOs battle after battle until the
//! store shows MatchEnded, then verifies the trace is Started →
//! Ended{Some(outcome)} per battle with MatchEnded last, and FAILS if
//! any state-poking primer trap (boot fast-path, comm-menu routing)
//! re-fires after the first battle — those must stay inert.
//!
//! Usage: pvp_ko_probe <rom> <save> [<rom2> <save2>] [max_ticks]
//!
//! The KO recipe: once a battle is live, poke the victim's in-battle HP
//! to 1 on BOTH cores in the same tick (each core simulates the whole
//! battle, so identical pokes keep the pair coherent), then mash a
//! deterministic input pattern — bn3's buster does 1 damage, so the KO
//! lands within seconds of sim time. Unlike shipped priming code, a
//! probe MAY inject real pad input; that's how it advances the result
//! screens.
//!
//! Env:
//! - PVP_KO_MATCH_TYPE=<mode,subtype>: tango match-type selection;
//!   (0,1..=3) = light/mid/heavy single battle, (0,0) = random weight,
//!   (1,0) = tri-battle. Default "0,0".
//! - PVP_KO_VICTIMS=<pattern>: which player's HP to poke per battle,
//!   e.g. "1" (player 1 every battle, a 2-0 set), "10" (alternating —
//!   a full 2-1 tri set). Default "1".
//! - PVP_KO_ADVANCE=<keys>: post-KO input policy (advance the result
//!   screens / wander the comm menu). Default "A". Key strings cycle one
//!   character per 10 ticks, pressed for the first 2: A/B/U/D/L/R/
//!   S(tart)/E(select), '.' = nothing.
//! - PVP_KO_DUMP_DIR: dump screens at phase transitions.
//!
//! Exits 0 on the full expected trace with no post-battle poke re-fires;
//! exits 1 on timeout, 2 on a poke re-fire.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tango_gamesupport_bn3::pvp;
use tango_match::telemetry::RoundEvent;
use tango_backend_mgba::GameSupport as _;

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"A3XE" => &pvp::PVP_A3XE_00,
        b"A6BE" => &pvp::PVP_A6BE_00,
        b"A3XJ" => &pvp::PVP_A3XJ_01,
        b"A6BJ" => &pvp::PVP_A6BJ_01,
        code => panic!("not a bn3 rom (code {:02x?})", code),
    }
}

/// Trap-era lifecycle anchors (disasm-verified for these ROMs), installed
/// as print-only diagnostics so the probe shows WHICH of them the real
/// protocol route actually executes. Anchors the primer already traps are
/// skipped (the primer wrapper logs those).
fn diag_anchors(rom: &[u8]) -> &'static [(&'static str, u32)] {
    match &rom[0xac..0xb0] {
        b"A6BE" => &[
            ("folder_exit_commit", 0x08034164),
            ("round_call_jump_table_ret", 0x08006470),
            ("round_ending_entry", 0x08006808),
            ("round_end_entry", 0x080068a0),
        ],
        b"A3XE" => &[
            ("round_call_jump_table_ret", 0x08006470),
            ("round_ending_entry", 0x08006808),
            ("round_end_entry", 0x080068a0),
        ],
        b"A3XJ" | b"A6BJ" => &[
            ("round_call_jump_table_ret", 0x08006404),
            ("round_ending_entry", 0x0800679c),
            ("round_end_entry", 0x08006834),
        ],
        code => panic!("not a bn3 rom (code {:02x?})", code),
    }
}

/// The primer's state-POKING trap sites (boot fast-path + comm-menu
/// routing). If any of these re-fires after the battle has run, the poke
/// corrupts the games' own post-battle flow — the probe fails on it.
fn poke_anchors(rom: &[u8]) -> &'static [u32] {
    match &rom[0xac..0xb0] {
        b"A3XE" => &[0x0802b358, 0x0802210a, 0x08022080, 0x0803e08a],
        b"A6BE" => &[0x0802b370, 0x08022122, 0x08022098, 0x0803e0a2],
        b"A3XJ" => &[0x0802b848, 0x080220ca, 0x08022040, 0x0803e532],
        b"A6BJ" => &[0x0802b860, 0x080220e2, 0x08022058, 0x0803e54a],
        code => panic!("not a bn3 rom (code {:02x?})", code),
    }
}

const NOISY_ANCHORS: &[&str] = &["round_call_jump_table_ret", "round_ending_entry"];

fn keys_for(policy: &str, t: u32) -> u32 {
    let chars: Vec<char> = policy.chars().collect();
    if chars.is_empty() {
        return 0;
    }
    let c = chars[((t / 10) as usize) % chars.len()];
    if t % 10 >= 2 {
        return 0;
    }
    match c {
        'A' => 0x001,
        'B' => 0x002,
        'E' => 0x004,
        'S' => 0x008,
        'R' => 0x010,
        'L' => 0x020,
        'U' => 0x040,
        'D' => 0x080,
        _ => 0,
    }
}

fn dump_screens(pair: &mut mgba_rollback::Link, tag: &str) {
    let Ok(dir) = std::env::var("PVP_KO_DUMP_DIR") else {
        return;
    };
    for i in 0..2 {
        if let Some(buf) = pair.video_buffer(i) {
            let img = image::RgbImage::from_fn(240, 160, |x, y| {
                let off = ((y * 240 + x) * 2) as usize;
                let v = u16::from_le_bytes([buf[off], buf[off + 1]]);
                image::Rgb([
                    ((v & 31) as u8) << 3,
                    (((v >> 5) & 31) as u8) << 3,
                    (((v >> 10) & 31) as u8) << 3,
                ])
            });
            img.save(format!("{dir}/{tag}_c{i}.png")).unwrap();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Priming,
    /// Wiggle until both cores report freshly initialized units (both
    /// players above 1 HP) for 60 straight ticks, then poke the KO.
    AwaitLive {
        round: u32,
        streak: u32,
    },
    /// Wiggle until the poked player's HP hits 0.
    Fighting {
        round: u32,
    },
    /// Mash the advance policy through the result screens until the store
    /// shows either the next round's Started (tri-battle mid-set) or
    /// MatchEnded (set decided / single battle over).
    PostBattle {
        round: u32,
        since: u32,
    },
    /// Keep mashing through the comm menu, watching for poke re-fires.
    Epilogue {
        until: u32,
    },
}

/// How long the post-battle result screens may take before giving up.
const POST_BATTLE_BUDGET: u32 = 6000;

fn main() {
    env_logger::init();
    mgba::log::install_default_logger();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (rom0, save0) = (
        std::fs::read(&args[0]).expect("rom0 unreadable"),
        std::fs::read(&args[1]).expect("save0 unreadable"),
    );
    let (rom1, save1) = if args.len() >= 4 {
        (
            std::fs::read(&args[2]).expect("rom1 unreadable"),
            std::fs::read(&args[3]).expect("save1 unreadable"),
        )
    } else {
        (rom0.clone(), save0.clone())
    };
    let max_ticks: u32 = args.last().and_then(|s| s.parse().ok()).unwrap_or(80000);
    let advance_policy = std::env::var("PVP_KO_ADVANCE").unwrap_or_else(|_| "A".to_string());
    // Per-battle victim pattern: battle N's victim is pattern[(N-1) % len].
    let victims: Vec<u8> = std::env::var("PVP_KO_VICTIMS")
        .unwrap_or_else(|_| "1".to_string())
        .chars()
        .filter_map(|c| c.to_digit(10).map(|d| d as u8))
        .collect();
    let victim_for = |round: u32| victims[((round - 1) as usize) % victims.len().max(1)];

    let (pvp0, pvp1) = (pvp_for(&rom0), pvp_for(&rom1));
    let anchors = [diag_anchors(&rom0), diag_anchors(&rom1)];
    let pokes = [poke_anchors(&rom0), poke_anchors(&rom1)];

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
        // PVP_KO_MATCH_TYPE="mode,subtype": (0, 1..=3) = light/mid/heavy
        // single battle, (0, 0) = random weight, (1, 0) = tri-battle.
        match_type: std::env::var("PVP_KO_MATCH_TYPE")
            .ok()
            .and_then(|s| {
                let (m, sub) = s.split_once(',')?;
                Some((m.trim().parse().ok()?, sub.trim().parse().ok()?))
            })
            .unwrap_or((0, 0)),
        rng_seed: *b"sio-probe-seed!!",
        disable_bgm: false,
    };
    let lifecycle = tango_match::telemetry::LifecycleSink::new();
    let primed = [tango_backend_mgba::PrimedLatch::new(), tango_backend_mgba::PrimedLatch::new()];

    // Wrap every primer trap with a fire log (tick, core, address), and
    // add the print-only diagnostic anchors on both cores.
    let tick_now = Arc::new(AtomicU32::new(0));
    #[allow(clippy::type_complexity)]
    let fires: Arc<Mutex<Vec<(u32, usize, u32, &'static str)>>> = Arc::new(Mutex::new(Vec::new()));
    let instrument = |core_idx: usize, traps: Vec<tango_backend_mgba::Trap>| -> Vec<tango_backend_mgba::Trap> {
        let mut out: Vec<tango_backend_mgba::Trap> = traps
            .into_iter()
            .map(|(addr, f)| {
                let tick_now = tick_now.clone();
                let fires = fires.clone();
                let g: Box<dyn Fn(&mut mgba::core::Core)> = Box::new(move |core: &mut mgba::core::Core| {
                    fires
                        .lock()
                        .unwrap()
                        .push((tick_now.load(Ordering::Relaxed), core_idx, addr, "primer"));
                    f(core)
                });
                (addr, g)
            })
            .collect();
        let installed: Vec<u32> = out.iter().map(|(a, _)| *a).collect();
        for &(name, addr) in anchors[core_idx] {
            if installed.contains(&addr) {
                continue; // primer already traps it; the wrapper logs it
            }
            let tick_now = tick_now.clone();
            let fires = fires.clone();
            out.push((
                addr,
                Box::new(move |_core: &mut mgba::core::Core| {
                    fires
                        .lock()
                        .unwrap()
                        .push((tick_now.load(Ordering::Relaxed), core_idx, addr, name));
                }),
            ));
        }
        out
    };
    pair.set_traps(0, instrument(0, pvp0.primer_traps(&config, 0, &lifecycle, &primed[0])));
    pair.set_traps(1, instrument(1, pvp1.primer_traps(&config, 1, &lifecycle, &primed[1])));

    // The real engine telemetry observer: pollers + the lifecycle sink,
    // recording into the shared store the probe prints from.
    let (mut telemetry, store) =
        tango_match::telemetry::Telemetry::new([pvp0.core_poller(0), pvp1.core_poller(1)], lifecycle.clone());

    println!("player ids: core0={} core1={}", pair.player_id(0), pair.player_id(1));

    let mut phase = Phase::Priming;
    let mut events_printed = 0usize;
    let mut prev_menu = [[0u8; 8]; 2];
    let mut prev_hp: Option<[u16; 2]> = None;
    let mut ko_tick: Option<u32> = None;
    let mut poke_refire = false;
    let mut last_noisy_fire: std::collections::HashMap<(usize, u32), u32> = std::collections::HashMap::new();

    for t in 0..max_ticks {
        tick_now.store(t, Ordering::Relaxed);

        let keys = match phase {
            Phase::Priming => [0, 0],
            Phase::AwaitLive { .. } | Phase::Fighting { .. } => {
                // The deterministic battle wiggle (distinct per core).
                let mut keys = [0u32; 2];
                for (i, k) in keys.iter_mut().enumerate() {
                    *k = ((t / 5).wrapping_mul(2654435761) >> i) & 0x3f3;
                }
                keys
            }
            Phase::PostBattle { .. } | Phase::Epilogue { .. } => [keys_for(&advance_policy, t); 2],
        };
        pair.tick(&keys);
        tango_backend_mgba::analysis::observe_pair(&mut telemetry, &mut pair, t);

        // ---- tracing ----
        for (ft, core, addr, name) in fires.lock().unwrap().drain(..) {
            if NOISY_ANCHORS.contains(&name) {
                // Battle-loop-rate anchor: print only (re)start of a burst.
                let last = last_noisy_fire.insert((core, addr), ft);
                if last.is_some_and(|l| ft <= l + 30) {
                    continue;
                }
                println!("[{ft:5}] core{core} {name}@{addr:08x} began firing");
                continue;
            }
            println!("[{ft:5}] core{core} {name}@{addr:08x} fired");
            if ko_tick.is_some() && pokes[core].contains(&addr) {
                println!("[{ft:5}] FAIL: state-poking primer trap re-fired post-battle on core{core}");
                poke_refire = true;
            }
        }
        {
            let s = store.lock().unwrap();
            let events = s.events();
            for (et, ev) in &events[events_printed..] {
                println!("[{et:5}] EVENT {ev:?}");
            }
            events_printed = events.len();
        }
        let menu = [
            pvp0.debug_menu_state(pair.core_mut(0)),
            pvp1.debug_menu_state(pair.core_mut(1)),
        ];
        for i in 0..2 {
            if menu[i] != prev_menu[i] {
                println!("[{t:5}] core{i} menu {:02x?} -> {:02x?}", prev_menu[i], menu[i]);
            }
        }
        prev_menu = menu;
        let hp = pvp0.debug_battle_hp(pair.core_mut(0));
        if hp != prev_hp {
            println!("[{t:5}] hp {prev_hp:?} -> {hp:?}");
            prev_hp = hp;
        }

        // ---- the phase machine ----
        phase = match phase {
            Phase::Priming => {
                if primed[0].is_set() && primed[1].is_set() {
                    println!("[{t:5}] PRIMED (both cores)");
                    dump_screens(&mut pair, "primed");
                    Phase::AwaitLive { round: 1, streak: 0 }
                } else {
                    Phase::Priming
                }
            }
            Phase::AwaitLive { round, streak } => {
                let live = [
                    pvp0.debug_battle_hp(pair.core_mut(0)),
                    pvp1.debug_battle_hp(pair.core_mut(1)),
                ]
                .iter()
                .all(|hp| hp.is_some_and(|hp| hp[0] > 1 && hp[1] > 1));
                let streak = if live { streak + 1 } else { 0 };
                if streak >= 60 {
                    let victim = victim_for(round);
                    println!("[{t:5}] battle {round} live; poking player {victim} HP to 1 on both cores");
                    pvp0.debug_set_hp(pair.core_mut(0), victim, 1);
                    pvp1.debug_set_hp(pair.core_mut(1), victim, 1);
                    dump_screens(&mut pair, &format!("r{round}_live"));
                    Phase::Fighting { round }
                } else {
                    Phase::AwaitLive { round, streak }
                }
            }
            Phase::Fighting { round } => {
                let victim = victim_for(round);
                if pvp0
                    .debug_battle_hp(pair.core_mut(0))
                    .is_some_and(|hp| hp[victim as usize] == 0)
                {
                    println!("[{t:5}] battle {round} KO (player {victim} at 0 HP)");
                    dump_screens(&mut pair, &format!("r{round}_ko"));
                    ko_tick = Some(t);
                    Phase::PostBattle { round, since: t }
                } else {
                    Phase::Fighting { round }
                }
            }
            Phase::PostBattle { round, since } => {
                // After a KO the game either chains into the set's next
                // battle (tri-battle mid-set: a fresh Started) or takes
                // the exit path (MatchEnded).
                let (started, match_ended) = {
                    let s = store.lock().unwrap();
                    let events = s.events();
                    (
                        events.iter().filter(|(_, e)| matches!(e, RoundEvent::Started)).count() as u32,
                        events.iter().any(|(_, e)| matches!(e, RoundEvent::MatchEnded)),
                    )
                };
                if match_ended {
                    println!("[{t:5}] MatchEnded observed after battle {round}; epilogue");
                    dump_screens(&mut pair, "match_end");
                    Phase::Epilogue { until: t + 1200 }
                } else if started > round {
                    println!("[{t:5}] set continues: battle {} started", round + 1);
                    Phase::AwaitLive {
                        round: round + 1,
                        streak: 0,
                    }
                } else if t - since > POST_BATTLE_BUDGET {
                    println!("[{t:5}] post-battle budget exhausted");
                    dump_screens(&mut pair, "stuck");
                    break;
                } else {
                    Phase::PostBattle { round, since }
                }
            }
            Phase::Epilogue { until } => {
                if t >= until {
                    dump_screens(&mut pair, "epilogue");
                    let s = store.lock().unwrap();
                    println!("---- final event trace ----");
                    for (et, ev) in s.events() {
                        println!("[{et:5}] {ev:?}");
                    }
                    // The full expected trace: one Ended with an announced
                    // outcome per Started, MatchEnded last.
                    let events = s.events();
                    let started = events.iter().filter(|(_, e)| matches!(e, RoundEvent::Started)).count();
                    let ended_with_outcome = events
                        .iter()
                        .filter(|(_, e)| matches!(e, RoundEvent::Ended { outcome: Some(_) }))
                        .count();
                    let match_ended_last = matches!(events.last(), Some((_, RoundEvent::MatchEnded)));
                    if poke_refire {
                        println!("FAIL: a state-poking primer trap re-fired post-battle");
                        std::process::exit(2);
                    }
                    if started == 0 || ended_with_outcome != started || !match_ended_last {
                        println!(
                            "FAIL: unbalanced trace (started={started} ended-with-outcome={ended_with_outcome} match-ended-last={match_ended_last})"
                        );
                        std::process::exit(1);
                    }
                    println!(
                        "SUCCESS: {started} battle(s); menu0={:02x?} menu1={:02x?}",
                        menu[0], menu[1]
                    );
                    std::process::exit(0);
                }
                Phase::Epilogue { until }
            }
        };

        if t % 600 == 0 {
            println!(
                "[{t:5}] {phase:?} menu0={:02x?} menu1={:02x?} pc0={:08x} pc1={:08x}",
                menu[0],
                menu[1],
                pair.core(0).gba().cpu().thumb_pc(),
                pair.core(1).gba().cpu().thumb_pc(),
            );
        }
    }

    dump_screens(&mut pair, "timeout");
    {
        let s = store.lock().unwrap();
        println!("---- event trace at timeout ----");
        for (et, ev) in s.events() {
            println!("[{et:5}] {ev:?}");
        }
    }
    println!(
        "TIMEOUT/STUCK in {phase:?}: menu0={:02x?} menu1={:02x?} pc0={:08x} pc1={:08x}",
        pvp0.debug_menu_state(pair.core_mut(0)),
        pvp1.debug_menu_state(pair.core_mut(1)),
        pair.core(0).gba().cpu().thumb_pc(),
        pair.core(1).gba().cpu().thumb_pc(),
    );
    std::process::exit(1);
}
