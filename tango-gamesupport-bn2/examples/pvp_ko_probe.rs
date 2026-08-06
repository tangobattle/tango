//! Headless round/match events_sink probe: prime a lockstep pair, force a
//! KO, and drive the games' REAL post-battle flow — result screens back
//! to the comm menu — tracing every telemetry events_sink event and every
//! primer/diagnostic trap firing along the way.
//!
//! bn2 matches are a single battle (no rematch conversation), so the
//! expected event trace is: Started → Ended{Some(outcome)} at the game's
//! own win/loss judgment → MatchEnded at the battle-mode exit. After the
//! match the probe keeps mashing through the comm menu for a while and
//! FAILS if any state-poking primer trap (boot fast-path, comm-menu
//! routing) re-fires — those must stay inert once the battle has run.
//!
//! Usage: pvp_ko_probe <rom> <save> [<rom2> <save2>] [max_ticks]
//!
//! The KO recipe: once the battle is live, poke player 1's in-battle HP
//! to 1 on BOTH cores in the same tick (each core simulates the whole
//! battle, so identical pokes keep the pair coherent), then mash a
//! deterministic input pattern — bn2's buster does 1 damage, so the KO
//! lands within seconds of sim time. Unlike shipped priming code, a
//! probe MAY inject real pad input; that's how it walks the post-battle
//! screens.
//!
//! Env:
//! - PVP_KO_ADVANCE=<keys>: post-KO input policy (advance the result
//!   screens / wander the comm menu). Default "A". Key strings cycle one
//!   character per 10 ticks, pressed for the first 2: A/B/U/D/L/R/
//!   S(tart)/E(select), '.' = nothing.
//! - PVP_KO_VICTIM=<0|1>: which player's HP to poke to 1 (default 1),
//!   for exercising both the set_win and set_loss judgment paths.
//! - PVP_KO_DUMP_DIR: dump screens at phase transitions.
//!
//! Exits 0 on the full expected trace with no post-battle poke re-fires;
//! exits 1 on timeout, 2 on a poke re-fire.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tango_gamesupport_bn2::pvp;
use tango_match::telemetry::Event;
use tango_backend_mgba::GameSupport as _;

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"AE2E" => &pvp::PVP_AE2E_00,
        b"AE2J" => &pvp::SIO_AE2J_00_AC,
        code => panic!("not a bn2 rom (code {:02x?})", code),
    }
}

/// Trap-era events_sink anchors (disasm-verified for these ROMs), installed
/// as print-only diagnostics so the probe shows WHICH of them the real
/// protocol route actually executes. Anchors the primer already traps are
/// skipped (the primer wrapper logs those).
fn diag_anchors(rom: &[u8]) -> &'static [(&'static str, u32)] {
    match &rom[0xac..0xb0] {
        b"AE2E" => &[
            ("round_call_jump_table_ret", 0x08005834),
            ("round_ending_entry1", 0x08005c3a),
            ("round_ending_entry2", 0x08005de6),
            ("round_end_entry", 0x08006114),
            ("round_end_set_win", 0x08006ec8),
            ("round_end_set_loss", 0x08006ed0),
            ("round_end_damage_judge_set_win", 0x08005fd8),
            ("round_end_damage_judge_set_loss", 0x08005fc8),
            ("round_end_damage_judge_set_draw", 0x08005fbe),
        ],
        b"AE2J" => &[
            ("round_call_jump_table_ret", 0x08005830),
            ("round_ending_entry1", 0x08005c2a),
            ("round_ending_entry2", 0x08005dd6),
            ("round_end_entry", 0x08006104),
            ("round_end_set_win", 0x08006d88),
            ("round_end_set_loss", 0x08006d90),
            ("round_end_damage_judge_set_win", 0x08005fc8),
            ("round_end_damage_judge_set_loss", 0x08005fb8),
            ("round_end_damage_judge_set_draw", 0x08005fae),
        ],
        code => panic!("not a bn2 rom (code {:02x?})", code),
    }
}

