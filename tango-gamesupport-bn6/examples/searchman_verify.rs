//! Verify the BN6 SearchMan patch in a live battle.
//!
//! Grafts the patched SerchMan chip's attack fields onto every standard
//! chip (so whatever the folder offers summons the ported actor), boots a
//! primed lockstep netbattle, and checks that
//!   - the replaced update routine and each of its states actually run,
//!   - the Scope Gun shot object gets spawned,
//!   - the battle tick keeps advancing afterwards (i.e. it didn't crash).
//!
//! Usage: searchman_verify <patched-rom> <save> [ticks]

use std::sync::{Arc, Mutex};

use tango_backend_mgba::GameSupport as _;
use tango_gamesupport_bn6::pvp;

const CHIP_DATA: u32 = 0x08021da8; // BR5E
const CHIP_STRIDE: usize = 0x2c;
const SERCHMAN: usize = 263;
const ATTACK_SPAN: std::ops::Range<usize> = 0x04..0x20;

// The patched routine, from searchman.asm.
const SITES: &[(u32, &str)] = &[
    // Diagnostics first, so a run that never used a chip is distinguishable
    // from one where the chip ran but the actor misbehaved.
    (0x08021aa4, "chip_lookup"), // raw chip record accessor
    (0x080126e4, "chip_use"),   // BN6 chip_setup_attack (BN5 0x0800fe72)
    (0x080bbed0, "navi_spawn"), // navi table [0x0e] -> allocs class 0x0e
    // The patched routine, from searchman.asm (addresses from searchman.sym).
    (0x080bbbd4, "update"),
    (0x080bbbf4, "init"),
    (0x080bbcca, "wait"),
    (0x080bbcf0, "fire"),
    (0x087fe36c, "xh_update"),
    (0x087fe398, "xh_init"),
    (0x087fe478, "xh_step"),
    (0x087fe562, "xh_locked"),
    (0x087fe792, "sd_spawn"),
    // The HolyDream port (SEARCHMAN_GRAFT_ID=302), from searchman.sym.
    (0x080eb45c, "hd_spawn_stub"), // BigHook's live navi-table stub
    (0x087fe848, "hd_init"),
    (0x087fe8d8, "hd_run"),
    (0x087fe946, "hd_end"),
    (0x087fe9be, "hd_orb_spawn"),
    (0x087fea80, "hd_orb_init"),
    (0x087feb30, "hd_orb_fly"),
];

