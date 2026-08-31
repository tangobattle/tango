//! Session-level KO probe: the [`pvp_ko_probe`] flow run through the
//! REAL live stack — two rollback [`Session`]s cross-fed over an
//! in-process wire with latency (mispredictions and snapshot
//! save/load/rollback active), telemetry observers installed exactly
//! like the engine's `Match`, and lifecycle events drained off the
//! confirmed boundary exactly like the app's drive loop. This is the
//! seam the linear KO probes can't see: traps must keep firing across
//! `Pair::load`, and events must survive speculation + revocation to
//! come out of `take_through`.
//!
//! The KO poke rides `on_tick` (gated on pure state + tick number), so
//! rollback re-simulation re-applies it identically — both peers stay
//! digest-equal throughout.
//!
//! The match is the game's own battle set (default (1,0) = triple,
//! best-of-three; PVP_KO_MATCH_TYPE overrides): battle 1's KO chains
//! into battle 2 by the game itself, and the set-deciding KO takes the
//! exit path — MatchEnded must confirm on BOTH peers off the set's own
//! conclusion, with the games back at the comm menu (each peer's input
//! idles once it sees the confirmed match end).
//!
//! Usage: pvp_session_ko <rom> <save> [<rom2> <save2>]
//!
//! Exits 0 when BOTH peers observe the confirmed round verdicts and a
//! confirmed MatchEnded at the set's conclusion, with no desync; 1 on
//! timeout.

use std::collections::VecDeque;

use tango_gamesupport_bn6::pvp;
use tango_match::telemetry::{CorePoller, Event, Telemetry};
use tango_backend_mgba::GameSupport as _;

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"BR5E" => &pvp::PVP_BR5E_00,
        b"BR6E" => &pvp::PVP_BR6E_00,
        b"BR5J" => &pvp::PVP_BR5J_00,
        b"BR6J" => &pvp::PVP_BR6J_00,
        code => panic!("not a bn6 rom (code {:02x?})", code),
    }
}

/// [`Telemetry`] plus the deterministic KO poke: from `from_tick` on,
/// while the victim's HP is above 1, poke it to 1 on both cores. Pure
/// function of (tick, state), so re-simulation after a rollback
/// re-applies it identically and the poke lands inside that tick's
/// snapshot.
struct KoPoker {
    telemetry: Telemetry<mgba::core::Core>,
    support: [&'static pvp::Pvp; 2],
    hp_poller: Box<dyn CorePoller<mgba::core::Core>>,
    from_tick: u32,
}

impl mgba_rollback::session::TickObserver for KoPoker {
    fn on_tick(&mut self, pair: &mut mgba_rollback::Link, tick: u32) {
        if tick >= self.from_tick {
            // The HP read is levels-only; its edge reports go nowhere.
            let hp = self
                .hp_poller
                .poll(pair.core_mut(0), &tango_match::telemetry::EventSink::new(), 0)
                .map(|o| o.units.map(|u| u.hp));
            if hp.is_some_and(|hp| hp[1] > 1) {
                self.support[0].debug_set_hp(pair.core_mut(0), 1, 1);
                self.support[1].debug_set_hp(pair.core_mut(1), 1, 1);
            }
        }
        tango_backend_mgba::analysis::observe_pair(&mut self.telemetry, pair, tick);
    }

