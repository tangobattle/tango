//! Forced-hand probe: the training-mode chip forcer run against a raw
//! lockstep pair — the same shape as a live training session (both
//! pads supplied before every tick, no rollback session at all), with
//! the per-game [`Trainer`](tango_match::trainer::Trainer) driven per
//! core per tick exactly as the engine's link drives it.
//!
//! Per turn: both players' custom screens are closed by the scripted
//! START→A sequence (START jumps the cursor to OK; never DOWN — that
//! lands on Beast Out), then the probe asserts both cores agree that
//! both players' committed hands are the forced lists (fired = 0,
//! ids[0] = the forced lead), then A-taps so chips actually fire and
//! checks the confirmed `ChipUsed` events carry forced ids. L-taps
//! open the next turn's screen once the gauge fills; the second turn's
//! assert covers the "re-applied at every close" contract.
//!
//! This is the empirical gate for the write's timing: if the game
//! re-touched the blocks when the battle resumes (after the SECOND
//! player confirms), a forced hand would be clobbered back to the
//! game's own picks and the assert would see them.
//!
//! Usage: training_forced_hand <rom> <save>
//!
//! Exits 0 on success, 1 on a failed assert or timeout.

use tango_backend_mgba::GameSupport as _;
use tango_gamesupport_bn6::pvp;
use tango_match::telemetry::{Event, Telemetry};

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"BR5E" => &pvp::PVP_BR5E_00,
        b"BR6E" => &pvp::PVP_BR6E_00,
        b"BR5J" => &pvp::PVP_BR5J_00,
        b"BR6J" => &pvp::PVP_BR6J_00,
        code => panic!("not a bn6 rom (code {:02x?})", code),
    }
}

/// The forced lists under test, per absolute player: the Cannon line
/// for player 0, the next three standards for player 1 — distinct per
/// side so a cross-write would be visible.
const FORCED: [&[u16]; 2] = [&[1, 2, 3], &[4, 5, 6]];

/// Raw dump of both players' chip blocks off one core — the fired
/// cursor, the plain ids, and the annotated picks at +0x32 the game
/// refreshes the ids from. The address is BN6's `chip_blocks` (same
/// across all four ROM variants).
fn dump_hands(core: &mut mgba::core::Core) -> String {
    let mut out = String::new();
    for p in 0..2u32 {
        let base = 0x020349c0 + p * 0x50;
        let fired = core.raw_read_16(base, -1);
        let ids: Vec<String> = (0..6)
            .map(|slot| format!("{:04x}", core.raw_read_16(base + 2 + slot * 2, -1)))
            .collect();
        let annotated: Vec<String> = (0..6)
            .map(|slot| format!("{:04x}", core.raw_read_16(base + 0x32 + slot * 2, -1)))
            .collect();
        out.push_str(&format!(
            " p{p}[fired={fired} ids={} picks={}]",
            ids.join(","),
            annotated.join(","),
        ));
    }
    out
}