fn main() {
    env_logger::init();
    mgba::log::install_default_logger();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut rom = std::fs::read(&args[0]).expect("rom unreadable");
    let save = std::fs::read(&args[1]).expect("save unreadable");
    let max_ticks: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(24000);
    assert_eq!(&rom[0xac..0xb0], b"BR5E", "expected a BR5E rom");

    let base = (CHIP_DATA & 0x01ff_ffff) as usize;
    let src: Vec<u8> = rom[base + SERCHMAN * CHIP_STRIDE..][ATTACK_SPAN].to_vec();
    println!(
        "grafting SerchMan attack (family {:#04x} sub {:#04x}) onto standard chips",
        src[0x0b - 0x04],
        src[0x0c - 0x04]
    );
    // SEARCHMAN_NOGRAFT=1 with a save whose folder already leads with
    // SerchMan: exercises the real chip rather than a stand-in.
    // SEARCHMAN_GRAFT_ID=<id>: graft a different chip's attack instead —
    // for tracing how a NATIVE BN6 chip (e.g. 236 EraseMan) does something.
    let graft_id: usize = std::env::var("SEARCHMAN_GRAFT_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(SERCHMAN);
    let src: Vec<u8> = rom[base + graft_id * CHIP_STRIDE..][ATTACK_SPAN].to_vec();
    if std::env::var("SEARCHMAN_NOGRAFT").is_err() {
        for id in 1..0x200usize {
            let rec = base + id * CHIP_STRIDE;
            // Standard chips only — except when tracing a foreign chip, in
            // which case the folder's lead SerchMan/CircusMan slots get it
            // too, so the deterministic first pick actually summons it.
            let force = graft_id != SERCHMAN && (263..=265).contains(&id);
            if (id >= 0x100 || rom[rec + 0x07] != 0) && !force {
                continue;
            }
            rom[rec + ATTACK_SPAN.start..rec + ATTACK_SPAN.end].copy_from_slice(&src);
            // Keep each chip's own class byte: a giga donor's class 2 on
            // the whole folder makes battle-start validation cull the hand.
            if !force {
                rom[rec + 0x07] = 0;
            }
        }
    } else {
        println!("(no graft: using the folder's own chips)");
    }

    let pvp = &pvp::PVP_BR5E_00;
    let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
        sides: vec![
            mgba_rollback::SideOptions { rom: rom.clone(), save: Some(save.clone()) },
            mgba_rollback::SideOptions { rom, save: Some(save) },
        ],
        rtc: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000)),
        peripheral: mgba_rollback::Peripheral::Cable,
    })
    .unwrap();

    let config = tango_backend_mgba::PrimeConfig {
        match_type: (0, 0),
        rng_seed: *b"sio-probe-seed!!",
        disable_bgm: true,
    };
    let events_sink = tango_match::telemetry::EventSink::new();
    let primed = [tango_backend_mgba::PrimedLatch::new(), tango_backend_mgba::PrimedLatch::new()];

    let hits: Arc<Mutex<Vec<(u32, u32)>>> = Arc::new(Mutex::new(vec![(0, 0); SITES.len()]));
    let tick = Arc::new(Mutex::new(0u32));
    let audiodump = std::env::var("SEARCHMAN_AUDIODUMP").ok();
    let sfx_log: Arc<Mutex<Vec<(u32, u32)>>> = Arc::new(Mutex::new(Vec::new()));
    // The actor object pointer, captured from r5 on the first update run
    // (the engine's object loop keeps the current object there).
    let actor: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    // Ditto the HolyDream orb, captured at hd_orb_init.
    let orb: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));

    for ci in 0..2 {
        let mut traps = pvp.primer_traps(&config, ci, &events_sink, &primed[ci]);
        if ci == 0 {
            for (n, &(addr, _)) in SITES.iter().enumerate() {
                let (hits, tick) = (hits.clone(), tick.clone());
                let actor = actor.clone();
                let orb = orb.clone();
                traps.push((
                    addr,
                    Box::new(move |c: &mut mgba::core::Core| {
                        let t = *tick.lock().unwrap();
                        let mut h = hits.lock().unwrap();
                        h[n].0 += 1;
                        if h[n].1 == 0 {
                            h[n].1 = t;
                        }
                        // The actor object pointer comes off whichever
                        // ported actor this run summons (mutually
                        // exclusive per graft).
                        if n == 3 || SITES[n].1 == "hd_init" {
                            let mut a = actor.lock().unwrap();
                            if *a == 0 {
                                *a = c.gba().cpu().gpr(5) as u32;
                            }
                        }
                        if SITES[n].1 == "hd_orb_init" {
                            *orb.lock().unwrap() = c.gba().cpu().gpr(5) as u32;
                        }
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // SEARCHMAN_AUDIODUMP: trap the sfx entry to log every sound
            // request (id, tick), matched later against the PCM dump.
            if audiodump.is_some() {
                let (tick, sfx_log) = (tick.clone(), sfx_log.clone());
                traps.push((
                    0x080005cc,
                    Box::new(move |c: &mut mgba::core::Core| {
                        let t = *tick.lock().unwrap();
                        let id = c.gba().cpu().gpr(0) as u32;
                        if id == 0x6b {
                            println!(
                                "[{t:5}] sfx 0x6b from {:08x}",
                                c.gba().cpu().gpr(14) as u32 & !1
                            );
                        }
                        sfx_log.lock().unwrap().push((id, t));
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // SEARCHMAN_ALLOCLOG=1: log every object allocation (class id +
            // call site). Finding which entity a native chip spawns for its
            // hits is a matter of reading this off the timeline.
            if std::env::var("SEARCHMAN_ALLOCLOG").is_ok() {
                for alloc in [0x08003320u32, 0x08003358] {
                    let tick = tick.clone();
                    let count = Arc::new(Mutex::new(0u32));
                    traps.push((
                        alloc,
                        Box::new(move |c: &mut mgba::core::Core| {
                            let t = *tick.lock().unwrap();
                            if t < 500 {
                                return; // skip the battle-intro storm
                            }
                            let mut n = count.lock().unwrap();
                            *n += 1;
                            if *n > 200 {
                                return;
                            }
                            let cls = c.gba().cpu().gpr(0) as u32;
                            let lr = c.gba().cpu().gpr(14) as u32 & !1;
                            println!("[{t:5}] ALLOC {alloc:08x} class={cls:#04x} from {lr:08x}");
                        }) as Box<dyn Fn(&mut mgba::core::Core)>,
                    ));
                }
            }
            // SEARCHMAN_WTYPELOG=1: trap the real panel write-type body
            // (IWRAM 0x030079a0, thunked at ROM 0x0800cc0c) and log every
            // (x, y, type) it is asked to write.
            if std::env::var("SEARCHMAN_WTYPELOG").is_ok() {
                let tick = tick.clone();
                traps.push((
                    0x030079a0,
                    Box::new(move |c: &mut mgba::core::Core| {
                        let t = *tick.lock().unwrap();
                        let g = |n| c.gba().cpu().gpr(n) as u32;
                        println!(
                            "[{t:5}] WTYPE x={} y={} type={:#x} from {:08x}",
                            g(0), g(1), g(2), g(14) & !1
                        );
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // SEARCHMAN_HITLOG=1: log calls into the class-0xc3 hit-spark
            // spawner (the native BN6 targeted-hit path EraseMan uses).
            if std::env::var("SEARCHMAN_HITLOG").is_ok() {
                let tick = tick.clone();
                traps.push((
                    0x080df2ca,
                    Box::new(move |c: &mut mgba::core::Core| {
                        let t = *tick.lock().unwrap();
                        let g = |n| c.gba().cpu().gpr(n) as u32;
                        println!(
                            "[{t:5}] HIT0xc3 x={} y={} r2={:#x} dmg={:08x} creator={:08x}",
                            g(0), g(1), g(2), g(6), g(5)
                        );
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // SEARCHMAN_FXLOG=1: log calls into the class-0x09 gun-effect
            // spawner (MachGun's reticle/hit visual).
            if std::env::var("SEARCHMAN_FXLOG").is_ok() {
                let tick = tick.clone();
                traps.push((
                    0x080c779c,
                    Box::new(move |c: &mut mgba::core::Core| {
                        let t = *tick.lock().unwrap();
                        let g = |n| c.gba().cpu().gpr(n) as u32;
                        println!(
                            "[{t:5}] FX0x09 x={} y={} kind={:#x} dmg={:08x} creator={:08x}",
                            g(0), g(1), g(2), g(6), g(5)
                        );
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // SEARCHMAN_INSERTLOG=1: log successful allocations at the
            // post-alloc list insert (r5 = the new object).
            if std::env::var("SEARCHMAN_INSERTLOG").is_ok() {
                let tick = tick.clone();
                traps.push((
                    0x08003400,
                    Box::new(move |c: &mut mgba::core::Core| {
                        let t = *tick.lock().unwrap();
                        if !(660..700).contains(&t) {
                            return;
                        }
                        let r5 = c.gba().cpu().gpr(5) as u32;
                        let mut f = [0u8; 8];
                        c.raw_read_range(r5, -1, &mut f);
                        println!("[{t:5}] INSERT obj={r5:08x} bytes={f:02x?}");
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // SEARCHMAN_SHOTLOG=1: watch the class-0x49 shot object itself.
            // Its update dispatches first on params[0] via obj+0x58 and then
            // on its own state bytes; logging both is how a shot that spawns
            // but never damages gets diagnosed.
            if std::env::var("SEARCHMAN_SHOTLOG").is_ok() {
                let tick = tick.clone();
                let count = Arc::new(Mutex::new(0u32));
                traps.push((
                    0x080b9a5c,
                    Box::new(move |c: &mut mgba::core::Core| {
                        let mut n = count.lock().unwrap();
                        *n += 1;
                        if *n > 60 {
                            return;
                        }
                        let t = *tick.lock().unwrap();
                        let r5 = c.gba().cpu().gpr(5) as u32;
                        let mut f = [0u8; 0x70];
                        c.raw_read_range(r5, -1, &mut f);
                        let p58 = u32::from_le_bytes(f[0x58..0x5c].try_into().unwrap());
                        let mut pb = [0u8; 16];
                        if (0x0200_0000..0x0204_0000).contains(&p58) {
                            c.raw_read_range(p58, -1, &mut pb);
                        }
                        println!(
                            "[{t:5}] SHOT49 obj={r5:08x} flags={:02x} st={:02x}/{:02x}/{:02x} obj4={:02x} pos={},{} own={:02x} dmg={:08x} p58={p58:08x} params={pb:02x?}",
                            f[0], f[8], f[9], f[0xa], f[4], f[0x12], f[0x13], f[0x16],
                            u32::from_le_bytes(f[0x2c..0x30].try_into().unwrap()),
                        );
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
        }
        pair.set_traps(ci, traps);
    }

    let mut pollers = [pvp.core_poller(0), pvp.core_poller(1)];
    let mut primed_at: Option<u32> = None;
    let mut last_tick = 0u32;
    let mut stalled_since: Option<u32> = None;
    // Tick-advance is NOT a freeze test: a hung chip animation can leave the
    // battle counter running. Hash the picture instead and flag a stretch
    // where nothing on screen changes at all.
    let mut last_frame: u64 = 0;
    let mut frame_same: u32 = 0;
    let mut worst_freeze: (u32, u32) = (0, 0);

    const A: u32 = 0x001;
    const RIGHT: u32 = 0x010;
    // Read off a frame dump of the custom screen: the cursor starts on the
    // first chip and the OK button sits at the right-hand end of that same
    // row. So: take the chip, walk right onto OK, confirm. Anything less
    // deterministic (random masks, one step per direction) never confirms,
    // which is why earlier runs saw no chip use at all -- on the stock ROM
    // as well as the patched one.
    const START: u32 = 0x008;
    // A to take the chip, START then A to leave the custom screen.
    // Do NOT press DOWN first: that moves onto the Beast Out option, and
    // selecting it makes every subsequent measurement about beast-out
    // rather than the chip -- which invalidated several earlier runs.
    // 24 phases: side A picks in the first half, side B (offset +12) in
    // the second, so BOTH hold a chip; A's wrap-around press fires first
    // and the post-summon quiet keeps B holding -- the chip-delete target.
    let _ = RIGHT;
    const SEQ: [u32; 24] = [
        A, 0, 0, START, 0, 0, A, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    // The reticle is USER-stopped: once the summon is out, the scripted
    // custom-screen sequence must go quiet (its stray A presses would stop
    // the cursor at random tiles) and the probe instead presses A exactly
    // when the cursor crosses the enemy's tile -- the timing a real player
    // would aim for.
    let mut snipe_press = 0u32;
    let mut snipe_cooldown = 0u32;

    // SEARCHMAN_AUDIODUMP=<prefix>: drain core 0's audio every tick into
    // <prefix>.pcm (i16 interleaved) with <prefix>.idx mapping ticks to
    // sample offsets, and log every sfx request (id, tick) via a trap on
    // the sfx entry -- the raw material for cross-correlating the ported
    // sound ids against BN5's renders of the same moments.
    let mut audio_pcm: Vec<i16> = Vec::new();
    let mut audio_idx: Vec<(u32, usize)> = Vec::new();
    if audiodump.is_some() {
        println!("audio: sample rate {}", pair.core_mut(0).audio_sample_rate());
    }

    for t in 0..max_ticks {
        *tick.lock().unwrap() = t;
        let mut keys = [0u32; 2];
        // Quiet from the first chip USE (not the spawn -- the announce takes
        // ~100 ticks and the defender's wrap-around press must not slip in).
        let summoned = hits.lock().unwrap()[1].1 != 0;
        if let (Some(p0), false) = (primed_at, summoned) {
            let rel = t - p0;
            let phase = (rel / 6) as usize;
            let held = |p: usize| if rel % 6 < 3 { SEQ[p % SEQ.len()] } else { 0 };
            keys = [held(phase), held(phase + 12)];
        }
        if summoned {
            let c = pair.core_mut(0);
            let mut units = [(0u8, 0u8, 0u8); 2];
            for (s, u) in units.iter_mut().enumerate() {
                let base = 0x0203a9b0 + s as u32 * 0xd8;
                *u = (
                    c.raw_read_8(base + 0x16, -1),
                    c.raw_read_8(base + 0x12, -1),
                    c.raw_read_8(base + 0x13, -1),
                );
            }
            if snipe_cooldown == 0 {
                for k in 0..12u32 {
                    let base = 0x0203cfe0 + k * 0xd8;
                    if c.raw_read_8(base + 1, -1) != 0xd1 || c.raw_read_8(base, -1) == 0 {
                        continue;
                    }
                    let own = c.raw_read_8(base + 0x16, -1);
                    let (cx, cy) = (c.raw_read_8(base + 0x12, -1), c.raw_read_8(base + 0x13, -1));
                    // SEARCHMAN_SNIPE_EARLY=1: stop the cursor on first
                    // sight instead -- a deliberate miss, for verifying
                    // the volley fires at an empty tile with no effects.
                    let early = std::env::var("SEARCHMAN_SNIPE_EARLY").is_ok();
                    if early || units.iter().any(|&(o, x, y)| o != own && x == cx && y == cy) {
                        snipe_press = 3;
                        snipe_cooldown = 60;
                        println!(
                            "[{t:5}] SNIPE: cursor at {cx},{cy}{} -- pressing A",
                            if early { " (early miss)" } else { " (on the enemy)" }
                        );
                    }
                }
            }
            // SEARCHMAN_MOVETEST=1: hammer UP on both sides from the summon
            // onward. The units-on-change log then shows whether movement is
            // frozen during the summon (pause parity with BN5) and resumes
            // after it ends.
            if std::env::var("SEARCHMAN_MOVETEST").is_ok() && (t / 6) % 2 == 0 {
                keys[0] |= 0x040;
                keys[1] |= 0x040;
            }
            snipe_cooldown = snipe_cooldown.saturating_sub(1);
            if snipe_press > 0 {
                snipe_press -= 1;
                // A+B arms the chip-delete (BN5's rule); SEARCHMAN_PRESS_A_ONLY=1
                // is the control: stop the cursor without arming.
                let mask = if std::env::var("SEARCHMAN_PRESS_A_ONLY").is_ok() { 0x001 } else { 0x003 };
                keys = [mask, mask];
            }
        }
        pair.tick(&keys);

        if audiodump.is_some() {
            audio_idx.push((t, audio_pcm.len()));
            let mut ab = pair.core_mut(0).audio_buffer();
            let n = ab.available();
            if n > 0 {
                let start = audio_pcm.len();
                audio_pcm.resize(start + n * 2, 0);
                let got = ab.read(&mut audio_pcm[start..], n);
                audio_pcm.truncate(start + got * 2);
            }
        }

        // SEARCHMAN_KOTEST=1: at the lock, drop the defender to 50 HP so
        // the volley's third shot KOs them mid-summon -- the probe then
        // watches whether the round ends cleanly and whether the hidden
        // summoner comes back (the unhide normally lives in sm_leave,
        // which a round-end teardown might skip).
        if std::env::var("SEARCHMAN_KOTEST").is_ok() {
            static KO_ARMED: Mutex<bool> = Mutex::new(false);
            let mut armed = KO_ARMED.lock().unwrap();
            if !*armed && hits.lock().unwrap()[10].1 != 0 {
                *armed = true;
                for ci in 0..2 {
                    let c = pair.core_mut(ci);
                    for s in 0..2u32 {
                        let base = 0x0203a9b0 + s * 0xd8;
                        if c.raw_read_8(base + 0x16, -1) == 0 {
                            c.raw_write_16(base + 0x24, -1, 50);
                        }
                    }
                }
                println!("[{t:5}] KOTEST: defender set to 50 HP");
            }
            if *armed && t % 25 == 0 {
                let c = pair.core_mut(0);
                let mut line = format!("[{t:5}] KOTEST:");
                for s in 0..2u32 {
                    let base = 0x0203a9b0 + s * 0xd8;
                    line += &format!(
                        " u{}(own{} fl={:02x} hp={})",
                        s,
                        c.raw_read_8(base + 0x16, -1),
                        c.raw_read_8(base, -1),
                        c.raw_read_16(base + 0x24, -1)
                    );
                }
                println!("{line}");
            }
        }

        if primed_at.is_none() && pvp.debug_battle_state(pair.core_mut(0)) != [0; 8] {
            primed_at = Some(t);
        }
        let _ = pollers[0].poll(pair.core_mut(0), &events_sink, 0);
        let _ = pollers[1].poll(pair.core_mut(1), &events_sink, 0);

        // SEARCHMAN_SHOTS=<dir>: dump frames so the custom screen can be
        // read directly instead of guessed at.
        if let (Ok(dir), Some(p)) = (std::env::var("SEARCHMAN_SHOTS"), primed_at) {
            let since = t - p;
            let w: u32 = std::env::var("SEARCHMAN_SHOT_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(120);
            if t % w == 0 && since > 0 {
                if let Some(buf) = pair.video_buffer(0) {
                    let img = image::RgbImage::from_fn(240, 160, |x, y| {
                        let off = ((y * 240 + x) * 2) as usize;
                        let v = u16::from_le_bytes([buf[off], buf[off + 1]]);
                        image::Rgb([
                            ((v & 31) as u8) << 3,
                            (((v >> 5) & 31) as u8) << 3,
                            (((v >> 10) & 31) as u8) << 3,
                        ])
                    });
                    let _ = img.save(format!("{dir}/f{:02}_t{t}.png", since / 120));
                }
            }
        }

        // SEARCHMAN_POOLSCAN=<tick>: dump every live object-pool slot at
        // one tick (class, flags, states, pos), for "did my object even
        // get created" questions.
        if let Ok(at) = std::env::var("SEARCHMAN_POOLSCAN") {
            if at.split(',').any(|s| s.parse() == Ok(t)) {
                let c = pair.core_mut(0);
                for i in 0..48u32 {
                    let base = 0x0203a9b0 + i * 0xd8;
                    let mut f = [0u8; 0x18];
                    c.raw_read_range(base, -1, &mut f);
                    if f[0] != 0 {
                        println!(
                            "[{t:5}] pool[{i:2}] @{base:08x} flags={:02x} cls={:02x} st={:02x}/{:02x}/{:02x} anim={:02x}/{:02x} pos={},{} owner={:02x}",
                            f[0], f[1], f[8], f[9], f[0xa], f[0x10], f[0x11], f[0x12], f[0x13], f[0x16]
                        );
                    }
                }
            }
        }
        // SEARCHMAN_CHIPLOG=1: both players' selected-chip blocks (fired
        // counter + id slots) on change -- how the ported hit's
        // chip-delete status is actually verified.
        if std::env::var("SEARCHMAN_CHIPLOG").is_ok() {
            let c = pair.core_mut(0);
            let mut b = [[0u16; 4]; 2];
            for p in 0..2u32 {
                for w in 0..4u32 {
                    b[p as usize][w as usize] = c.raw_read_16(0x020349c0 + p * 0x50 + w * 2, -1);
                }
            }
            static CLAST: Mutex<Option<[[u16; 4]; 2]>> = Mutex::new(None);
            let mut cl = CLAST.lock().unwrap();
            if cl.map_or(true, |l| l != b) && t > 540 {
                println!(
                    "[{t:5}] chips: p0 fired={} ids={:04x?} | p1 fired={} ids={:04x?}",
                    b[0][0], &b[0][1..], b[1][0], &b[1][1..]
                );
                *cl = Some(b);
            }
        }
        // SEARCHMAN_PARAMLOG=1: each unit's params-block ptr (+0x58) and
        // the halfword at params+0x2c -- the field BN5's cursor polls for
        // its stop-the-reticle button press. Logged on change, with the
        // keys the probe is feeding, to identify the BN6 input bits.
        if std::env::var("SEARCHMAN_PARAMLOG").is_ok() {
            let c = pair.core_mut(0);
            let mut vals = [0u32; 4];
            for s in 0..2u32 {
                let p = c.raw_read_32(0x0203a9b0 + s * 0xd8 + 0x58, -1);
                vals[s as usize * 2] = p;
                if (0x0200_0000..0x0204_0000).contains(&p) {
                    vals[s as usize * 2 + 1] = c.raw_read_16(p + 0x2c, -1) as u32;
                }
            }
            static PLAST: Mutex<Option<[u32; 4]>> = Mutex::new(None);
            let mut pl = PLAST.lock().unwrap();
            if pl.map_or(true, |l| l != vals) && t > 600 {
                println!(
                    "[{t:5}] params: u0 p={:08x} f={:04x} | u1 p={:08x} f={:04x} keys={:03x}/{:03x}",
                    vals[0], vals[1], vals[2], vals[3], keys[0], keys[1]
                );
                *pl = Some(vals);
            }
        }
        // SEARCHMAN_ACTORLOG=1: once the actor exists, log its state and
        // animation fields every tick they change. obj+0 bit 1 is the
        // visible bit; +0x10/+0x11 select the animation the engine's
        // per-frame tail plays. Also logs both units' tile+HP on change,
        // which is how shot damage is actually verified.
        if std::env::var("SEARCHMAN_ACTORLOG").is_ok() {
            let c = pair.core_mut(0);
            let mut u = [[0u8; 0x28]; 2];
            for (s, buf) in u.iter_mut().enumerate() {
                c.raw_read_range(0x0203a9b0 + s as u32 * 0xd8, -1, buf);
            }
            let brief: Vec<(u8, u8, u8, u16)> = u
                .iter()
                .map(|b| (b[0x16], b[0x12], b[0x13], u16::from_le_bytes([b[0x24], b[0x25]])))
                .collect();
            static ULAST: Mutex<Option<Vec<(u8, u8, u8, u16)>>> = Mutex::new(None);
            let mut ul = ULAST.lock().unwrap();
            if ul.as_ref() != Some(&brief) {
                println!(
                    "[{t:5}] units: owner{} @{},{} hp={} | owner{} @{},{} hp={}",
                    brief[0].0, brief[0].1, brief[0].2, brief[0].3,
                    brief[1].0, brief[1].1, brief[1].2, brief[1].3,
                );
                *ul = Some(brief);
            }
        }
        if std::env::var("SEARCHMAN_ACTORLOG").is_ok() {
            let a = *actor.lock().unwrap();
            if a != 0 {
                let c = pair.core_mut(0);
                let mut f = [0u8; 0x40];
                c.raw_read_range(a, -1, &mut f);
                static LAST: Mutex<Option<[u8; 0x40]>> = Mutex::new(None);
                let mut last = LAST.lock().unwrap();
                if last.map_or(true, |l| l != f) {
                    println!(
                        "[{t:5}] flags={:02x} cls={:02x} st={:02x}/{:02x}/{:02x} anim={:02x}/{:02x} pos={},{} dmg={} t22={:04x}",
                        f[0], f[1], f[8], f[9], f[0xa], f[0x10], f[0x11], f[0x12], f[0x13],
                        u32::from_le_bytes([f[0x2c], f[0x2d], f[0x2e], f[0x2f]]),
                        u16::from_le_bytes([f[0x22], f[0x23]]),
                    );
                    *last = Some(f);
                }
            }
        }

        // SEARCHMAN_ORBLOG=1: dump the HolyDream orb's motion fields every
        // tick it exists — pixel X/Y (16.16 at +0x34/+0x38), velocity,
        // state, the arming timer and its hit slot's connect flag.
        if std::env::var("SEARCHMAN_ORBLOG").is_ok() {
            let o = *orb.lock().unwrap();
            if o != 0 {
                let c = pair.core_mut(0);
                let mut f = [0u8; 0x78];
                c.raw_read_range(o, -1, &mut f);
                let w = |i: usize| u32::from_le_bytes([f[i], f[i + 1], f[i + 2], f[i + 3]]);
                if f[1] == 0x8d {
                    let slot = w(0x54);
                    let conn = if slot != 0 { c.raw_read_8(slot + 0x70, -1) } else { 0xff };
                    println!(
                        "[{t:5}] orb: flags={:02x} st={:02x} x={:08x} y={:08x} v={:08x} t20={:04x} slot={:08x} conn={:02x} tile={},{}",
                        f[0], f[8], w(0x34), w(0x38), w(0x40),
                        u16::from_le_bytes([f[0x20], f[0x21]]), slot, conn, f[0x12], f[0x13],
                    );
                    if slot != 0 {
                        let mut s = [0u8; 0x30];
                        c.raw_read_range(slot, -1, &mut s);
                        println!("[{t:5}] slot: {}", s.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
                    }
                } else {
                    *orb.lock().unwrap() = 0;
                }
            }
        }
        // SEARCHMAN_TYPETEST=1: at tick 560, poke candidate panel type
        // ids across the field — row 1 gets types 3..8 by column, row 3
        // gets 9..0xe — then read the SHOTS frames to identify which id
        // renders as holy (and which as cracked) in BN6.
        if std::env::var("SEARCHMAN_TYPETEST").is_ok() && t == 560 {
            for ci in 0..2 {
                let c = pair.core_mut(ci);
                for x in 1..=6u32 {
                    // +6 is the displayed-type cache; 0xff forces redraw
                    c.raw_write_8(0x02039ae0 + (8 + x) * 0x20 + 2, -1, (x + 2) as u8);
                    c.raw_write_8(0x02039ae0 + (8 + x) * 0x20 + 6, -1, 0xff);
                    c.raw_write_8(0x02039ae0 + (24 + x) * 0x20 + 2, -1, (x - 1) as u8);
                    c.raw_write_8(0x02039ae0 + (24 + x) * 0x20 + 6, -1, 0xff);
                }
            }
            println!("[{t:5}] TYPETEST: row1 = types 3-8, row3 = types 0-5");
        }
        // SEARCHMAN_HOLYPREP=<n>: set n of the right side's panels
        // (columns 4-6, row-major) to holy (type 9) on BOTH cores — the
        // BN6 analog of bn5's ST_HOLYPREP, for the HolyDream parity
        // matrix (N=0/3/6/9 -> 1/4/7/10 orbs in BN5; BN6 holy = type 5,
        // not BN5's 9). Default tick 560:
        // after the field settles (~160) but before the graft's first
        // chip use summons the actor (~600). SEARCHMAN_HOLYPREP_AT
        // overrides. NOTE: BN6 keeps the panel TYPE at +3, not BN5's +2.
        if let Ok(n) = std::env::var("SEARCHMAN_HOLYPREP") {
            let at: u32 = std::env::var("SEARCHMAN_HOLYPREP_AT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(560);
            if t == at {
                if let Ok(n) = n.parse::<u32>() {
                    // SEARCHMAN_HOLYPREP_LEFT=1: columns 1-3 instead (the
                    // summoner's ENEMY side) to test scan coverage.
                    let cols: [u32; 3] = if std::env::var("SEARCHMAN_HOLYPREP_LEFT").is_ok() {
                        [1, 2, 3]
                    } else {
                        [4, 5, 6]
                    };
                    for ci in 0..2 {
                        let c = pair.core_mut(ci);
                        let mut left = n;
                        'outer: for y in 1..=3u32 {
                            for &x in &cols {
                                if left == 0 {
                                    break 'outer;
                                }
                                c.raw_write_8(0x02039ae0 + (y * 8 + x) * 0x20 + 2, -1, 5);
                                left -= 1;
                            }
                        }
                    }
                    println!("[{t:5}] HOLYPREP: {n} holy panels on cols {cols:?}");
                }
            }
        }
        // SEARCHMAN_PANELLOG=1: the field's 18 panel-type bytes on change
        // (BN6 panel array 0x02039ae0, stride 0x24, index y*8+x, type +3).
        if std::env::var("SEARCHMAN_PANELLOG").is_ok() {
            let c = pair.core_mut(0);
            // Both candidate type bytes, printed as fixed two-digit hex —
            // +2 is BN5's type offset, +3 is where BN6's write thunk puts
            // the new type.
            let mut grid = [[(0u8, 0u8); 6]; 3];
            for y in 1..=3u32 {
                for x in 1..=6u32 {
                    let rec = 0x02039ae0 + (y * 8 + x) * 0x20;
                    grid[y as usize - 1][x as usize - 1] =
                        (c.raw_read_8(rec + 2, -1), c.raw_read_8(rec + 3, -1));
                }
            }
            static PLAST2: Mutex<Option<[[(u8, u8); 6]; 3]>> = Mutex::new(None);
            let mut pl = PLAST2.lock().unwrap();
            if pl.as_ref() != Some(&grid) {
                for (which, sel) in [("t2", 0usize), ("t3", 1)] {
                    let rows: Vec<String> = grid
                        .iter()
                        .map(|r| {
                            r.iter()
                                .map(|v| {
                                    format!("{:02x}", if sel == 0 { v.0 } else { v.1 })
                                })
                                .collect::<Vec<_>>()
                                .join(".")
                        })
                        .collect();
                    println!("[{t:5}] panels {which}: {}", rows.join(" / "));
                }
                *pl = Some(grid);
            }
        }

        // SEARCHMAN_IWRAMDUMP=<file>: dump all of IWRAM once at tick 600
        // (the anim player and other engine hot code are IWRAM-resident).
        if let Ok(f) = std::env::var("SEARCHMAN_IWRAMDUMP") {
            if t == 600 {
                let c = pair.core_mut(0);
                let mut buf = vec![0u8; 0x8000];
                c.raw_read_range(0x0300_0000, -1, &mut buf);
                std::fs::write(&f, &buf).unwrap();
                println!("[{t:5}] IWRAM dumped to {f}");
            }
        }
        // SEARCHMAN_VRAMDUMP=<dir>: dump palette RAM, VRAM and OAM at fixed
        // ticks after the actor's update routine first runs. Diffing these
        // between the stock and --with-sprites builds separates "tiles never
        // staged" from "tiles staged wrong".
        if let Ok(dir) = std::env::var("SEARCHMAN_VRAMDUMP") {
            let first_update = hits.lock().unwrap()[3].1;
            if first_update != 0 {
                let d = t - first_update;
                if [1, 5, 12, 20, 30].contains(&d) {
                    let c = pair.core_mut(0);
                    for (name, addr, len) in [
                        ("pal", 0x0500_0000u32, 0x400usize),
                        ("vram", 0x0600_0000, 0x18000),
                        ("oam", 0x0700_0000, 0x400),
                    ] {
                        let mut buf = vec![0u8; len];
                        c.raw_read_range(addr, -1, &mut buf);
                        std::fs::write(format!("{dir}/{name}_d{d:03}.bin"), &buf).unwrap();
                    }
                }
            }
        }

        if primed_at.is_some() {
            if let Some(buf) = pair.video_buffer(0) {
                let mut h: u64 = 0xcbf29ce484222325;
                for b in buf.iter().step_by(7) {
                    h ^= *b as u64;
                    h = h.wrapping_mul(0x100000001b3);
                }
                if h == last_frame {
                    frame_same += 1;
                    if frame_same > worst_freeze.0 {
                        worst_freeze = (frame_same, t);
                    }
                } else {
                    frame_same = 0;
                    last_frame = h;
                }
            }
            let bt = pvp.debug_battle_tick(pair.core_mut(0));
            if bt == last_tick {
                stalled_since.get_or_insert(t);
                if t - stalled_since.unwrap() > 600 {
                    println!("FAIL: battle tick stuck at {bt} for 600 pair ticks (crash/hang)");
                    break;
                }
            } else {
                stalled_since = None;
                last_tick = bt;
            }
        }
    }

    if let Some(prefix) = &audiodump {
        let bytes: Vec<u8> = audio_pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::write(format!("{prefix}.pcm"), &bytes).unwrap();
        let idx: String = audio_idx.iter().map(|(t, o)| format!("{t} {o}\n")).collect();
        std::fs::write(format!("{prefix}.idx"), idx).unwrap();
        let sfx: String = sfx_log.lock().unwrap().iter().map(|(id, t)| format!("{id:#x} {t}\n")).collect();
        std::fs::write(format!("{prefix}.sfx"), sfx).unwrap();
        println!("audio dump: {} samples, {} sfx events", audio_pcm.len() / 2, sfx_log.lock().unwrap().len());
    }

    println!("\nsite hits:");
    let h = hits.lock().unwrap();
    let mut ok = true;
    for (n, (addr, name)) in SITES.iter().enumerate() {
        let (n_hits, first) = h[n];
        println!("  {addr:08x} {name:<12} hits={n_hits:<8} first@{first}");
        if n_hits == 0 {
            ok = false;
        }
    }
    println!("\nfinal battle tick: {last_tick}");
    println!("longest identical-frame run: {} ticks (ending @{})", worst_freeze.0, worst_freeze.1);
    println!("{}", if ok && last_tick > 0 { "PASS" } else { "INCOMPLETE (see above)" });
}
