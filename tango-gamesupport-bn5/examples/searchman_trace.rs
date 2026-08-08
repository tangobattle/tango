//! Scratch probe: find the battle routine BN5 runs for a navi chip.
//!
//! Static xrefs dead-end before the attack entity (the behavior tables at
//! 0x08003958/0x08003b80/0x08003f60 are reached through a table-of-tables
//! nothing loads by literal), so this locates it by execution instead.
//!
//! Method:
//!  - rewrite every *standard* chip's attack fields (record +0x04..+0x20)
//!    with SerchMan's (chip 230), so whichever chip the folder happens to
//!    offer spawns SearchMan's attack. Codes/MB/icons are left alone so
//!    the folder and custom screen still work.
//!  - trap every entry point in the three behavior tables and record which
//!    ones execute, with the pair tick.
//!  - trap chip_setup_attack (0x0800fe7e) and the family dispatcher
//!    (0x0800f358) to confirm the chip ids and families actually reached.
//!
//! Usage: searchman_trace <rom> <save> [ticks]
//! Env:   ST_CHIP=<id>  trace a different chip (default 230 = SerchMan)

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tango_backend_mgba::GameSupport as _;
use tango_gamesupport_bn5::pvp;

const CHIP_DATA: u32 = 0x0801e214; // BRBE
const CHIP_STRIDE: usize = 0x2c;
const CHIP_SETUP_ATTACK: u32 = 0x0800fe7e;
const DISPATCH_FAMILY: u32 = 0x0800f358;

/// Attack-relevant span of a chip record: attack_element .. dark_chip_id.
const ATTACK_SPAN: std::ops::Range<usize> = 0x04..0x20;

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"BRBE" => &pvp::PVP_BRBE_00,
        code => panic!("this probe is BRBE-only (code {:02x?})", code),
    }
}

/// Every distinct thumb entry point in the battle behavior tables.
fn behavior_entries(rom: &[u8]) -> Vec<u32> {
    let mut out = std::collections::BTreeSet::new();
    let mut a = 0x3930usize;
    while a + 4 <= 0x4200 {
        let v = u32::from_le_bytes(rom[a..a + 4].try_into().unwrap());
        if v & 1 != 0 && (0x080b0000..0x08110000).contains(&(v & !1)) {
            out.insert(v & !1);
        }
        a += 4;
    }
    out.into_iter().collect()
}