    fn on_rewind(&mut self, tick: u32) {
        self.telemetry.on_rewind(tick);
    }
}

/// One peer of the pair: the live-engine construction order (traps →
/// prime → telemetry → session → observer). The traps live inside the
/// pair's cores (see `mgba_rollback::Link::set_traps`), exactly like
/// `Match` installs them.
struct Peer {
    session: mgba_rollback::session::Session,
    store: tango_match::telemetry::TelemetryHandle,
}

fn build_peer(
    roms: [Vec<u8>; 2],
    saves: [Vec<u8>; 2],
    support: [&'static pvp::Pvp; 2],
    local_player: usize,
    ko_from_tick: u32,
) -> Peer {
    let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
        sides: vec![
            mgba_rollback::SideOptions {
                rom: roms[0].clone(),
                save: Some(saves[0].clone()),
            },
            mgba_rollback::SideOptions {
                rom: roms[1].clone(),
                save: Some(saves[1].clone()),
            },
        ],
        rtc: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000)),
        peripheral: mgba_rollback::Peripheral::Cable,
    })
    .unwrap();

    let config = tango_backend_mgba::PrimeConfig {
        // PVP_KO_MATCH_TYPE="mode,subtype"; default (1,0) = triple
        // battle — the set mode is what live matches run.
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
    let events_sink = tango_match::telemetry::EventSink::new();
    let primed = [tango_backend_mgba::PrimedLatch::new(), tango_backend_mgba::PrimedLatch::new()];
    pair.set_traps(0, support[0].primer_traps(&config, 0, &events_sink, &primed[0]));
    pair.set_traps(1, support[1].primer_traps(&config, 1, &events_sink, &primed[1]));
    let mut t = 0u32;
    while !(primed[0].is_set() && primed[1].is_set()) {
        assert!(t < 3600, "priming timed out");
        pair.tick(&[0, 0]);
        t += 1;
    }

    let (telemetry, store) = Telemetry::new([support[0].core_poller(0), support[1].core_poller(1)], events_sink);
    let mut session = mgba_rollback::session::Session::new(pair, local_player, 2).unwrap();
    session.set_observer(Some(Box::new(KoPoker {
        telemetry,
        support,
        hp_poller: support[0].core_poller(0),
        from_tick: ko_from_tick,
    })));
    Peer { session, store }
}

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
    let pvps = [pvp_for(&rom0), pvp_for(&rom1)];

    // Session ticks are post-priming; the battle intro runs a few
    // hundred ticks before units go live, and the poke gate (pollable
    // HP > 1) holds it off until they do.
    const KO_FROM_TICK: u32 = 120;
    const LATENCY: usize = 5;

    println!("priming both peers…");
    let mut peers = [
        build_peer(
            [rom0.clone(), rom1.clone()],
            [save0.clone(), save1.clone()],
            pvps,
            0,
            KO_FROM_TICK,
        ),
        build_peer([rom0, rom1], [save0, save1], pvps, 1, KO_FROM_TICK),
    ];
    println!("both peers primed to a live battle.");

    let mut wire: [VecDeque<(u32, i16)>; 2] = [VecDeque::new(), VecDeque::new()];
    let mut checkpoints: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();

    // Per-peer navigation phase, driven off that peer's own frontier
    // view (like a real player watching their screen). Each battle's
    // KO leads into A-taps that page the result screens; the game
    // itself chains the set's next battle (a fresh confirmed Started
    // flips the phase back to Fight) and ends the set off the deciding
    // KO. Once a peer sees the confirmed MatchEnded its input idles —
    // the games must land at the comm menu on their own.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Phase {
        Fight { round: u32 },
        Post { round: u32 },
    }
    let mut phase = [Phase::Fight { round: 1 }; 2];
    // Per-peer confirmed telemetry observations.
    let mut verdicts: [Vec<Option<tango_match::telemetry::Outcome>>; 2] = [Vec::new(), Vec::new()];
    let mut round_starts = [1u32, 1u32];
    let mut saw_match_end = [false; 2];
    // Peer 0 also folds its confirmed telemetry into match stats, the
    // way the app's drive loop does — validating the fold path (round
    // boundaries, rebased ticks, verdicts) end to end under rollback.
    let mut stats = tango_backend_mgba::analysis::StatsBuilder::new();

    let wiggle = |t: u32, i: usize| ((t / 5).wrapping_mul(2654435761u32) >> i) & 0x3f3;

    for frame in 0..40_000u32 {
        for p in 0..2 {
            let keys = match phase[p] {
                Phase::Fight { .. } => wiggle(frame, p),
                // Taps: A pages the result screens between the set's
                // battles. Idle once this peer's confirmed stream shows
                // the match end — the set is over and the games walk
                // themselves back to the comm menu.
                Phase::Post { .. } => {
                    if frame % 8 < 2 && !saw_match_end[p] {
                        1
                    } else {
                        0
                    }
                }
            };
            let (out, report) = peers[p].session.advance(keys).expect("advance");
            wire[p].push_back((out.keys, out.tick_advantage));

            // Drain confirmed telemetry exactly like the app's drive loop.
            let (samples, events) = peers[p].store.lock().unwrap().take_through(report.confirmed);
            if p == 0 {
                peers[p].session.drain_confirmed();
                tango_backend_mgba::analysis::fold_confirmed(&mut stats, 0, samples.clone(), events.clone());
            }
            for (tick, ev) in events {
                println!("[peer{p}][{tick:5}] confirmed EVENT {ev:?}");
                match ev {
                    Event::RoundEnded { outcome } => verdicts[p].push(outcome),
                    Event::MatchEnded => saw_match_end[p] = true,
                    Event::RoundStarted => {
                        if tick > 0 {
                            round_starts[p] += 1;
                            // A confirmed later round start is what the
                            // stats fold marks a round boundary from.
                            println!("[peer{p}][{tick:5}] -> round {} (round mark)", round_starts[p]);
                            phase[p] = Phase::Fight { round: round_starts[p] };
                        }
                    }
                    Event::ChipUsed { .. } => {}
                }
            }
        }
        for p in 0..2 {
            while wire[p].len() > LATENCY {
                let (keys, adv) = wire[p].pop_front().unwrap();
                peers[1 - p].session.add_remote_input(p, keys, adv);
            }
        }

        // Desync check at commonly settled boundaries.
        for peer in &mut peers {
            if let Some((tick, digest)) = peer.session.checkpoint() {
                match checkpoints.get(&tick) {
                    Some(&other) if other == digest => {}
                    Some(&other) => panic!("DESYNC at tick {tick}: {digest:08x} vs {other:08x}"),
                    None => {
                        checkpoints.insert(tick, digest);
                    }
                }
            }
        }

        // Phase transitions off each peer's own frontier view: once the
        // KO lands (either side at 0 HP), start driving the post-battle
        // conversation.
        for p in 0..2 {
            if let Phase::Fight { round } = phase[p] {
                let ko = {
                    let mut poller = pvps[0].core_poller(0);
                    peers[p].session.with_link(|pair| {
                        // Levels-only read; edge reports go nowhere.
                        poller
                            .poll(pair.core_mut(0), &tango_match::telemetry::EventSink::new(), 0)
                            .is_some_and(|o| o.units.iter().any(|u| u.hp == 0))
                    })
                };
                if ko {
                    println!("[peer{p}][frame {frame}] round {round} KO on the frontier; driving post-battle");
                    phase[p] = Phase::Post { round };
                }
            }
        }

        if saw_match_end.iter().all(|&e| e) {
            // Let both peers' remaining confirmations flush, then report.
            let ok = verdicts.iter().all(|v| v.len() == 2 && v.iter().all(|o| o.is_some()))
                && round_starts.iter().all(|&r| r == 2);
            println!(
                "verdicts={verdicts:?} rounds={round_starts:?} settled-checkpoints={}",
                checkpoints.len()
            );
            // Where each peer's games actually are: both parked back at
            // the comm menu after the set — nobody near the overworld.
            for (p, peer) in peers.iter_mut().enumerate() {
                peer.session.with_link(|pair| {
                    println!(
                        "peer{p} frontier: menu0={:02x?} menu1={:02x?}",
                        pvps[0].debug_menu_state(pair.core_mut(0)),
                        pvps[1].debug_menu_state(pair.core_mut(1)),
                    );
                });
            }
            // The folded stats must show two decided rounds on the
            // match's ONE contiguous timebase: ticks session-absolute
            // and strictly monotone across the round boundary (round
            // 2's samples sit after round 1's, where the match actually
            // put them — the recording is one contiguous input stream).
            let folded = stats.snapshot();
            // The stats are flat series marked up by the rounds, so a
            // round's points are the ones inside its span.
            let points = |i: usize| -> Vec<u32> {
                let Some((start, end)) = folded.round_span(i, None) else { return vec![] };
                folded
                    .hp
                    .iter()
                    .map(|p| p.tick)
                    .filter(|&t| t >= start && t < end)
                    .collect()
            };
            let shape: Vec<_> = (0..folded.rounds.len())
                .map(|i| {
                    let ticks = points(i);
                    (
                        folded.rounds[i].outcome,
                        ticks.first().copied(),
                        ticks.last().copied(),
                        ticks.len(),
                    )
                })
                .collect();
            println!("stats rounds (outcome, first tick, last tick, points): {shape:?}");
            let stats_ok = folded.rounds.len() == 2
                && (0..2).all(|i| folded.rounds[i].outcome.is_some() && points(i).len() >= 2)
                && points(0).last() < points(1).first();
            if ok && stats_ok {
                println!("SESSION KO PROBE SUCCESS at frame {frame}");
                std::process::exit(0);
            } else {
                println!("SESSION KO PROBE FAIL: match ended but verdicts/rounds/stats incomplete");
                std::process::exit(1);
            }
        }
    }

    println!("SESSION KO PROBE TIMEOUT: phases={phase:?} verdicts={verdicts:?} rounds={round_starts:?} match_end={saw_match_end:?}");
    for (p, peer) in peers.iter_mut().enumerate() {
        peer.session.with_link(|pair| {
            println!(
                "peer{p}: menu0={:02x?} menu1={:02x?}",
                pvps[0].debug_menu_state(pair.core_mut(0)),
                pvps[1].debug_menu_state(pair.core_mut(1)),
            );
            if let Ok(dir) = std::env::var("PVP_PROBE_DUMP_DIR") {
                for i in 0..2 {
                    pair.set_frameskip(i, 0);
                }
                pair.tick(&[0, 0]);
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
                        img.save(format!("{dir}/timeout_p{p}_c{i}.png")).unwrap();
                    }
                }
            }
        });
    }
    std::process::exit(1);
}