/// The primer's state-POKING trap sites (boot fast-path + comm-menu
/// routing). If any of these re-fires after the battle has run, the poke
/// corrupts the games' own post-battle flow — the probe fails on it.
fn poke_anchors(rom: &[u8]) -> &'static [u32] {
    match &rom[0xac..0xb0] {
        b"AE2E" => &[0x08024a7c, 0x0801c302, 0x0801c174, 0x08003ccc, 0x0802b19c, 0x0802b2a0],
        b"AE2J" => &[0x08024984, 0x0801c18e, 0x0801c000, 0x08003ccc, 0x0802b018, 0x0802b11c],
        code => panic!("not a bn2 rom (code {:02x?})", code),
    }
}

const NOISY_ANCHORS: &[&str] = &["round_call_jump_table_ret"];

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
        streak: u32,
    },
    /// Wiggle until the poked player's HP hits 0.
    Fighting,
    /// Mash the advance policy through the result screens until the full
    /// expected trace (Ended with outcome + MatchEnded) is in the store.
    PostBattle {
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
    let victim: u8 = std::env::var("PVP_KO_VICTIM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

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
        match_type: (0, 0),
        rng_seed: *b"sio-probe-seed!!",
        disable_bgm: false,
    };
    let events_sink = tango_match::telemetry::EventSink::new();
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
    pair.set_traps(0, instrument(0, pvp0.primer_traps(&config, 0, &events_sink, &primed[0])));
    pair.set_traps(1, instrument(1, pvp1.primer_traps(&config, 1, &events_sink, &primed[1])));

    // The real engine telemetry observer: pollers + the events_sink sink,
    // recording into the shared store the probe prints from.
    let (mut telemetry, store) =
        tango_match::telemetry::Telemetry::new([pvp0.core_poller(0), pvp1.core_poller(1)], events_sink.clone());

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
            Phase::AwaitLive { .. } | Phase::Fighting => {
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
                    Phase::AwaitLive { streak: 0 }
                } else {
                    Phase::Priming
                }
            }
            Phase::AwaitLive { streak } => {
                let live = [
                    pvp0.debug_battle_hp(pair.core_mut(0)),
                    pvp1.debug_battle_hp(pair.core_mut(1)),
                ]
                .iter()
                .all(|hp| hp.is_some_and(|hp| hp[0] > 1 && hp[1] > 1));
                let streak = if live { streak + 1 } else { 0 };
                if streak >= 60 {
                    println!("[{t:5}] battle live; poking player {victim} HP to 1 on both cores");
                    pvp0.debug_set_hp(pair.core_mut(0), victim, 1);
                    pvp1.debug_set_hp(pair.core_mut(1), victim, 1);
                    dump_screens(&mut pair, "live");
                    Phase::Fighting
                } else {
                    Phase::AwaitLive { streak }
                }
            }
            Phase::Fighting => {
                if pvp0
                    .debug_battle_hp(pair.core_mut(0))
                    .is_some_and(|hp| hp[victim as usize] == 0)
                {
                    println!("[{t:5}] KO (player {victim} at 0 HP)");
                    dump_screens(&mut pair, "ko");
                    ko_tick = Some(t);
                    Phase::PostBattle { since: t }
                } else {
                    Phase::Fighting
                }
            }
            Phase::PostBattle { since } => {
                // The full expected single-battle trace, in order and
                // after the KO: Ended with an announced outcome, then
                // MatchEnded.
                let done = {
                    let s = store.lock().unwrap();
                    let events = s.events();
                    let ended_at = events.iter().find_map(|(et, e)| match e {
                        Event::RoundEnded { outcome: Some(_) } => Some(*et),
                        _ => None,
                    });
                    let match_ended_at = events
                        .iter()
                        .find_map(|(et, e)| matches!(e, Event::MatchEnded).then_some(*et));
                    match (ended_at, match_ended_at) {
                        (Some(e), Some(m)) => e <= m && m >= since,
                        _ => false,
                    }
                };
                if done {
                    println!("[{t:5}] full events_sink trace observed; epilogue");
                    dump_screens(&mut pair, "match_end");
                    Phase::Epilogue { until: t + 1200 }
                } else if t - since > POST_BATTLE_BUDGET {
                    println!("[{t:5}] post-battle budget exhausted");
                    dump_screens(&mut pair, "stuck");
                    break;
                } else {
                    Phase::PostBattle { since }
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
                    if poke_refire {
                        println!("FAIL: a state-poking primer trap re-fired post-battle");
                        std::process::exit(2);
                    }
                    println!("SUCCESS: menu0={:02x?} menu1={:02x?}", menu[0], menu[1]);
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