fn main() {
    env_logger::init();
    mgba::log::install_default_logger();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut rom = std::fs::read(&args[0]).expect("rom unreadable");
    let save = std::fs::read(&args[1]).expect("save unreadable");
    let max_ticks: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20000);

    let pvp = pvp_for(&rom);
    // ST_ACTORLOG=1: instead of the full behavior sweep, trap only the
    // SearchMan actor's class update (0x080c11d8), capture the object
    // pointer from r5, and log its state/animation fields per tick.
    let actorlog = std::env::var("ST_ACTORLOG").is_ok();
    // ST_ACTOR_ENTRY=<hex>: which class-update entry the actorlog r5
    // capture watches (default: SearchMan's). For HolyDream use 80bd728.
    let actor_entry = std::env::var("ST_ACTOR_ENTRY")
        .ok()
        .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0x080c11d8);
    let entries = if actorlog { vec![actor_entry] } else { behavior_entries(&rom) };
    println!("behavior entry points to trap: {}", entries.len());
    let actor: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let actor_at: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let sfx_log: Arc<Mutex<Vec<(u32, u32)>>> = Arc::new(Mutex::new(Vec::new()));

    // Graft SerchMan's attack onto every standard chip.
    let want: usize = std::env::var("ST_CHIP").ok().and_then(|s| s.parse().ok()).unwrap_or(230);
    let base = (CHIP_DATA & 0x01ff_ffff) as usize;
    let src: Vec<u8> = rom[base + want * CHIP_STRIDE..][ATTACK_SPAN].to_vec();
    let mut grafted = 0;
    for id in 1..0x100usize {
        let rec = base + id * CHIP_STRIDE;
        if rom[rec + 0x07] != 0 {
            continue; // class != standard
        }
        rom[rec + ATTACK_SPAN.start..rec + ATTACK_SPAN.end].copy_from_slice(&src);
        // Keep the STANDARD class byte by default: a giga class across the
        // folder looked like a total battle-start cull in wander mode
        // (zero uses over 12k ticks) -- though with the reactive ST_PICK
        // ladder that may have been input flakiness. ST_KEEPCLASS=1 keeps
        // the donor's class instead: giga-family attacks (HolyDream's
        // fam15 sub23) only DISPATCH through the giga use path, so a
        // class-0 graft fires chip_setup_attack and then silently no-ops.
        // ST_KEEPCLASS=<id,id,...> keeps the donor's class on JUST those
        // ids (folders legally carry one giga; a blanket giga class
        // degrades the battle) — the same selective force the bn6 probe
        // uses. ST_KEEPCLASS=all keeps it everywhere.
        let keep = std::env::var("ST_KEEPCLASS").ok();
        let keep_this = match keep.as_deref() {
            Some("all") => true,
            Some(list) => list.split(',').any(|s| s.trim().parse::<usize>() == Ok(id)),
            None => false,
        };
        if !keep_this {
            rom[rec + 0x07] = 0;
        }
        grafted += 1;
    }
    println!(
        "grafted chip {}'s attack (family {:#04x} sub {:#04x}) onto {} standard chips",
        want,
        src[0x0b - 0x04],
        src[0x0c - 0x04],
        grafted
    );

    let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
        sides: vec![
            mgba_rollback::SideOptions { rom: rom.clone(), save: Some(save.clone()) },
            mgba_rollback::SideOptions { rom: rom.clone(), save: Some(save) },
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

    // addr -> (hits, first tick seen)
    let hits: Arc<Mutex<BTreeMap<u32, (u32, u32)>>> = Arc::new(Mutex::new(BTreeMap::new()));
    let chips: Arc<Mutex<BTreeMap<u16, u32>>> = Arc::new(Mutex::new(BTreeMap::new()));
    let fams: Arc<Mutex<BTreeMap<u32, u32>>> = Arc::new(Mutex::new(BTreeMap::new()));
    let tick = Arc::new(Mutex::new(0u32));
    // Chronological (tick, text) log: chip-use calls and the first
    // execution of each behavior entry, so the entity that spawns right
    // after a chip use is readable straight off the timeline.
    let log: Arc<Mutex<Vec<(u32, String)>>> = Arc::new(Mutex::new(Vec::new()));

    for ci in 0..2 {
        let mut traps = pvp.primer_traps(&config, ci, &events_sink, &primed[ci]);
        if ci == 0 {
            for &a in &entries {
                let (hits, tick, log) = (hits.clone(), tick.clone(), log.clone());
                let actor = actor.clone();
                let actor_at2 = actor_at.clone();
                traps.push((
                    a,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let t = *tick.lock().unwrap();
                        let mut h = hits.lock().unwrap();
                        let e = h.entry(a).or_insert((0, t));
                        e.0 += 1;
                        if e.0 == 1 {
                            log.lock().unwrap().push((t, format!("  entity {a:08x} first run")));
                        }
                        if actorlog {
                            let mut ac = actor.lock().unwrap();
                            if *ac == 0 {
                                *ac = core.gba().cpu().gpr(5) as u32;
                                *actor_at2.lock().unwrap() = t;
                            }
                        }
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // ST_AUDIODUMP: log every sfx request (id, tick) for matching
            // against the PCM dump.
            if std::env::var("ST_AUDIODUMP").is_ok() {
                let (tick, sfx_log) = (tick.clone(), sfx_log.clone());
                traps.push((
                    0x08000598,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let t = *tick.lock().unwrap();
                        sfx_log.lock().unwrap().push((core.gba().cpu().gpr(0) as u32, t));
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // ST_MUZZLELOG=1: capture the muzzle-offset pair HolyDream's
            // per-hit step gets back from the chip-keyed lookup 0x8015148
            // (trap the return site, read r0/r1).
            if std::env::var("ST_MUZZLELOG").is_ok() {
                let (log, tick) = (log.clone(), tick.clone());
                traps.push((
                    0x080bd8c6,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let t = *tick.lock().unwrap();
                        let (r0, r1) = (core.gba().cpu().gpr(0), core.gba().cpu().gpr(1));
                        log.lock().unwrap().push((
                            t,
                            format!("  MUZZLE dx={} dy={}", r0 as i32, r1 as i32),
                        ));
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // ST_ACTORLOG extras: SearchMan's aim/fire/teardown subs and the
            // Scope Gun shot spawner, with live registers, so the real
            // firing protocol can be read off a timeline instead of
            // reconstructed from static disassembly.
            if actorlog {
                for (a, name) in [
                    (0x080c155cu32, "aim0"),
                    (0x080c156a, "aim1"),
                    (0x080c15a4, "fire0"),
                    (0x080c15e4, "fire1"),
                    (0x080c15f4, "teardown"),
                ] {
                    let (log, tick) = (log.clone(), tick.clone());
                    traps.push((
                        a,
                        Box::new(move |core: &mut mgba::core::Core| {
                            let t = *tick.lock().unwrap();
                            let r5 = core.gba().cpu().gpr(5) as u32;
                            let w64 = core.raw_read_32(r5 + 0x64, -1);
                            let w6c = core.raw_read_32(r5 + 0x6c, -1);
                            log.lock().unwrap().push((
                                t,
                                format!("  {name} obj+64={w64:08x} obj+6c={w6c}"),
                            ));
                        }) as Box<dyn Fn(&mut mgba::core::Core)>,
                    ));
                }
                let (log_s, tick_s) = (log.clone(), tick.clone());
                traps.push((
                    0x080ceba4,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let t = *tick_s.lock().unwrap();
                        let g = |n| core.gba().cpu().gpr(n) as u32;
                        log_s.lock().unwrap().push((
                            t,
                            format!(
                                "  SHOT x={} y={} owner={:04x} dmg={} flag={:08x}",
                                g(0), g(1), g(2) & 0xffff, g(6), g(7)
                            ),
                        ));
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
                // Every object allocation with class id + call site, so the
                // spawner (and through its address, the orchestrator) of
                // each summon entity can be read off the timeline.
                for alloc in [0x08003140u32, 0x08003178] {
                    let (log, tick) = (log.clone(), tick.clone());
                    traps.push((
                        alloc,
                        Box::new(move |core: &mut mgba::core::Core| {
                            let t = *tick.lock().unwrap();
                            if t < 500 {
                                return;
                            }
                            let cls = core.gba().cpu().gpr(0) as u32;
                            let lr = core.gba().cpu().gpr(14) as u32 & !1;
                            log.lock().unwrap().push((
                                t,
                                format!("  ALLOC {alloc:08x} class={cls:#04x} from {lr:08x}"),
                            ));
                        }) as Box<dyn Fn(&mut mgba::core::Core)>,
                    ));
                }
                // The two companion entities the discovery trace saw fire
                // alongside the actor: log their object fields on change —
                // their positions over time are the targeting record.
                for (a, name) in [(0x080d0d78u32, "xhair"), (0x080e5208, "ent5208")] {
                    let (log, tick) = (log.clone(), tick.clone());
                    let last = Arc::new(Mutex::new([0u8; 0x40]));
                    traps.push((
                        a,
                        Box::new(move |core: &mut mgba::core::Core| {
                            let t = *tick.lock().unwrap();
                            let r5 = core.gba().cpu().gpr(5) as u32;
                            let mut f = [0u8; 0x40];
                            core.raw_read_range(r5, -1, &mut f);
                            let mut l = last.lock().unwrap();
                            if *l != f {
                                *l = f;
                                log.lock().unwrap().push((
                                    t,
                                    format!(
                                        "  {name} obj={r5:08x} flags={:02x} st={:02x}/{:02x}/{:02x} anim={:02x} pos={},{} dmg={} owner={:02x}",
                                        f[0], f[8], f[9], f[0xa], f[0x10], f[0x12], f[0x13],
                                        u32::from_le_bytes([f[0x2c], f[0x2d], f[0x2e], f[0x2f]]),
                                        f[0x16]
                                    ),
                                ));
                            }
                        }) as Box<dyn Fn(&mut mgba::core::Core)>,
                    ));
                }
            }
            // The three chip_setup_attack entry points. The raw record
            // accessor is deliberately NOT trapped: the folder/library
            // scans hammer it at boot and bury the battle traffic.
            for a in [0x0800fe72u32, 0x0800fe78, CHIP_SETUP_ATTACK] {
                let (chips_c, tick_c, log_c) = (chips.clone(), tick.clone(), log.clone());
                traps.push((
                    a,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let id = core.gba().cpu().gpr(0) as u16;
                        let t = *tick_c.lock().unwrap();
                        chips_c.lock().unwrap().entry(id).or_insert(t);
                        log_c.lock().unwrap().push((t, format!("CHIP USE via {a:08x} id={id}")));
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // Every graphics-bank load, so we can see exactly which assets
            // a navi-chip summon pulls and from which bank/entry.
            for a in [0x08002650u32] {
                let (log_c, tick_c) = (log.clone(), tick.clone());
                traps.push((
                    a,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let (r0, r1, r2) = (
                            core.gba().cpu().gpr(0) as u32,
                            core.gba().cpu().gpr(1) as u32,
                            core.gba().cpu().gpr(2) as u32,
                        );
                        let t = *tick_c.lock().unwrap();
                        log_c.lock().unwrap().push((
                            t,
                            format!("  GFX {a:08x}(anim={r0:#x}, bank={r1}, entry={r2:#x})"),
                        ));
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            // Every family dispatcher entry point, plus set_attack.
            for a in [0x0800f330u32, 0x0800f338, DISPATCH_FAMILY, 0x0800f208] {
                let (fams_c, tick_c, log_c) = (fams.clone(), tick.clone(), log.clone());
                traps.push((
                    a,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let f = core.gba().cpu().gpr(0) as u32;
                        let t = *tick_c.lock().unwrap();
                        fams_c.lock().unwrap().entry(f).or_insert(t);
                        log_c.lock().unwrap().push((t, format!("  dispatch {a:08x} r0={f:#04x}")));
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
        }
        pair.set_traps(ci, traps);
    }

    let mut pollers = [pvp.core_poller(0), pvp.core_poller(1)];
    let mut primed_at: Option<u32> = None;
    // ST_PICK mode: tick the chip-select opened (per player), read off
    // battle_state+0x14/+0x15 == 4 — keys are driven REACTIVELY from
    // this, because the select's opening tick shifts with input history
    // and a fixed-phase ladder misses it (measured: the fade hides the
    // first window, and a blind ladder leaves the select stuck open).
    let mut sel_at: [Option<u32>; 2] = [None, None];
    let audiodump = std::env::var("ST_AUDIODUMP").ok();
    let mut audio_pcm: Vec<i16> = Vec::new();
    let mut audio_idx: Vec<(u32, usize)> = Vec::new();

    for t in 0..max_ticks {
        *tick.lock().unwrap() = t;
        // Once in a fight, mash a deterministic pattern so both sides open
        // the custom screen and actually send chips. 0x0f3 deliberately
        // omits L/R (see the team-switch notes) — nothing here needs them.
        // Once in a fight, drive the custom screen with single, held keys
        // rather than random bitmasks: a random mask presses A and B in the
        // same frame and never confirms. This walks the cursor and taps A
        // often enough to add a chip and hit OK. 6-tick phases: press for
        // 3, release for 3, so every press is a fresh edge.
        const A: u32 = 0x001;
        const RIGHT: u32 = 0x010;
        const LEFT: u32 = 0x020;
        const UP: u32 = 0x040;
        const DOWN: u32 = 0x080;
        const SEQ: [u32; 8] = [A, LEFT, A, UP, A, RIGHT, A, DOWN];
        // In actorlog mode, don't wander the custom screen forever — pick
        // the lead chip, confirm, and go quiet (the 12-phase wrap presses A
        // once in battle, which uses the queued chip). Endless custom-screen
        // mashing keeps the battle paused and the actor never leaves the
        // engine's wait state, let alone fires.
        const START: u32 = 0x008;
        const CALM: [u32; 12] = [A, 0, 0, START, 0, 0, A, 0, 0, 0, 0, 0];
        // ST_PICK=<n>: in calm mode, walk the custom cursor n slots RIGHT
        // before adding (the hand is dealt shuffled, and slot 0 of this
        // save's hand turned out to be un-usable -- the A/START/A default
        // never queued anything and the in-battle A only fired the
        // buster). The pick phases run only during the opening custom
        // window; in battle, a plain A comes every 12 phases.
        let pick_n: usize = std::env::var("ST_PICK").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mut keys = [0u32; 2];
        if let Some(p0) = primed_at {
            // Phases ANCHORED to primed_at (the raw t/6 the legacy modes
            // use only works because their sequences cycle; the one-shot
            // pick sequence needs absolute positions).
            let phase = ((t - p0) / 6) as usize;
            let sel_now: [bool; 2] = {
                let c = pair.core_mut(0);
                [
                    c.raw_read_8(0x02034a90 + 0x14, -1) == 4,
                    c.raw_read_8(0x02034a90 + 0x15, -1) == 4,
                ]
            };
            for i in 0..2 {
                if sel_now[i] {
                    if sel_at[i].is_none() {
                        sel_at[i] = Some(t);
                    }
                } else {
                    sel_at[i] = None;
                }
            }
            let pick = |p: usize| {
                if actorlog && std::env::var("ST_PICK").is_ok() {
                    // p here is the side index, not a phase
                    let i = p % 2;
                    if t % 6 >= 3 {
                        return 0;
                    }
                    return if let Some(s0) = sel_at[i] {
                        let ph = ((t - s0) / 6) as usize;
                        if ph < pick_n {
                            RIGHT
                        } else if ph == pick_n {
                            A
                        } else if ph == pick_n + 2 {
                            START
                        } else if ph > pick_n && (ph - pick_n) % 4 == 0 {
                            A // keep confirming until it closes
                        } else {
                            0
                        }
                    } else if (t / 6) % 12 == 0 {
                        A // in battle: fire the queued chip
                    } else {
                        0
                    };
                }
                let s: &[u32] = if actorlog { &CALM } else { &SEQ };
                if t % 6 < 3 {
                    s[p % s.len()]
                } else {
                    0
                }
            };
            // Offset core 1 so the two sides don't move in lockstep.
            // (In ST_PICK mode the closure takes the SIDE index instead.)
            let legacy_phase = (t / 6) as usize;
            let _ = phase;
            if actorlog && std::env::var("ST_PICK").is_ok() {
                keys = [pick(0), pick(1)];
            } else {
                keys = [pick(legacy_phase), pick(legacy_phase + 3)];
            }
            if std::env::var("ST_MOVETEST").is_ok()
                && *actor.lock().unwrap() != 0
                && (t / 6) % 2 == 0
            {
                keys[0] |= UP;
                keys[1] |= UP;
            }
            // ST_STOP_AT=<n>: press A on both sides n ticks after the
            // actor appears -- aim it early to stop BN5's cursor short of
            // the enemy and observe the genuine miss behavior.
            if let Ok(n) = std::env::var("ST_STOP_AT") {
                let a = *actor_at.lock().unwrap();
                if a != 0 {
                    if let Ok(n) = n.parse::<u32>() {
                        if (a + n..a + n + 3).contains(&t) {
                            keys[0] |= A | 0x002; // A+B: also arms, so the
                            keys[1] |= A | 0x002; // 0x8b chirp is on record
                        }
                    }
                }
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

        if primed_at.is_none() && pvp.debug_battle_state(pair.core_mut(0)) != [0; 8] {
            primed_at = Some(t);
            println!("primed at tick {t}");
        }
        let _ = pollers[0].poll(pair.core_mut(0), &events_sink, 0);
        let _ = pollers[1].poll(pair.core_mut(1), &events_sink, 0);

        // ST_SHOTS=<dir>: dump frames (every 3 ticks once the actor
        // exists) so the real BN5 summon can be looked at, not inferred.
        if let Ok(dir) = std::env::var("ST_SHOTS") {
            // ST_SHOTS_FROM=<tick>: dump from a fixed tick instead of
            // waiting on the actor capture (which only actorlog mode
            // performs) — how the HolyDream reference set is taken.
            let from: Option<u32> = std::env::var("ST_SHOTS_FROM").ok().and_then(|s| s.parse().ok());
            let armed = match from {
                Some(f) => t >= f,
                None => *actor.lock().unwrap() != 0,
            };
            if armed && t % 3 == 0 {
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
                    let _ = img.save(format!("{dir}/t{t}.png"));
                }
            }
        }

        // ST_MOVETEST: once the actor exists, hammer UP on both sides so
        // post-volley mobility (hitstun/paralysis length) is measurable.
        // Applied via the NEXT tick's keys (see below).
        // ST_ACTORLOG extra: both units' owner/tile/hp on change.
        if actorlog {
            let c = pair.core_mut(0);
            let mut brief = [(0u8, 0u8, 0u8, 0u16); 2];
            for (s, b) in brief.iter_mut().enumerate() {
                let base = 0x0203b200 + s as u32 * 0xd8;
                *b = (
                    c.raw_read_8(base + 0x16, -1),
                    c.raw_read_8(base + 0x12, -1),
                    c.raw_read_8(base + 0x13, -1),
                    c.raw_read_16(base + 0x24, -1),
                );
            }
            static ULAST: Mutex<Option<[(u8, u8, u8, u16); 2]>> = Mutex::new(None);
            let mut ul = ULAST.lock().unwrap();
            if ul.as_ref() != Some(&brief) {
                println!(
                    "[{t:5}] units: o{} @{},{} hp={} | o{} @{},{} hp={}",
                    brief[0].0, brief[0].1, brief[0].2, brief[0].3,
                    brief[1].0, brief[1].1, brief[1].2, brief[1].3,
                );
                *ul = Some(brief);
            }
        }
        // ST_FORCE_CHIP=<id>: poke the id straight into player 0's
        // loaded-chip block (0x02034e20 + p*0x50: +0 fired, +2 ids[6])
        // on BOTH cores at ST_FORCE_AT (default 600) — bypasses the
        // custom screen entirely, so the REAL chip fires (class byte
        // included: a poked 307 runs the authentic giga presentation,
        // which no standard-chip graft can). The next in-battle A uses
        // it.
        if let Ok(id) = std::env::var("ST_FORCE_CHIP") {
            let at: u32 = std::env::var("ST_FORCE_AT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(600);
            if t == at {
                if let Ok(id) = id.parse::<u16>() {
                    for ci in 0..2 {
                        let c = pair.core_mut(ci);
                        let base = 0x02034e20u32;
                        c.raw_write_16(base, -1, 0); // fired = 0
                        c.raw_write_16(base + 2, -1, id);
                        for s in 1..6u32 {
                            c.raw_write_16(base + 2 + s * 2, -1, 0xffff);
                        }
                        // the unit's own loaded-chip cell (+0x2a, 0xffff
                        // = none) must agree or the A press won't fire
                        for slot in 0..2u32 {
                            let u = 0x0203b200 + slot * 0xd8;
                            if c.raw_read_8(u + 0x16, -1) == 0 {
                                c.raw_write_16(u + 0x2a, -1, id);
                            }
                        }
                        // and the per-player chip-select state
                        // (battle_state +0x14/+0x15: 4 = still choosing)
                        // must read confirmed, or battle input stays
                        // swallowed by the invisible custom screen
                        c.raw_write_8(0x02034a90 + 0x14, -1, 0);
                        c.raw_write_8(0x02034a90 + 0x15, -1, 0);
                    }
                    println!("[{t:5}] FORCE_CHIP: id {id} loaded for player 0");
                }
            }
        }
        // ST_HOLYPREP=<n>: at tick 900, set n of the right side's panels
        // (columns 4-6, row-major) to holy (type 9) on BOTH cores, for
        // measuring HolyDream's panel-count damage scaling.
        if let Ok(n) = std::env::var("ST_HOLYPREP") {
            let at: u32 = std::env::var("ST_HOLYPREP_AT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900);
            if t == at {
                if let Ok(n) = n.parse::<u32>() {
                    // ST_HOLYPREP_LEFT=1: put them on columns 1-3 instead
                    // (the summoner's ENEMY side) to test scan coverage.
                    let cols: [u32; 3] = if std::env::var("ST_HOLYPREP_LEFT").is_ok() {
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
                                c.raw_write_8(0x0203a100 + (y * 8 + x) * 0x24 + 2, -1, 9);
                                left -= 1;
                            }
                        }
                    }
                    println!("[{t:5}] HOLYPREP: {n} holy panels on cols {cols:?}");
                }
            }
        }
        // ST_PANELLOG=1: the field's 18 panel-type bytes on change
        // (BN5 panel array 0x0203a100, stride 0x24, index y*8+x, type +2).
        if std::env::var("ST_PANELLOG").is_ok() {
            let c = pair.core_mut(0);
            let mut grid = [[0u8; 6]; 3];
            for y in 1..=3u32 {
                for x in 1..=6u32 {
                    grid[y as usize - 1][x as usize - 1] =
                        c.raw_read_8(0x0203a100 + (y * 8 + x) * 0x24 + 2, -1);
                }
            }
            static PLAST2: Mutex<Option<[[u8; 6]; 3]>> = Mutex::new(None);
            let mut pl = PLAST2.lock().unwrap();
            if pl.as_ref() != Some(&grid) {
                let rows: Vec<String> = grid
                    .iter()
                    .map(|r| r.iter().map(|v| format!("{v:x}")).collect::<String>())
                    .collect();
                println!("[{t:5}] panels: {}", rows.join(" / "));
                *pl = Some(grid);
            }
        }
        // ST_ACTORLOG: print the actor's state/anim fields on change.
        if actorlog {
            let a = *actor.lock().unwrap();
            if a != 0 {
                let c = pair.core_mut(0);
                let mut f = [0u8; 0x40];
                c.raw_read_range(a, -1, &mut f);
                static LAST: Mutex<Option<[u8; 0x40]>> = Mutex::new(None);
                let mut last = LAST.lock().unwrap();
                if last.map_or(true, |l| l != f) {
                    println!(
                        "[{t:5}] flags={:02x} cls={:02x} st={:02x}/{:02x}/{:02x} anim={:02x}/{:02x} pos={},{} t20={:04x} t22={:04x} y3e={:04x}",
                        f[0], f[1], f[8], f[9], f[0xa], f[0x10], f[0x11], f[0x12], f[0x13],
                        u16::from_le_bytes([f[0x20], f[0x21]]),
                        u16::from_le_bytes([f[0x22], f[0x23]]),
                        u16::from_le_bytes([f[0x3e], f[0x3f]]),
                    );
                    *last = Some(f);
                }
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

    let chips = chips.lock().unwrap();
    println!("\nchip ids reaching chip_setup_attack: {} distinct", chips.len());
    for (id, t) in chips.iter().take(40) {
        println!("  chip {id:4} first@{t}");
    }
    let fams = fams.lock().unwrap();
    println!("\nfamilies reaching dispatch_family: {} distinct", fams.len());
    for (f, t) in fams.iter() {
        println!("  family {f:#04x} first@{t}");
    }
    let hits = hits.lock().unwrap();
    println!("\nbehavior entries that executed: {} of {}", hits.len(), entries.len());
    let mut by_tick: Vec<_> = hits.iter().map(|(a, (n, t))| (*t, *a, *n)).collect();
    by_tick.sort();
    for (t, a, n) in by_tick {
        println!("  {a:08x}  hits={n:<7} first@{t}");
    }

    // Timeline, trimmed to the neighbourhood of chip uses.
    let log = log.lock().unwrap();
    let chip_ticks: Vec<u32> = log
        .iter()
        .filter(|(_, s)| s.starts_with("CHIP USE"))
        .map(|(t, _)| *t)
        .collect();
    println!("\nchip-use events: {}", chip_ticks.len());
    if let Some(&first) = chip_ticks.first() {
        println!("timeline around first chip use (tick {first}):");
        let window = if actorlog { 4000 } else { 400 };
        for (t, s) in log.iter().filter(|(t, _)| *t >= first.saturating_sub(4) && *t <= first + window) {
            println!("  [{t:6}] {s}");
        }
    }
}
