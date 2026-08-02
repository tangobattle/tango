//! Headless probe: boot two BCC games as a lockstep pair, prime both
//! through their real menus into a link battle, and report how far the
//! walk got.
//!
//! Usage: pvp_probe <rom> <save> [<rom2> <save2>] [ticks]

use tango_gamesupport_bcc::pvp;
use tango_backend_mgba::GameSupport as _;

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"A89E" => &pvp::PVP_A89E_00,
        b"A89J" => &pvp::PVP_A89J_00,
        code => panic!("not a bcc rom (code {:02x?})", code),
    }
}

fn dump_screens(pair: &mut mgba_rollback::Link, dir: &str, tag: &str) {
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
    let max_ticks: u32 = args.last().and_then(|s| s.parse().ok()).unwrap_or(7200);

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

    let match_type: u8 = std::env::var("PVP_PROBE_MODE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let config = tango_backend_mgba::PrimeConfig {
        match_type: (match_type, 0),
        rng_seed: *b"bcc-probe-seed!!",
        // PVP_PROBE_DISABLE_BGM: exercise the primer's battle-BGM skip.
        disable_bgm: std::env::var("PVP_PROBE_DISABLE_BGM").is_ok(),
    };
    let events_sink = tango_match::telemetry::EventSink::new();
    let primed = [tango_backend_mgba::PrimedLatch::new(), tango_backend_mgba::PrimedLatch::new()];
    // PVP_PROBE_TRAP=<hex>[,<hex>]: extra printing traps, for checking
    // that a hook site is reached and when.
    let extra: Vec<u32> = std::env::var("PVP_PROBE_TRAP")
        .map(|v| {
            v.split(',')
                .map(|a| u32::from_str_radix(a.trim().trim_start_matches("0x"), 16).unwrap())
                .collect()
        })
        .unwrap_or_default();
    for (i, pvp) in [pvp0, pvp1].into_iter().enumerate() {
        let mut traps = pvp.primer_traps(&config, i, &events_sink, &primed[i]);
        for &addr in &extra {
            traps.push((
                addr,
                Box::new(move |_: &mut mgba::core::Core| println!("  core{i} hit {addr:08x}")),
            ));
        }
        pair.set_traps(i, traps);
    }

    // Drive the engine's own telemetry observer, so the round and match
    // events this reports are the ones a session would see.
    let (mut telemetry, store) =
        tango_match::telemetry::Telemetry::new([pvp0.core_poller(0), pvp1.core_poller(1)], events_sink.clone());
    let mut seen_events = 0usize;

    // PVP_PROBE_EWRAM_AT=<tick>:<core>:<file>[,...]: dump a core's EWRAM at
    // those ticks, for diffing what a match end writes.
    let ewram_at: Vec<(u32, usize, String)> = std::env::var("PVP_PROBE_EWRAM_AT")
        .map(|v| {
            v.split(',')
                .map(|e| {
                    let mut it = e.split(':');
                    let t = it.next().unwrap().parse().unwrap();
                    let c = it.next().unwrap().parse().unwrap();
                    (t, c, it.next().unwrap().to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    let dump_dir = std::env::var("PVP_PROBE_DUMP_DIR").ok();
    // PVP_PROBE_PLAY: once both games are primed, tap A on both sides so
    // the battle's per-turn chip picks actually get made and the match
    // can run to its end.
    let play = std::env::var("PVP_PROBE_PLAY").is_ok();
    let mut prev = [[0u8; 8]; 2];
    let mut primed_at = [None::<u32>; 2];
    for t in 0..max_ticks {
        let keys = if play && primed.iter().all(|p| p.is_set()) && (t / 8) % 2 == 0 {
            [1, 1]
        } else {
            [0, 0]
        };
        pair.tick(&keys);

        {
                        tango_backend_mgba::analysis::observe_pair(&mut telemetry, &mut pair, t);
            let st = store.lock().unwrap();
            for (tick, ev) in st.events().iter().skip(seen_events) {
                println!("[{tick:5}] {ev:?}");
            }
            seen_events = st.events().len();
            // PVP_PROBE_OBS=<n>: print every nth battle observation, so
            // a poller change can be eyeballed against the screen.
            if let Some(every) = std::env::var("PVP_PROBE_OBS").ok().and_then(|v| v.parse::<u32>().ok()) {
                if t % every == 0 {
                    if let Some((tick, obs)) = st.samples().last() {
                        println!("[{tick:5}] obs {obs:?}");
                    }
                }
            }
        }
        for i in 0..2 {
            if primed_at[i].is_none() && primed[i].is_set() {
                primed_at[i] = Some(t);
                println!("[{t:5}] core{i} PRIMED");
            }
        }

        let now = [
            pvp0.debug_menu_state(pair.core_mut(0)),
            pvp1.debug_menu_state(pair.core_mut(1)),
        ];
        for i in 0..2 {
            if now[i] != prev[i] {
                println!("[{t:5}] core{i} {:02x?} -> {:02x?}", prev[i], now[i]);
                prev[i] = now[i];
            }
        }
        for (at, core, path) in &ewram_at {
            if *at == t {
                let mut buf = vec![0u8; 0x40000];
                pair.core_mut(*core).raw_read_range(0x02000000, -1, &mut buf);
                std::fs::write(path, buf).unwrap();
            }
        }
        if let Some(dir) = &dump_dir {
            // PVP_PROBE_SHOT_EVERY overrides the coarse default when
            // watching a short window like the battle entry.
            let every: u32 = std::env::var("PVP_PROBE_SHOT_EVERY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(600);
            if t % every == 0 {
                dump_screens(&mut pair, dir, &format!("t{t:05}"));
            }
        }
    }

    // PVP_PROBE_STATS: fold the collected telemetry the way a session
    // does and print each round's shape, so the analysis side can be
    // checked against what the poller reported.
    if std::env::var("PVP_PROBE_STATS").is_ok() {
        let mut builder = tango_backend_mgba::analysis::StatsBuilder::new();
        let (samples, events) = store.lock().unwrap().drain_confirmed(max_ticks);
        tango_backend_mgba::analysis::fold_confirmed(&mut builder, 0, samples, events);
        // The stats are flat series over the whole match with the
        // rounds marking them up, so each round's share is whatever
        // falls inside its span.
        let stats = builder.finish();
        for i in 0..stats.rounds.len() {
            let Some((start, end)) = stats.round_span(i, None) else { continue };
            let within = |t: u32| t >= start && t < end;
            println!(
                "round {i}: {start}..{end} outcome={:?} hp_points={} custom_spans={} chip_uses={:?}",
                stats.rounds[i].outcome,
                stats.hp.iter().filter(|p| within(p.tick)).count(),
                stats.custom.iter().filter(|&&(a, _)| within(a)).count(),
                stats
                    .chip_uses
                    .iter()
                    .map(|v| v.iter().filter(|&&(t, _)| within(t)).count())
                    .collect::<Vec<_>>(),
            );
            for side in 0..2 {
                let head: Vec<_> = stats.chip_uses[side].iter().filter(|&&(t, _)| within(t)).take(6).collect();
                println!("   side{side} first uses: {head:?}");
            }
        }
    }

    if let Some(dir) = &dump_dir {
        dump_screens(&mut pair, dir, "final");
    }
    println!(
        "end after {max_ticks} ticks: core0={:02x?} core1={:02x?} primed={:?}",
        pvp0.debug_menu_state(pair.core_mut(0)),
        pvp1.debug_menu_state(pair.core_mut(1)),
        primed.iter().map(|p| p.is_set()).collect::<Vec<_>>(),
    );
}
