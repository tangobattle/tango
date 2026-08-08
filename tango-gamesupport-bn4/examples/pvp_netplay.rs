//! Headless two-peer integration test for the SIO engine: builds two
//! matches (one per player) over an in-process wire with injected
//! latency, drives them to a live battle, forces mispredictions, and
//! checks that (a) both peers settle identical confirmed input rows,
//! (b) both peers' settled telemetry reads identical battle state at
//! every commonly settled tick, and (c) telemetry sees the round start
//! with pollable HP.
//!
//! Usage: pvp_netplay <rom0> <save0> [<rom1> <save1>] [ticks] [latency]

use std::collections::{HashMap, VecDeque};

use tango_gamesupport_bn4::{pvp, FAMILY};
use tango_match::telemetry::Event;
use tango_match::{Backend as _, HostInput};

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"B4WE" => &pvp::PVP_B4WE_00,
        b"B4BE" => &pvp::PVP_B4BE_00,
        b"B4WJ" => &pvp::PVP_B4WJ_01,
        b"B4BJ" => &pvp::PVP_B4BJ_01,
        code => panic!("not a supported rom (code {:02x?})", code),
    }
}

fn build(
    roms: [Vec<u8>; 2],
    saves: [Vec<u8>; 2],
    pvp: [&'static pvp::Pvp; 2],
    local_player: usize,
) -> tango_match::Match {
    let peer = &roms[1 - local_player];
    let peer_rom = tango_match::PeerRom {
        code: peer[0xac..0xb0].try_into().unwrap(),
        revision: peer[0xbc],
    };
    tango_backend_mgba::GbaBackend::new(pvp[local_player], FAMILY)
        .start(tango_match::StartConfig {
            roms: [&roms[0], &roms[1]],
            saves: [Some(&saves[0]), Some(&saves[1])],
            rng_seed: *b"sio-probe-seed!!",
            rtc: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000),
            match_type: (0, 0),
            peer_rom,
            local_player,
            present_delay: 2,
            disable_bgm: false,
            // Nobody is listening: the harness drives the pair headless.
            audio: None,
            cancel: None,
            trainer: None,
        })
        .expect("build match")
}

fn main() {
    env_logger::init();

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
    let nums: Vec<u32> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let ticks = nums.first().copied().unwrap_or(3000);
    let latency = nums.get(1).copied().unwrap_or(5) as usize;

    let pvps = [pvp_for(&rom0), pvp_for(&rom1)];

    println!("priming both peers…");
    let mut peers = [
        build([rom0.clone(), rom1.clone()], [save0.clone(), save1.clone()], pvps, 0),
        build([rom0, rom1], [save0, save1], pvps, 1),
    ];
    println!("both peers primed to a live battle.");

    // In-flight packet wires, one per direction, delayed by `latency`.
    let mut wire: [VecDeque<(HostInput, i16)>; 2] = [VecDeque::new(), VecDeque::new()];
    let mut round_started = [false, false];
    // The settled trajectory is shared state: whichever peer settles a
    // tick first records it here, and the other must agree bit for bit
    // — on the confirmed input row, and on the telemetry read off the
    // settled simulation.
    let mut input_rows: HashMap<u32, [u32; 2]> = HashMap::new();
    let mut hp_rows: HashMap<u32, [u16; 2]> = HashMap::new();
    let mut agreed = 0u32;

    for frame in 0..ticks {
        for p in 0..2 {
            // Alternating held buttons per peer, phased, so the streams
            // differ and predictions sometimes miss → rollbacks.
            let keys = if (frame + (p as u32) * 7) % 5 < 2 { 1u32 } else { 0 };
            let (_tick, outgoing, tick_advantage) = peers[p].advance(HostInput::keys(keys)).expect("advance");
            wire[p].push_back((outgoing, tick_advantage));
        }
        // Deliver anything older than `latency` frames.
        for p in 0..2 {
            while wire[p].len() > latency {
                let (input, adv) = wire[p].pop_front().unwrap();
                peers[1 - p].add_remote_input(input, adv);
            }
        }

        // Cross-check what each peer settled this frame.
        for p in 0..2 {
            for (tick, row) in peers[p].drain_confirmed() {
                let row = row.map(|input| input.keys);
                if let Some(other) = input_rows.insert(tick, row) {
                    if other != row {
                        panic!("DESYNC at tick {tick}: peer confirmed {row:?}, other {other:?}");
                    }
                    agreed += 1;
                }
            }
            let confirmed = peers[p].confirmed();
            let (samples, events) = peers[p]
                .telemetry()
                .expect("sio matches publish telemetry")
                .lock()
                .unwrap()
                .drain_confirmed(confirmed);
            for (tick, obs) in samples {
                let hp = [obs.units[0].hp, obs.units[1].hp];
                if let Some(other) = hp_rows.insert(tick, hp) {
                    if other != hp {
                        panic!("DESYNC at tick {tick}: peer sees HP {hp:?}, other {other:?}");
                    }
                    agreed += 1;
                }
            }
            for (tick, event) in events {
                if !round_started[p] && matches!(event, Event::RoundStarted) {
                    println!("peer{p}: round started at tick {tick}");
                    round_started[p] = true;
                }
            }
        }
    }

    let live = round_started[0] && round_started[1];
    println!("frames={ticks} latency={latency} settled-agreements={agreed} both-in-battle={live}");
    if !live {
        eprintln!("FAIL: at least one peer never entered a battle");
        std::process::exit(1);
    }
    if agreed == 0 {
        eprintln!("FAIL: no settled state was cross-checked");
        std::process::exit(1);
    }
    println!("PASS: two peers converged with telemetry, no desync across {agreed} checked rows");
}
