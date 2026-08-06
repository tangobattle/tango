//! Headless probe: boot two BN6 games as a lockstep pair, prime both
//! through their real comm menus, and report whether the games' own SIO
//! handshake reaches a live link battle.
//!
//! Usage: pvp_probe <rom> <save> [<rom2> <save2>] [ticks]
//!
//! Exits 0 once both games report a live battle with the battle tick
//! advancing for 120 consecutive pair ticks; exits 1 on timeout, dumping
//! the last menu/battle state bytes from both cores.

use tango_gamesupport_bn6::pvp;
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

/// Decode the interesting bits of a lockstep driver blob's flags word
/// (GBASIOLockstepSerializedFlags in mgba's gba/pvp/lockstep.c):
/// this player's mode, its view of players 0/1's modes, queued event
/// count, whether the delivery event is armed, and (player-0 blob only)
/// whether a transfer is active.
fn decode_driver_flags(blob: &[u8]) -> String {
    let flags = u32::from_le_bytes(blob[4..8].try_into().unwrap());
    let mode = |v: u32| ["--", "MU", "N8", "N32", "GP", "UA", "JB", "??"][(v & 7) as usize];
    format!(
        "mode={} others=[{},{}] evq={} armed={} asleep={} xfer={}",
        mode(flags & 7),
        mode((flags >> 10) & 7),
        mode((flags >> 13) & 7),
        (flags >> 3) & 15,
        (flags >> 9) & 1,
        (flags >> 7) & 1,
        flags >> 31,
    )
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

    let config = tango_backend_mgba::PrimeConfig {
        // PVP_PROBE_MATCH_TYPE="mode,subtype"; default (0,0) = single
        // battle (the blessed prime-tick/digest baselines are recorded
        // per mode).
        match_type: std::env::var("PVP_PROBE_MATCH_TYPE")
            .ok()
            .and_then(|s| {
                let (m, sub) = s.split_once(',')?;
                Some((m.trim().parse().ok()?, sub.trim().parse().ok()?))
            })
            .unwrap_or((0, 0)),
        rng_seed: *b"sio-probe-seed!!",
        // PVP_PROBE_DISABLE_BGM: exercise the primer's battle-BGM skip.
        disable_bgm: std::env::var("PVP_PROBE_DISABLE_BGM").is_ok(),
    };
    let events_sink = tango_match::telemetry::EventSink::new();
    let primed = [tango_backend_mgba::PrimedLatch::new(), tango_backend_mgba::PrimedLatch::new()];
    pair.set_traps(0, pvp0.primer_traps(&config, 0, &events_sink, &primed[0]));
    pair.set_traps(1, pvp1.primer_traps(&config, 1, &events_sink, &primed[1]));
    let mut pollers = [pvp0.core_poller(0), pvp1.core_poller(1)];

    println!("player ids: core0={} core1={}", pair.player_id(0), pair.player_id(1));

    // PVP_PROBE_TRACE: per-tick submenu-control transition trace — the
    // raw data behind (and now verifying) the primer's state-poke table.
    let trace = std::env::var("PVP_PROBE_TRACE").is_ok();
    let mut prev_menu = [[0u8; 8]; 2];

    let mut prev_tick: Option<u32> = None;
    let mut live_streak = 0u32;
    for t in 0..max_ticks {
        let slices = pair.tick(&[0, 0]);

        if trace {
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
        }

        let obs0 = pollers[0].poll(pair.core_mut(0), &events_sink, 0);
        let obs1 = pollers[1].poll(pair.core_mut(1), &events_sink, 0);
        let advancing = match (&obs0, &obs1) {
            (Some(_), Some(_)) => {
                let tick = pvp0.debug_battle_tick(pair.core_mut(0));
                let ok = prev_tick.is_none_or(|p| tick == p + 1);
                prev_tick = Some(tick);
                ok
            }
            _ => {
                prev_tick = None;
                false
            }
        };
        live_streak = if advancing { live_streak + 1 } else { 0 };

        if live_streak == 120 {
            // Final side-by-side screens (with PVP_PROBE_DUMP_DIR set):
            // both cores mid-battle, for eyeballing stage agreement.
            if let Ok(dir) = std::env::var("PVP_PROBE_DUMP_DIR") {
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
                        img.save(format!("{dir}/success_c{i}.png")).unwrap();
                    }
                }
            }
            let a = obs0.unwrap();
            println!(
                "SUCCESS at pair tick {t}: battle live for 120 ticks, game_tick={} hp={:?} custom={} tiles={:?} digest={:08x}",
                pvp0.debug_battle_tick(pair.core_mut(0)),
                a.units.map(|u| u.hp),
                a.custom_self,
                a.units.map(|u| u.tile),
                pair.save().unwrap().digest()
            );
            std::process::exit(0);
        }

        // With PVP_PROBE_DUMP_DIR set, snapshot both screens at a few
        // interesting ticks (standby, right after the nudge, later).
        if let Ok(dir) = std::env::var("PVP_PROBE_DUMP_DIR") {
            if [120u32, 239, 260, 420, 840].contains(&t) {
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
                        img.save(format!("{dir}/probe_t{t}_c{i}.png")).unwrap();
                    }
                }
            }
        }

        if t % 60 == 0 {
            let io = |pair: &mut mgba_rollback::Link, i: usize| {
                let mut core = pair.core_mut(i);
                // NB: raw reads see memory.io[] (last game-written value),
                // not the GBASIO shadows reads are served from — SIOMULTI*
                // is genuinely io-array-backed though, so received data
                // shows up there.
                [
                    core.raw_read_16(0x0400_0120, -1), // SIOMULTI0
                    core.raw_read_16(0x0400_0122, -1), // SIOMULTI1
                    core.raw_read_16(0x0400_012a, -1), // SIOMLT_SEND
                ]
            };
            let m0 = io(&mut pair, 0);
            let m1 = io(&mut pair, 1);
            let snap = pair.save().unwrap();
            println!(
                "[{t:5}] drv0<{}> drv1<{}>",
                decode_driver_flags(snap.driver_blob(0)),
                decode_driver_flags(snap.driver_blob(1)),
            );
            println!(
                "[{t:5}] slices={slices:4} menu0={:02x?} menu1={:02x?} pc0={:08x} pc1={:08x} multi0={m0:04x?} multi1={m1:04x?} obs=({},{})",
                pvp0.debug_menu_state(pair.core_mut(0)),
                pvp1.debug_menu_state(pair.core_mut(1)),
                pair.core(0).gba().cpu().thumb_pc(),
                pair.core(1).gba().cpu().thumb_pc(),
                obs0.is_some(),
                obs1.is_some(),
            );
        }
    }

    println!(
        "TIMEOUT after {max_ticks} ticks: menu0={:02x?} menu1={:02x?} battle0={:02x?} battle1={:02x?}",
        pvp0.debug_menu_state(pair.core_mut(0)),
        pvp1.debug_menu_state(pair.core_mut(1)),
        pvp0.debug_battle_state(pair.core_mut(0)),
        pvp1.debug_battle_state(pair.core_mut(1)),
    );
    std::process::exit(1);
}
