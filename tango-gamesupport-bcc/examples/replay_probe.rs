//! Re-simulate a recorded BCC replay the way the app's stats path does
//! and print what the fold made of it, plus the raw per-tick chip
//! stream — for checking the poller against a real match instead of a
//! synthetic pair.
//!
//! Usage: replay_probe <rom> <replay> [<rom2>]

use tango_backend_mgba::GameSupport as _;

fn pvp_for(rom: &[u8]) -> &'static tango_gamesupport_bcc::pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"A89E" => &tango_gamesupport_bcc::pvp::PVP_A89E_00,
        b"A89J" => &tango_gamesupport_bcc::pvp::PVP_A89J_00,
        code => panic!("not a bcc rom (code {:02x?})", code),
    }
}

fn main() {
    mgba::log::install_default_logger();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom0 = std::fs::read(&args[0]).expect("rom unreadable");
    let rom1 = args.get(2).map(|p| std::fs::read(p).expect("rom2 unreadable")).unwrap_or_else(|| rom0.clone());
    let replay = tango_replay::Replay::decode(std::fs::File::open(&args[1]).expect("replay unreadable"))
        .expect("replay did not decode");
    println!(
        "replay: complete={} local_player={} inputs={} match_type={}.{}",
        replay.is_complete,
        replay.local_player_index,
        replay.inputs.len(),
        replay.metadata.match_type,
        replay.metadata.match_subtype,
    );

    let (pvp0, pvp1) = (pvp_for(&rom0), pvp_for(&rom1));
    let local_player = replay.local_player_index as usize;
    let inputs: Vec<[u32; 2]> = replay.inputs.iter().map(|&row| row.map(|i| i.keys as u32)).collect();
    let stats = tango_backend_mgba::analysis::analyze(
        tango_backend_mgba::analysis::AnalyzeConfig {
            roms: [rom0.clone(), rom1.clone()],
            saves: replay.srams.clone(),
            support: [pvp0, pvp1],
            match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            rng_seed: replay.rng_seed,
            rtc: replay.rtc_time(),
            local_player,
            inputs: &inputs,
        },
        &mut |_, _, _| {},
        &std::sync::atomic::AtomicBool::new(false),
    )
    .expect("analyze failed");

    // REPLAY_PROBE_TRACE=<from>-<to>: re-simulate the same replay and
    // print the battle block tick by tick over that window, so a chip
    // event can be lined up against what the game's RAM was doing.
    if let Ok(win) = std::env::var("REPLAY_PROBE_TRACE") {
        let (from, to) = win.split_once('-').expect("REPLAY_PROBE_TRACE=<from>-<to>");
        let (from, to): (u32, u32) = (from.parse().unwrap(), to.parse().unwrap());
        let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
            sides: vec![
                mgba_rollback::SideOptions { rom: rom0.clone(), save: Some(replay.srams[0].clone()) },
                mgba_rollback::SideOptions { rom: rom1.clone(), save: Some(replay.srams[1].clone()) },
            ],
            rtc: Some(replay.rtc_time()),
            peripheral: mgba_rollback::Peripheral::Cable,
        })
        .unwrap();
        let config = tango_backend_mgba::PrimeConfig {
            match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            rng_seed: replay.rng_seed,
            disable_bgm: false,
        };
        let events_sink = tango_match::telemetry::EventSink::new();
        let primed = [tango_backend_mgba::PrimedLatch::new(), tango_backend_mgba::PrimedLatch::new()];
        pair.set_traps(0, pvp0.primer_traps(&config, 0, &events_sink, &primed[0]));
        pair.set_traps(1, pvp1.primer_traps(&config, 1, &events_sink, &primed[1]));
        while !(primed[0].is_set() && primed[1].is_set()) {
            pair.tick(&[0, 0]);
        }
        let watch: Vec<u32> = std::env::var("REPLAY_PROBE_WATCH")
            .map(|v| v.split(',').map(|a| u32::from_str_radix(a.trim().trim_start_matches("0x"), 16).unwrap()).collect())
            .unwrap_or_else(|_| vec![0x02005154, 0x02005156, 0x02005157, 0x02005144, 0x02005148, 0x02005160, 0x02005161]);
        let mut prev: Option<Vec<u8>> = None;
        for (i, &keys) in inputs.iter().enumerate() {
            let tick = i as u32 + 1;
            pair.tick(&[keys[0], keys[1]]);
            if tick < from || tick > to {
                continue;
            }
            // REPLAY_PROBE_SHOT_AT=<tick>[,...]: dump both screens, for
            // reading what the game was showing at a moment of interest.
            for spec in std::env::var("REPLAY_PROBE_SHOT_AT").unwrap_or_default().split(',') {
                if spec.parse::<u32>() == Ok(tick) {
                    for side in 0..2 {
                        if let Some(buf) = pair.video_buffer(side) {
                            image::RgbImage::from_fn(240, 160, |x, y| {
                                let off = ((y * 240 + x) * 2) as usize;
                                let v = u16::from_le_bytes([buf[off], buf[off + 1]]);
                                image::Rgb([
                                    ((v & 31) as u8) << 3,
                                    (((v >> 5) & 31) as u8) << 3,
                                    (((v >> 10) & 31) as u8) << 3,
                                ])
                            })
                            .save(format!("replay_t{tick:05}_c{side}.png"))
                            .unwrap();
                        }
                    }
                }
            }
            let core = pair.core_mut(0);
            // REPLAY_PROBE_EWRAM_AT=<tick>:<file>[,...]
            for spec in std::env::var("REPLAY_PROBE_EWRAM_AT").unwrap_or_default().split(',') {
                if let Some((t, path)) = spec.split_once(':') {
                    if t.parse::<u32>() == Ok(tick) {
                        let mut buf = vec![0u8; 0x40000];
                        core.raw_read_range(0x02000000, -1, &mut buf);
                        std::fs::write(path, buf).unwrap();
                    }
                }
            }
            let hp = [core.raw_read_16(0x0200513c, -1) as u16, core.raw_read_16(0x0200513e, -1) as u16];
            let now: Vec<u8> = watch.iter().map(|&a| core.raw_read_8(a, -1)).collect();
            let line = (now.clone(), hp);
            if prev.as_ref() != Some(&line.0) {
                let cells: Vec<String> = watch.iter().zip(&now).map(|(a, v)| format!("{a:08x}={v:3}")).collect();
                println!("[{tick:5}] hp={hp:?}  {}", cells.join(" "));
                prev = Some(now);
            }
        }
    }

    // The stats are flat series over the whole match with the rounds
    // marking them up, so each round's share is whatever falls inside
    // its span.
    for i in 0..stats.rounds.len() {
        let Some((start, end)) = stats.round_span(i, None) else { continue };
        let within = |t: u32| t >= start && t < end;
        let hp: Vec<_> = stats.hp.iter().filter(|p| within(p.tick)).collect();
        println!(
            "round {i}: {start}..{end} outcome={:?} hp_points={} custom={} uses={:?}",
            stats.rounds[i].outcome,
            hp.len(),
            stats.custom.iter().filter(|&&(a, _)| within(a)).count(),
            stats
                .chip_uses
                .iter()
                .map(|v| v.iter().filter(|&&(t, _)| within(t)).count())
                .collect::<Vec<_>>()
        );
        for side in 0..2 {
            let uses: Vec<_> = stats.chip_uses[side].iter().filter(|&&(t, _)| within(t)).collect();
            println!("   side{side}: {uses:?}");
        }
        for p in hp.iter().take(30) {
            println!("      hp @{} = {}/{}", p.tick, p.local, p.remote);
        }
    }
}