/// One player's share of the per-tick input script.
#[derive(Clone, Copy, PartialEq, Debug)]
enum PlayerPhase {
    /// Custom screen open: debounce, then loop the close script
    /// (START 3 ticks, idle 3, A 3, idle 3) until the flag falls.
    Custom { since: u32 },
    /// In battle (or the shared pause, for whoever confirmed first).
    Fight { since: u32 },
}

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let save = std::fs::read(&args[1]).expect("save unreadable");
    let support = pvp_for(&rom);

    let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
        sides: vec![
            mgba_rollback::SideOptions {
                rom: rom.clone(),
                save: Some(save.clone()),
            },
            mgba_rollback::SideOptions {
                rom,
                save: Some(save),
            },
        ],
        rtc: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000)),
        peripheral: mgba_rollback::Peripheral::Cable,
    })
    .unwrap();

    let config = tango_backend_mgba::PrimeConfig {
        // Training's own match type: one single battle.
        match_type: (0, 0),
        rng_seed: *b"sio-probe-seed!!",
        disable_bgm: false,
    };
    let events_sink = tango_match::telemetry::EventSink::new();
    let primed = [
        tango_backend_mgba::PrimedLatch::new(),
        tango_backend_mgba::PrimedLatch::new(),
    ];
    pair.set_traps(0, support.primer_traps(&config, 0, &events_sink, &primed[0]));
    pair.set_traps(1, support.primer_traps(&config, 1, &events_sink, &primed[1]));
    println!("priming…");
    let mut t = 0u32;
    while !(primed[0].is_set() && primed[1].is_set()) {
        assert!(t < 3600, "priming timed out");
        pair.tick(&[0, 0]);
        t += 1;
    }
    println!("primed in {t} ticks.");

    let (mut telemetry, store) = Telemetry::new([support.core_poller(0), support.core_poller(1)], events_sink);
    // The production write path: the game's trainer over a live control
    // with both hands forced, ticked per core per tick like the
    // engine's link does it (trainer before the telemetry poll).
    let control = tango_match::trainer::TrainerControl::new();
    let mut trainer = support.trainer().expect("bn6 offers a trainer");
    // TFH_ORGANIC=1: leave the hands alone and have the close script
    // pick three chips first — a reference run for what the game's own
    // block looks like, dumped at each fire.
    let organic = std::env::var("TFH_ORGANIC").is_ok();
    if !organic {
        for (player, ids) in FORCED.iter().enumerate() {
            control.set_forced_hand(player, Some(ids.to_vec()));
        }
    }

    // Frontier pollers for the input script's own view (edge reports go
    // nowhere) — custom_self is per-core, core p answering player p.
    let mut frontier = [support.core_poller(0), support.core_poller(1)];

    let mut phase = [PlayerPhase::Fight { since: 0 }; 2];
    /// Ticks of custom-flag debounce before the close script engages.
    const DEBOUNCE: u32 = 20;
    /// Post-close idle before the committed hands are asserted — after
    /// the shared pause has had time to end, before any tap can fire a
    /// chip and advance the counter (taps are gated on the assert).
    const ASSERT_AT: u32 = 30;

    let mut turns = 0u32;
    let mut asserted_turns = 0u32;
    let mut prev_any_custom = false;
    // Confirmed chip fires per player, across the whole run.
    let mut fires: [Vec<u16>; 2] = [Vec::new(), Vec::new()];

    let mut tick = 0u32;
    let mut deadline = None;
    for _ in 0..30_000u32 {
        // Each player's input for the tick about to advance, from the
        // pair's settled state — the same order training's driver runs.
        let mut custom = [false; 2];
        for p in 0..2 {
            custom[p] = frontier[p]
                .poll(pair.core_mut(p), &tango_match::telemetry::EventSink::new(), 0)
                .is_some_and(|o| o.custom_self);
        }
        let mut keys = [0u32; 2];
        for p in 0..2 {
            phase[p] = match (phase[p], custom[p]) {
                (PlayerPhase::Fight { .. }, true) => PlayerPhase::Custom { since: 0 },
                (PlayerPhase::Custom { .. }, false) => PlayerPhase::Fight { since: 0 },
                (PlayerPhase::Custom { since }, true) => PlayerPhase::Custom { since: since + 1 },
                (PlayerPhase::Fight { since }, false) => PlayerPhase::Fight { since: since + 1 },
            };
            keys[p] = match phase[p] {
                PlayerPhase::Custom { since } if since >= DEBOUNCE => {
                    // Organic reference: take the first three offered
                    // chips (A on the cursor advances it), then the
                    // close script. Forced runs skip straight to it:
                    // START (cursor to OK), gap, A, gap — repeating
                    // until the flag falls. Held 3 ticks each so the
                    // game sees clean edges.
                    let script = since - DEBOUNCE;
                    if organic && script < 36 {
                        if script % 12 < 3 {
                            tango_match::keys::A
                        } else {
                            0
                        }
                    } else {
                        match (since - DEBOUNCE) % 12 {
                            0..=2 => tango_match::keys::START,
                            6..=8 => tango_match::keys::A,
                            _ => 0,
                        }
                    }
                }
                // In battle, once this turn's hands are asserted: A
                // bursts fire the loaded chip, L taps open the next
                // turn's screen when the gauge fills (inert before).
                PlayerPhase::Fight { since } if turns == asserted_turns => {
                    if since % 30 < 3 {
                        tango_match::keys::A
                    } else if since % 47 < 3 {
                        tango_match::keys::L
                    } else {
                        0
                    }
                }
                _ => 0,
            };
        }

        pair.tick(&keys);
        tick += 1;
        // The engine's order: trainer first, telemetry after, so the
        // pollers read post-write state.
        for i in 0..2 {
            trainer.tick(pair.core_mut(i), i, &control);
        }
        tango_backend_mgba::analysis::observe_pair(&mut telemetry, &mut pair, tick);

        let (_samples, events) = store.lock().unwrap().drain_confirmed(tick);
        for (at, ev) in events {
            if let Event::ChipUsed { player, chip } = ev {
                println!("[{at:5}] player {player} fired chip {chip:#05x};{}", dump_hands(pair.core_mut(0)));
                fires[player].push(chip);
            }
        }

        // Turn accounting: BN6 opens both players' screens together, so
        // a turn is one episode of either custom flag standing open.
        let any_custom = custom.iter().any(|&c| c);
        if any_custom && !prev_any_custom {
            turns += 1;
            println!("[{tick:5}] turn {turns}: custom screens open");
        }
        prev_any_custom = any_custom;

        // The assert window: this turn's screens all closed (the two
        // players confirm at different ticks — the last one out ends
        // the episode) and the post-close idle has passed. The
        // committed hands must be the forced lists, on both cores, for
        // both players, with nothing fired yet.
        if turns > asserted_turns
            && !any_custom
            && phase.iter().all(|p| matches!(p, PlayerPhase::Fight { .. }))
            && phase
                .iter()
                .map(|p| match p {
                    PlayerPhase::Fight { since } => *since,
                    PlayerPhase::Custom { .. } => unreachable!(),
                })
                .min()
                == Some(ASSERT_AT)
        {
            if organic {
                println!("[{tick:5}] turn {turns} organic reference:{}", dump_hands(pair.core_mut(0)));
            } else {
                for i in 0..2 {
                    let loaded = support.debug_loaded_chips(pair.core_mut(i));
                    for p in 0..2 {
                        let expected = FORCED[p][0];
                        match loaded[p] {
                            Some(chip) if chip.id == expected && chip.fires == 0 => {}
                            got => {
                                println!(
                                    "FORCED HAND PROBE FAIL: turn {turns} core {i} player {p}: loaded {got:?}, expected id {expected:#05x} fires 0"
                                );
                                std::process::exit(1);
                            }
                        }
                    }
                }
                println!("[{tick:5}] turn {turns}: both cores agree both hands are forced");
            }
            asserted_turns = turns;
            if asserted_turns == 2 {
                // Run on a little longer so turn 2's fires confirm too.
                deadline = Some(tick + 600);
            }
        }

        if deadline.is_some_and(|d| tick >= d) {
            break;
        }
    }

    if asserted_turns < 2 {
        println!("FORCED HAND PROBE TIMEOUT: turns={turns} asserted={asserted_turns} phases={phase:?}");
        std::process::exit(1);
    }

    // Every confirmed fire must have come off a forced list, and both
    // players must actually have fired — the hand isn't just written,
    // it plays.
    for p in 0..2 {
        assert!(
            fires[p].iter().all(|chip| FORCED[p].contains(chip)),
            "player {p} fired outside the forced list: {:?}",
            fires[p]
        );
        assert!(!fires[p].is_empty(), "player {p} never fired a forced chip");
        assert_eq!(
            fires[p].first().copied(),
            Some(FORCED[p][0]),
            "player {p}'s first fire wasn't the forced lead"
        );
    }
    println!("FORCED HAND PROBE SUCCESS: fires p0={:?} p1={:?}", fires[0], fires[1]);
}
