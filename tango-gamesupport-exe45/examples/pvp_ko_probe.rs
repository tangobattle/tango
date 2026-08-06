//! Headless round/match events_sink probe: prime a lockstep pair, force
//! KOs, and drive the games' REAL battle-set flow — battles chained by
//! the game itself under triple battle, then the set teardown — tracing
//! every telemetry events_sink event and every primer trap firing along
//! the way.
//!
//! exe45's comm battle is one battle for mode 0 (single) and a
//! best-of-three SET for mode 1 (triple): mid-set, the round teardown
//! chains straight into the next battle (`round_start_ret` re-fires —
//! no menus between), and only the set-deciding battle's teardown takes
//! the exit path through `match_end_ret`. The probe KOs battle after
//! battle until the store shows MatchEnded, then verifies the trace up
//! to it is Started → Ended{Some(outcome)} per battle with MatchEnded
//! closing the set. exe45 quirks (see the in-line comments): the fight
//! wiggle is A + D-pad only — the other buttons are operator commands
//! that ABORT a link set — and the post-set flow auto-starts a new set
//! on idle pads (harmless live: the session tears down at MatchEnded),
//! so the trace check truncates at the first MatchEnded.
//!
//! Usage: pvp_ko_probe <rom> <save> [<rom2> <save2>] [max_ticks]
//!
//! The KO recipe: once a battle is live, poke the victim's in-battle HP
//! to 1 on BOTH cores in the same tick (each core simulates the whole
//! battle, so identical pokes keep the pair coherent), then mash a
//! deterministic input pattern until the KO lands. Unlike shipped
//! priming code, a probe MAY inject real pad input; that's how it
//! advances the result screens.
//!
//! Env:
//! - PVP_KO_MATCH_TYPE=<mode,subtype>: tango match-type selection;
//!   (0,0) = single battle, (1,0) = triple battle. Default "1,0" —
//!   the set mode is what live matches run.
//! - PVP_KO_VICTIMS=<pattern>: which player's HP to poke per battle,
//!   e.g. "1" (player 1 every battle, a 2-0 set), "10" (alternating —
//!   a full 2-1 tri set). Default "1".
//! - PVP_KO_ADVANCE=<keys>: post-KO input policy (advance the result
//!   screens). Default "A". Key strings cycle one character per 10
//!   ticks, pressed for the first 2: A/B/U/D/L/R/S(tart)/E(select),
//!   '.' = nothing.
//! - PVP_PROBE_DUMP_DIR: dump screens at phase transitions.
//!
//! Exits 0 on the full expected trace; 1 on timeout/unbalanced trace.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tango_gamesupport_exe45::pvp;
use tango_match::telemetry::Event;
use tango_backend_mgba::GameSupport as _;

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"BR4J" => &pvp::PVP_BR4J_00,
        code => panic!("not an exe45 rom (code {:02x?})", code),
    }
}

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
    let Ok(dir) = std::env::var("PVP_PROBE_DUMP_DIR") else {
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
    /// shows either the next round's Started (triple mid-set) or
    /// MatchEnded (set decided / single battle over).
    PostBattle {
        round: u32,
        since: u32,
    },
    /// Idle out the teardown, then verify the games sit at the comm menu.
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
    // Per-battle victim pattern: battle N's victim is pattern[(N-1) % len].
    let victims: Vec<u8> = std::env::var("PVP_KO_VICTIMS")
        .unwrap_or_else(|_| "1".to_string())
        .chars()
        .filter_map(|c| c.to_digit(10).map(|d| d as u8))
        .collect();
    let victim_for = |round: u32| victims[((round - 1) as usize) % victims.len().max(1)];

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
        // PVP_KO_MATCH_TYPE="mode,subtype": (0,0) = single battle,
        // (1,0) = triple battle. Default triple — the set mode is what
        // live matches run.
        match_type: std::env::var("PVP_KO_MATCH_TYPE")
            .ok()
            .and_then(|s| {
                let (m, sub) = s.split_once(',')?;
                Some((m.trim().parse().ok()?, sub.trim().parse().ok()?))
            })
            .unwrap_or((1, 0)),
        rng_seed: *b"sio-probe-seed!!",
        disable_bgm: false,
    };
    println!("match_type: {:?}", config.match_type);
    let events_sink = tango_match::telemetry::EventSink::new();
    let primed = [tango_backend_mgba::PrimedLatch::new(), tango_backend_mgba::PrimedLatch::new()];

    // Wrap every primer trap with a fire log (tick, core, address).
    let tick_now = Arc::new(AtomicU32::new(0));
    #[allow(clippy::type_complexity)]
    let fires: Arc<Mutex<Vec<(u32, usize, u32)>>> = Arc::new(Mutex::new(Vec::new()));
    let instrument = |traps: Vec<tango_backend_mgba::Trap>, core_idx: usize| -> Vec<tango_backend_mgba::Trap> {
        traps
            .into_iter()
            .map(|(addr, f)| {
                let tick_now = tick_now.clone();
                let fires = fires.clone();
                let g: Box<dyn Fn(&mut mgba::core::Core)> = Box::new(move |core: &mut mgba::core::Core| {
                    fires
                        .lock()
                        .unwrap()
                        .push((tick_now.load(Ordering::Relaxed), core_idx, addr));
                    f(core)
                });
                (addr, g)
            })
            .collect()
    };
    pair.set_traps(0, instrument(pvp0.primer_traps(&config, 0, &events_sink, &primed[0]), 0));
    pair.set_traps(1, instrument(pvp1.primer_traps(&config, 1, &events_sink, &primed[1]), 1));

    // The real engine telemetry observer: pollers + the events_sink sink,
    // recording into the shared store the probe prints from.
    let (mut telemetry, store) =
        tango_match::telemetry::Telemetry::new([pvp0.core_poller(0), pvp1.core_poller(1)], events_sink.clone());

    let mut phase = Phase::Priming;
    let mut events_printed = 0usize;
    let mut prev_menu = [[0u8; 8]; 2];
    let mut prev_hp: Option<[u16; 2]> = None;
    // Compress per-tick-hold trap fires into runs: (core, addr) -> (start, last, n).
    let mut runs: std::collections::HashMap<(usize, u32), (u32, u32, u32)> = std::collections::HashMap::new();

    for t in 0..max_ticks {
        tick_now.store(t, Ordering::Relaxed);

        let keys = match phase {
            Phase::Priming | Phase::Epilogue { .. } => [0, 0],
            Phase::AwaitLive { .. } | Phase::Fighting { .. } => {
                // The deterministic battle wiggle (distinct per core),
                // masked to A + D-pad: exe45 is an operation battle and
                // the other buttons are operator commands — mashing
                // those in a link set aborts it (the set exits with no
                // verdict, KO-probe observed), while fully idle pads
                // leave the navis passive forever.
                let mut keys = [0u32; 2];
                for (i, k) in keys.iter_mut().enumerate() {
                    *k = ((t / 5).wrapping_mul(2654435761) >> i) & 0x0f1;
                }
                keys
            }
            Phase::PostBattle { .. } => [keys_for(&advance_policy, t); 2],
        };
        pair.tick(&keys);
        tango_backend_mgba::analysis::observe_pair(&mut telemetry, &mut pair, t);

        // ---- tracing ----
        for (ft, core, addr) in fires.lock().unwrap().drain(..) {
            match runs.entry((core, addr)) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    let (start, last, n) = *e.get();
                    if ft <= last + 1 {
                        *e.get_mut() = (start, ft, n + 1);
                    } else {
                        println!("[{start:5}..{last:5}] core{core} TRAP {addr:08x} x{n}");
                        *e.get_mut() = (ft, ft, 1);
                    }
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert((ft, ft, 1));
                    println!("[{ft:5}] core{core} TRAP {addr:08x} fires");
                }
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
                    let ok0 = pvp0.debug_set_hp(pair.core_mut(0), victim, 1);
                    let ok1 = pvp1.debug_set_hp(pair.core_mut(1), victim, 1);
                    assert!(ok0 && ok1, "HP poke failed (ok0={ok0} ok1={ok1})");
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
                    Phase::PostBattle { round, since: t }
                } else {
                    Phase::Fighting { round }
                }
            }
            Phase::PostBattle { round, since } => {
                // After a KO the game either chains into the set's next
                // battle (triple mid-set: a fresh Started) or takes the
                // exit path (MatchEnded).
                let (started, match_ended) = {
                    let s = store.lock().unwrap();
                    let events = s.events();
                    (
                        events.iter().filter(|(_, e)| matches!(e, Event::RoundStarted)).count() as u32,
                        events.iter().any(|(_, e)| matches!(e, Event::MatchEnded)),
                    )
                };
                if match_ended {
                    println!("[{t:5}] MatchEnded observed after battle {round}; epilogue (input idles)");
                    dump_screens(&mut pair, "match_end");
                    Phase::Epilogue { until: t + 900 }
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
                    // The full expected trace UP TO the match end: one
                    // Ended with an announced outcome per Started,
                    // MatchEnded closing the set. Checked on the
                    // truncated trace because exe45's post-set flow
                    // AUTO-STARTS a new set ~100 ticks later even on
                    // idle pads (the primer's route state auto-continues
                    // — no re-poke fires; KO-probe mapped fires after
                    // battles 2 and 4 across back-to-back 2-0 sets). A
                    // live session tears down at MatchEnded, so the
                    // restart never runs there; the probe just ignores
                    // everything after it.
                    let events = s.events();
                    let end_pos = events.iter().position(|(_, e)| matches!(e, Event::MatchEnded));
                    let truncated = &events[..end_pos.map(|p| p + 1).unwrap_or(events.len())];
                    let started = truncated
                        .iter()
                        .filter(|(_, e)| matches!(e, Event::RoundStarted))
                        .count();
                    let ended_with_outcome = truncated
                        .iter()
                        .filter(|(_, e)| matches!(e, Event::RoundEnded { outcome: Some(_) }))
                        .count();
                    let match_ended_last = matches!(truncated.last(), Some((_, Event::MatchEnded)));
                    if started == 0 || ended_with_outcome != started || !match_ended_last {
                        println!(
                            "FAIL: unbalanced trace (started={started} ended-with-outcome={ended_with_outcome} match-ended-last={match_ended_last})"
                        );
                        std::process::exit(1);
                    }
                    println!(
                        "SUCCESS: {started} battle(s) to the match end; menu0={:02x?} menu1={:02x?}",
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
