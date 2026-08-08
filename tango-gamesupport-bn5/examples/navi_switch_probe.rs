//! Scratch probe: emulate the team-switch ROM patch's final hook set
//! with traps in a primed lockstep netbattle, and drive the full
//! scenario: L switches to slot 1, R to slot 2 (toggle back to MegaMan),
//! SELECT opens the custom screen; benched navis keep their HP.
//!
//! Hooks emulated (identical semantics to the planned ROM patch):
//! - F_stage  @0x080074ae (fight/referee tick): while the gauge is full
//!   (modeflags bit 1), L/R pressed edges (per player, from the
//!   exchanged records) stage that slot's navi (toggle to MegaMan when
//!   already active), bench HP, build the incoming config in PENDING,
//!   set modeflags bit 0x10 (the game's own commit request) and the
//!   patch's "switch commit" flag.
//! - F_pred   @0x0802d102: changeable predicate = staged != live.
//! - F_done   @0x080077a6: on a switch commit, phase := 4 (fight) and
//!   clear modeflags 0x10 instead of signaling the custom applet.
//! - H5       @0x080109e8: custom-open key mask 0x300 -> SELECT (0x4),
//!   emulated by overriding the loaded mask registers.
//!
//! Usage: navi_switch_probe <rom> <save>

use tango_backend_mgba::GameSupport as _;
use tango_gamesupport_bn5::pvp;

const BATTLE_STATE: u32 = 0x02034a90;
const PHASE_BLOCK: u32 = 0x0203c5d0;
const STAGING: u32 = 0x0203b1d0; // + p*0x10; +5 staged, +6 megaHP u16, +0xc/+0xe slot HPs
const RECORDS: u32 = 0x02036e90; // + p*8: {_, word u16, pressed u16, released u16}
const LIVE_CFG: u32 = 0x0203c880; // + p*0x60
const ROUND_CFG: u32 = 0x0203c670; // + p*0x60 (round-start pristine)
const PENDING_CFG: u32 = 0x02034ec0; // + p*0x60
const UNIT: u32 = 0x0203b200; // + slot*0xd8
// Patch state block (machine block tail, netbattle-untouched; initialized
// per round by the F_init hook): +0 commit flag, +1/+2 staged, +4/+6
// mega bench HP, +8.. slot bench HPs (2 per player).
const PSTATE: u32 = 0x0203c450;


const SLOT_NAVIS: [u8; 2] = [1, 2]; // ProtoMan, GyroMan (v1 defaults)

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"BRBE" => &pvp::PVP_BRBE_00,
        code => panic!("this probe is BRBE-only (code {:02x?})", code),
    }
}

/// The game's own config loader semantics (0x08011230 variant).
fn write_navi_config(c: &mut mgba::core::Core, rom: &[u8], base: u32, id: u8) {
    for off in 0..0x60 {
        c.raw_write_8(base + off, -1, 0);
    }
    c.raw_write_16(base + 0x44, -1, 500);
    c.raw_write_8(base + 0x5, -1, 1);
    c.raw_write_8(base + 0x7, -1, 0xff);
    c.raw_write_8(base + 0x9, -1, 4);
    c.raw_write_8(base + 0xa, -1, 5);
    c.raw_write_8(base + 0xb, -1, 5);
    c.raw_write_8(base + 0xc, -1, 1);
    c.raw_write_8(base + 0x12, -1, 0xff);
    c.raw_write_8(base + 0x21, -1, 1);
    c.raw_write_8(base + 0x27, -1, 0x1f);
    c.raw_write_8(base + 0xe, -1, 0x99);
    for off in [0x2e, 0x2f, 0x30] {
        c.raw_write_8(base + off, -1, 0xff);
    }
    c.raw_write_8(base + 0x3a, -1, 1);
    let rec = &rom[0x1d55f + id as usize * 0x10..][..0x10];
    let hp = u16::from_le_bytes([rom[0x2fd74 + id as usize * 2], rom[0x2fd75 + id as usize * 2]]);
    c.raw_write_8(base + 0x29, -1, id);
    for off in [0x3e, 0x40, 0x42] {
        c.raw_write_16(base + off, -1, hp);
    }
    c.raw_write_8(base + 0x23, -1, rec[1]);
    c.raw_write_8(base + 0x1b, -1, rec[2]);
    c.raw_write_8(base + 0x1c, -1, rec[3]);
    c.raw_write_8(base + 0x1d, -1, rec[4]);
    c.raw_write_8(base + 0x6, -1, rec[5]);
    c.raw_write_8(base + 0xb, -1, rec[6]);
    c.raw_write_8(base + 0xc, -1, rec[7]);
    c.raw_write_8(base + 0x4, -1, rec[8]);
    c.raw_write_8(base + 0x5, -1, rec[9]);
    c.raw_write_8(base + 0x7, -1, rec[0xa]);
    c.raw_write_16(base + 0x46, -1, rec[0xb] as u16);
    c.raw_write_16(base + 0x4a, -1, rec[0xc] as u16);
    c.raw_write_16(base + 0x48, -1, rec[0xd] as u16);
    c.raw_write_8(base, -1, rec[0xe]);
    c.raw_write_8(base + 0x39, -1, rec[0xf]);
}

fn unit_of(c: &mut mgba::core::Core, p: u8) -> Option<u32> {
    (0..2u32)
        .map(|slot| UNIT + slot * 0xd8)
        .find(|&u| c.raw_read_8(u + 0x16, -1) == p)
}

fn shot(pair: &mut mgba_rollback::Link, i: usize, name: &str) {
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
        let dir = std::env::var("PVP_PROBE_DUMP_DIR").unwrap_or_else(|_| ".".into());
        img.save(format!("{dir}/{name}_c{i}.png")).unwrap();
    }
}

/// The changes indicator as drawn in the HUD: one glyph per change,
/// 0x9226 = still available, 0x9227 = spent.
fn pips(c: &mut mgba::core::Core) -> String {
    (0..6)
        .map(|i| match c.raw_read_16(0x0600f830 + i * 2, -1) {
            0x9226 => '*',
            0x9227 => 'o',
            0 => '.',
            _ => '?',
        })
        .collect()
}

fn status(c: &mut mgba::core::Core, label: &str) {
    let navi = [
        c.raw_read_8(LIVE_CFG + 0x29, -1),
        c.raw_read_8(LIVE_CFG + 0x60 + 0x29, -1),
    ];
    let mut staging = [0u8; 0x20];
    c.raw_read_range(STAGING, -1, &mut staging);
    let mut units = vec![];
    for slot in 0..2u32 {
        let base = UNIT + slot * 0xd8;
        units.push((
            c.raw_read_8(base + 0x16, -1),
            c.raw_read_16(base + 0x28, -1),
            c.raw_read_16(base + 0x24, -1),
            c.raw_read_16(base + 0x26, -1),
        ));
    }
    let mut ps = [0u8; 0x14];
    c.raw_read_range(PSTATE, -1, &mut ps);
    let _ = staging;
    println!("    hud pips: {}", pips(c));
    println!(
        "{label}: pb={:02x} mode={:04x} live_navi={navi:?} staged={:02x},{:02x} megaHP={:04x},{:04x} slotHPs p0={:04x},{:04x} p1={:04x},{:04x} units={units:?}",
        c.raw_read_8(PHASE_BLOCK, -1),
        c.raw_read_16(BATTLE_STATE + 0x32, -1),
        ps[1], ps[2],
        u16::from_le_bytes([ps[4], ps[5]]),
        u16::from_le_bytes([ps[6], ps[7]]),
        u16::from_le_bytes([ps[8], ps[9]]),
        u16::from_le_bytes([ps[0xa], ps[0xb]]),
        u16::from_le_bytes([ps[0xc], ps[0xd]]),
        u16::from_le_bytes([ps[0xe], ps[0xf]]),
    );
    println!(
        "    style capture={:02x},{:02x} live+0xe={:02x},{:02x}",
        ps[0x10], ps[0x11],
        c.raw_read_8(LIVE_CFG + 0xe, -1),
        c.raw_read_8(LIVE_CFG + 0x60 + 0xe, -1),
    );
}

fn main() {
    env_logger::init();
    mgba::log::install_default_logger();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let save = std::fs::read(&args[1]).expect("save unreadable");
    let p = pvp_for(&rom);

    let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
        sides: vec![
            mgba_rollback::SideOptions {
                rom: rom.clone(),
                save: Some(save.clone()),
            },
            mgba_rollback::SideOptions {
                rom: rom.clone(),
                save: Some(save),
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
    let primed = [
        tango_backend_mgba::PrimedLatch::new(),
        tango_backend_mgba::PrimedLatch::new(),
    ];

    // NSWITCH_REAL=1: the ROM already carries the patch; only the primer
    // traps are installed.
    let real = std::env::var("NSWITCH_REAL").is_ok();
    let field_test = std::env::var("NSWITCH_FIELD").is_ok();
    for ci in 0..2 {
        let mut traps = p.primer_traps(&config, ci, &events_sink, &primed[ci]);
        if std::env::var("NSWITCH_KOFIX").is_ok() && ci == 0 {
            for (addr, label) in [
                (0x081bcb84u32, "F_pred TRUE"),
                (0x081bcbaa, "F_pred false"),
                (0x0802d0f4, "arm-change"),
                (0x0802ca64, "machine-init"),
                (0x0802cd80, "swap-apply"),
                (0x0802d102, "pred-entry"),
                (0x0802d130, "pred-TRUE"),
            ] {
                traps.push((
                    addr,
                    Box::new(move |core: &mut mgba::core::Core| {
                        println!("      [{label}] phase={:02x} navi0={}",
                            core.raw_read_8(PHASE_BLOCK, -1),
                            core.raw_read_8(LIVE_CFG + 0x29, -1));
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
        }
        if std::env::var("NSWITCH_KOFIX").is_ok() {
            let rom3 = rom.clone();
            traps.push((
                0x080138c6,
                Box::new(move |core: &mut mgba::core::Core| {
                    let unit = core.gba().cpu().gpr(5) as u32;
                    let owner = core.raw_read_8(unit + 0x16, -1);
                    if owner > 1 {
                        return;
                    }
                    let live = core.raw_read_8(LIVE_CFG + owner as u32 * 0x60 + 0x29, -1);
                    if live == 0 {
                        return; // MegaMan down: the round is lost, vanilla
                    }
                    // Skip the alive decrement entirely.
                    core.gba_mut().cpu_mut().set_thumb_pc(0x080138ca);
                    // Put the player back on the board...
                    core.raw_write_16(unit + 0x24, -1, 1);
                    core.raw_write_8(BATTLE_STATE + 0x12 + owner as u32, -1, 1);
                    // Put it back in the table the per-frame entity loop
                    // walks — the death took it out, so it stopped being
                    // updated and the swap could never advance.
                    let mut slot_written = None;
                    for i in 0..8u32 {
                        let a = BATTLE_STATE + 0x80 + i * 4;
                        if core.raw_read_32(a, -1) == unit {
                            slot_written = Some(i);
                            break;
                        }
                    }
                    if slot_written.is_none() {
                        for i in 0..8u32 {
                            let a = BATTLE_STATE + 0x80 + i * 4;
                            if core.raw_read_32(a, -1) == 0 {
                                core.raw_write_32(a, -1, unit);
                                slot_written = Some(i);
                                break;
                            }
                        }
                    }
                    println!("      [KOFIX] re-registered in table slot {slot_written:?}");
                    let obj = core.raw_read_32(unit + 0x58, -1);
                    core.raw_write_8(obj + 2, -1, 0);
                    // ...as MegaMan, on the HP he left with.
                    let round = ROUND_CFG + owner as u32 * 0x60;
                    let pend = PENDING_CFG + owner as u32 * 0x60;
                    let mut buf = vec![0u8; 0x60];
                    core.raw_read_range(round, -1, &mut buf);
                    for (i, b) in buf.iter().enumerate() {
                        core.raw_write_8(pend + i as u32, -1, *b);
                    }
                    let style = core.raw_read_8(PSTATE + 8 + owner as u32, -1);
                    if style != 0 {
                        core.raw_write_8(pend + 0xe, -1, style);
                    }
                    let bench = core.raw_read_16(PSTATE + 0x20 + owner as u32 * 0x10, -1);
                    if bench != 0xffff {
                        core.raw_write_16(pend + 0x40, -1, bench);
                    }
                    let _ = &rom3;
                    core.raw_write_8(PSTATE + 1 + owner as u32, -1, 0);
                    let mode = core.raw_read_16(BATTLE_STATE + 0x32, -1);
                    core.raw_write_16(BATTLE_STATE + 0x32, -1, mode | 0x10);
                    core.raw_write_8(PSTATE, -1, 1);
                    {
                        let mut t80 = [0u8; 0x20];
                        core.raw_read_range(BATTLE_STATE + 0x80, -1, &mut t80);
                        let mut td0 = [0u8; 0x20];
                        core.raw_read_range(BATTLE_STATE + 0xd0, -1, &mut td0);
                        println!("      [KOFIX] unit={unit:#x} table80={:08x?} tabled0={:08x?}",
                            (0..8).map(|i| u32::from_le_bytes(t80[i*4..i*4+4].try_into().unwrap())).collect::<Vec<_>>(),
                            (0..8).map(|i| u32::from_le_bytes(td0[i*4..i*4+4].try_into().unwrap())).collect::<Vec<_>>());
                    }
                    println!(
                        "      [KOFIX] player {owner} revived; obj+0x18={:02x} obj+2={:02x} unitcount={},{}",
                        core.raw_read_8(obj + 0x18, -1),
                        core.raw_read_8(obj + 2, -1),
                        core.raw_read_8(BATTLE_STATE + 0x12, -1),
                        core.raw_read_8(BATTLE_STATE + 0x13, -1),
                    );
                }) as Box<dyn Fn(&mut mgba::core::Core)>,
            ));
        }
        if std::env::var("NSWITCH_CHIPTRACE").is_ok() && ci == 0 {
            let seen = std::cell::Cell::new(0u32);
            traps.push((
                0x08014a5c,
                Box::new(move |core: &mut mgba::core::Core| {
                    if seen.get() >= 3 {
                        return;
                    }
                    seen.set(seen.get() + 1);
                    let u = core.gba().cpu().gpr(5) as u32;
                    let f = |o: u32| core.raw_read_16(u + o, -1);
                    println!(
                        "      [chip use] unit={u:#x} owner={} +0x18={:02x} +0x19={:02x} +0x1e={:04x} +0x30={:04x} +0x32={:04x} +0x36={:04x} +0x38={:08x} +0x3e={:04x}",
                        core.raw_read_8(u + 0x16, -1),
                        core.raw_read_8(u + 0x18, -1),
                        core.raw_read_8(u + 0x19, -1),
                        f(0x1e), f(0x30), f(0x32), f(0x36),
                        core.raw_read_32(u + 0x38, -1),
                        f(0x3e),
                    );
                }) as Box<dyn Fn(&mut mgba::core::Core)>,
            ));
        }
        if std::env::var("NSWITCH_SPPORT").is_ok() && ci == 0 {
            traps.push((
                0x081bd456,
                Box::new(move |core: &mut mgba::core::Core| {
                    let seen = core.raw_read_32(0x0203f124, -1);
                    let _ = seen;
                    if core.gba().cpu().gpr(0) != 0 {
                        core.raw_write_32(0x0203f128, -1, core.raw_read_32(0x0203f128, -1) + 1);
                    }
                }) as Box<dyn Fn(&mut mgba::core::Core)>,
            ));
            traps.push((
                0x081bd44a,
                Box::new(move |core: &mut mgba::core::Core| {
                    let seen = core.raw_read_32(0x0203f120, -1);
                    if seen < 4 {
                        println!(
                            "      [F_support] player={} tag_read={:#x} pstate_c={:02x},{:02x}",
                            core.gba().cpu().gpr(7),
                            core.gba().cpu().gpr(0),
                            core.raw_read_8(0x0203c450 + 0xc, -1),
                            core.raw_read_8(0x0203c450 + 0xd, -1),
                        );
                        core.raw_write_32(0x0203f120, -1, seen + 1);
                    }
                }) as Box<dyn Fn(&mut mgba::core::Core)>,
            ));
        }
        if std::env::var("NSWITCH_KOTRACE").is_ok() && ci == 0 {
            for (addr, label) in [
                (0x080138b6u32, "unit-delete-handler"),
                (0x08008ae4, "alive--"),
                (0x08008afc, "unregister"),
                (0x0802cd80, "swap-apply"),
            ] {
                traps.push((
                    addr,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let unit0 = (0..2u32)
                            .map(|sl| UNIT + sl * 0xd8)
                            .find(|&u| core.raw_read_8(u + 0x16, -1) == 0);
                        println!(
                            "      [{label}] tick={} phase={:02x} alive={},{} navi0={} unit0_hp={:?}",
                            core.raw_read_32(BATTLE_STATE + 0x60, -1),
                            core.raw_read_8(PHASE_BLOCK, -1),
                            core.raw_read_8(BATTLE_STATE + 4, -1),
                            core.raw_read_8(BATTLE_STATE + 5, -1),
                            core.raw_read_8(LIVE_CFG + 0x29, -1),
                            unit0.map(|u| core.raw_read_16(u + 0x24, -1)),
                        );
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
        }
        if std::env::var("NSWITCH_KO").is_ok() && std::env::var("NSWITCH_KOTRACE").is_err() && ci == 0 {
            let seq = std::cell::Cell::new(0u32);
            let seq2 = seq.clone();
            traps.push((
                0x080074ae,
                Box::new(move |core: &mut mgba::core::Core| {
                    let n = seq.get() + 1;
                    seq.set(n);
                    if std::env::var("NSWITCH_ORDER").is_ok() {
                        println!("      [order] fight-hook  tick={} n={n}", core.raw_read_32(BATTLE_STATE + 0x60, -1));
                    }
                }) as Box<dyn Fn(&mut mgba::core::Core)>,
            ));
            traps.push((
                0x08008afc,
                Box::new(move |core: &mut mgba::core::Core| {
                    let _ = &seq2;
                    if std::env::var("NSWITCH_ORDER").is_ok() {
                        println!("      [order] unit-death  tick={}", core.raw_read_32(BATTLE_STATE + 0x60, -1));
                    }
                }) as Box<dyn Fn(&mut mgba::core::Core)>,
            ));
        }
        if field_test && ci == 0 && std::env::var("NSWITCH_TEXTTRACE").is_ok() {
            let seen = std::cell::Cell::new(0u32);
            traps.push((
                0x08045a3c,
                Box::new(move |core: &mut mgba::core::Core| {
                    if seen.get() >= 5 {
                        return;
                    }
                    seen.set(seen.get() + 1);
                    let cpu = core.gba().cpu();
                    println!(
                        "      [text render] bank={:08x} entry={} r2={:08x} r3={:08x} r4={} r5={} r6={:08x} r7={}",
                        cpu.gpr(0), cpu.gpr(1), cpu.gpr(2), cpu.gpr(3),
                        cpu.gpr(4), cpu.gpr(5), cpu.gpr(6), cpu.gpr(7),
                    );
                }) as Box<dyn Fn(&mut mgba::core::Core)>,
            ));
        }
        if field_test {
            // Drop the field/menu/comm walk redirects: boot to CONTINUE
            // and stay in the overworld.
            traps.retain(|(addr, _)| {
                ![0x080054b6u32, 0x0812864c, 0x08134d0a, 0x08134e04].contains(addr)
            });
            if ci == 0 {
            traps.push((
                    0x08004e4a,
                    Box::new(move |core: &mut mgba::core::Core| {
                        core.raw_write_32(0x0203f01c, -1, core.raw_read_32(0x0203f01c, -1) + 1);
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            traps.push((
                    0x08000436,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let blk = core.gba().cpu().gpr(0) as u32;
                        core.raw_write_32(0x0203f014, -1,
                            core.raw_read_32(0x0203f014, -1) | core.raw_read_16(blk, -1) as u32);
                        core.raw_write_32(0x0203f018, -1,
                            core.raw_read_32(0x0203f018, -1) | core.raw_read_16(blk + 2, -1) as u32);
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            traps.push((
                    0x080003ea,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let keyinput = core.raw_read_16(0x04000130, -1) as u32;
                        let active = (!keyinput) & 0x3ff;
                        core.raw_write_32(0x0203f010, -1, core.raw_read_32(0x0203f010, -1) | active);
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            traps.push((
                    0x080054c6,
                    Box::new(move |core: &mut mgba::core::Core| {
                        let blk = core.raw_read_32(core.gba().cpu().gpr(10) as u32 + 4, -1);
                        let held = core.raw_read_16(blk, -1) as u32;
                        let pressed = core.raw_read_16(blk + 2, -1) as u32;
                        // scratch counters: hits, last held, last pressed
                        core.raw_write_32(0x0203f000, -1, core.raw_read_32(0x0203f000, -1) + 1);
                        core.raw_write_32(0x0203f004, -1, held);
                        core.raw_write_32(0x0203f008, -1, core.raw_read_32(0x0203f008, -1) | pressed);
                        core.raw_write_32(0x0203f00c, -1, core.raw_read_32(0x0203f00c, -1) | held);
                    }) as Box<dyn Fn(&mut mgba::core::Core)>,
                ));
            }
            pair.set_traps(ci, traps);
            continue;
        }
        if real {
            pair.set_traps(ci, traps);
            continue;
        }

        // F_stage: L/R switch staging at the fight tick.
        let rom2 = rom.clone();
        traps.push((
            0x080074ae,
            Box::new(move |core: &mut mgba::core::Core| {
                if core.raw_read_16(BATTLE_STATE + 0x32, -1) & 2 == 0 {
                    return;
                }
                let mut any = false;
                for pl in 0..2u8 {
                    let pressed = core.raw_read_16(RECORDS + pl as u32 * 8 + 4, -1) as u32;
                    let want_slot = if pressed & 0x200 != 0 {
                        Some(0)
                    } else if pressed & 0x100 != 0 {
                        Some(1)
                    } else {
                        None
                    };
                    let Some(slot) = want_slot else { continue };
                    let live_cfg = LIVE_CFG + pl as u32 * 0x60;
                    let live = core.raw_read_8(live_cfg + 0x29, -1);
                    let mut want = SLOT_NAVIS[slot];
                    if live == want {
                        want = 0;
                    }
                    if want == live {
                        continue;
                    }
                    core.raw_write_8(PSTATE + 1 + pl as u32, -1, want);
                    // Bench the outgoing navi's HP.
                    if let Some(unit) = unit_of(core, pl) {
                        let hp = core.raw_read_16(unit + 0x24, -1);
                        if live == 0 {
                            core.raw_write_16(PSTATE + 4 + pl as u32 * 2, -1, hp);
                        } else if live == SLOT_NAVIS[0] {
                            core.raw_write_16(PSTATE + 8 + pl as u32 * 4, -1, hp);
                        } else if live == SLOT_NAVIS[1] {
                            core.raw_write_16(PSTATE + 0xa + pl as u32 * 4, -1, hp);
                        }
                    }
                    // Build the incoming navi's config in PENDING.
                    let pend = PENDING_CFG + pl as u32 * 0x60;
                    if want == 0 {
                        let mut buf = vec![0u8; 0x60];
                        core.raw_read_range(ROUND_CFG + pl as u32 * 0x60, -1, &mut buf);
                        for (i, b) in buf.iter().enumerate() {
                            core.raw_write_8(pend + i as u32, -1, *b);
                        }
                        let bench = core.raw_read_16(PSTATE + 4 + pl as u32 * 2, -1);
                        if bench != 0xffff {
                            core.raw_write_16(pend + 0x40, -1, bench);
                        }
                    } else {
                        write_navi_config(core, &rom2, pend, want);
                        let bench = core.raw_read_16(
                            PSTATE + pl as u32 * 4 + if want == SLOT_NAVIS[0] { 8 } else { 0xa },
                            -1,
                        );
                        if bench != 0 {
                            core.raw_write_16(pend + 0x40, -1, bench);
                        }
                    }
                    any = true;
                }
                if any {
                    let mode = core.raw_read_16(BATTLE_STATE + 0x32, -1);
                    core.raw_write_16(BATTLE_STATE + 0x32, -1, mode | 0x10);
                    core.raw_write_8(PSTATE, -1, 1);
                }
            }) as Box<dyn Fn(&mut mgba::core::Core)>,
        ));

        // F_init at the battle-start tail: per-round patch state init.
        traps.push((
            0x08006e62,
            Box::new(move |core: &mut mgba::core::Core| {
                core.raw_write_8(PSTATE, -1, 0);
                for pl in 0..2u32 {
                    let live = core.raw_read_8(LIVE_CFG + pl * 0x60 + 0x29, -1);
                    core.raw_write_8(PSTATE + 1 + pl, -1, live);
                    core.raw_write_16(PSTATE + 4 + pl * 2, -1, 0xffff);
                    core.raw_write_16(PSTATE + 8 + pl * 4, -1, 0);
                    core.raw_write_16(PSTATE + 0xa + pl * 4, -1, 0);
                }
            }) as Box<dyn Fn(&mut mgba::core::Core)>,
        ));

        // F_pred.
        traps.push((
            0x0802d10a,
            Box::new(move |core: &mut mgba::core::Core| {
                let unit = core.gba().cpu().gpr(5) as u32;
                let ok = if unit == 0 {
                    false
                } else {
                    let owner = core.raw_read_8(unit + 0x16, -1);
                    let obj = core.raw_read_32(unit + 0x58, -1);
                    let staged = core.raw_read_8(PSTATE + 1 + owner as u32, -1);
                    let live = core.raw_read_8(LIVE_CFG + owner as u32 * 0x60 + 0x29, -1);
                    core.raw_read_8(obj + 0x18, -1) == 0 && staged <= 6 && staged != live
                };
                core.gba_mut()
                    .cpu_mut()
                    .set_thumb_pc(if ok { 0x0802d130 } else { 0x0802d132 });
            }) as Box<dyn Fn(&mut mgba::core::Core)>,
        ));

        // F_done.
        traps.push((
            0x080077a6,
            Box::new(move |core: &mut mgba::core::Core| {
                if core.raw_read_8(PSTATE, -1) != 0 {
                    core.raw_write_8(PSTATE, -1, 0);
                    core.raw_write_32(PHASE_BLOCK, -1, 4);
                    let mode = core.raw_read_16(BATTLE_STATE + 0x32, -1);
                    core.raw_write_16(BATTLE_STATE + 0x32, -1, mode & !0x10);
                    core.gba_mut().cpu_mut().set_thumb_pc(0x080077aa);
                }
                // else: vanilla [pb+4]=6 (custom applet) runs.
            }) as Box<dyn Fn(&mut mgba::core::Core)>,
        ));

        // H5: custom-open mask 0x300 -> SELECT (0x4).
        traps.push((
            0x08010756,
            Box::new(move |core: &mut mgba::core::Core| {
                if core.gba().cpu().gpr(1) == 0x300 {
                    core.gba_mut().cpu_mut().set_gpr(1, 0x4);
                }
            }) as Box<dyn Fn(&mut mgba::core::Core)>,
        ));
        traps.push((
            0x08010782,
            Box::new(move |core: &mut mgba::core::Core| {
                if core.gba().cpu().gpr(1) == 0x300 {
                    core.gba_mut().cpu_mut().set_gpr(1, 0x4);
                }
            }) as Box<dyn Fn(&mut mgba::core::Core)>,
        ));

        pair.set_traps(ci, traps);
    }

    // ---- scenario driver ----
    // NSWITCH_EXPECT="a,b": the slot navis the assertions expect
    // (defaults 1,2 — the patch's fallback slots).
    let expect: [u8; 2] = std::env::var("NSWITCH_EXPECT")
        .ok()
        .and_then(|s| {
            let (a, b) = s.split_once(',')?;
            Some([a.trim().parse().ok()?, b.trim().parse().ok()?])
        })
        .unwrap_or([1, 2]);
    let mut t = 0u32;
    // L/R switch navis now, so keep them out of the random mashing.
    let wiggle = |t: u32, i: usize| ((t / 5).wrapping_mul(2654435761) >> i) & 0x0f3;

    // NSWITCH_FIELD: overworld slot-configurator test.
    if field_test {
        // Clear whatever dialog the save boots into: mash A until the
        // field tick's key site is running steadily (free roam).
        let mut free = false;
        for i in 0..6000u32 {
            t += 1;
            pair.tick(&[if i % 12 < 4 { 0x1 } else { 0 }, 0]);
            if i % 120 == 119 {
                let c = pair.core_mut(0);
                let hits = c.raw_read_32(0x0203f000, -1);
                c.raw_write_32(0x0203f000, -1, 0);
                if hits > 100 {
                    println!("[field] free roam reached at i={i} (site hits {hits}/120)");
                    free = true;
                    break;
                }
            }
        }
        if !free {
            println!("[field] never reached free roam");
        }
        let cfg = |c: &mut mgba::core::Core| {
            [c.raw_read_8(0x02007c10, -1), c.raw_read_8(0x02007c11, -1)]
        };
        println!("[field] initial cfg={:?}", cfg(pair.core_mut(0)));
        let mut pulse = |pair: &mut mgba_rollback::Link, t: &mut u32, key: u32, label: &str| {
            for _ in 0..30 {
                *t += 1;
                pair.tick(&[0x4, 0]);
            }
            for _ in 0..30 {
                *t += 1;
                pair.tick(&[key, 0]);
            }
            for _ in 0..30 {
                *t += 1;
                pair.tick(&[0x4, 0]);
            }
            for _ in 0..30 {
                *t += 1;
                pair.tick(&[0, 0]);
            }
            {
                let c = pair.core_mut(0);
                println!(
                    "           menu_state={:02x} dispcnt={:04x} bg3cnt={:04x}",
                    c.raw_read_8(0x02007c13, -1),
                    c.raw_read_16(0x04000000, -1),
                    c.raw_read_16(0x0400000e, -1),
                );
                println!(
                    "[field] after {label}: cfg={:?} site_hits={} last_held={:04x} pressed_seen={:04x}",
                    [c.raw_read_8(0x02007c10, -1), c.raw_read_8(0x02007c11, -1)],
                    c.raw_read_32(0x0203f000, -1),
                    c.raw_read_32(0x0203f004, -1),
                    c.raw_read_32(0x0203f008, -1),
                );
                println!(
                    "           held_seen={:04x} KEYINPUT_seen={:04x} joy_ret held={:04x} pressed={:04x}",
                    c.raw_read_32(0x0203f00c, -1),
                    c.raw_read_32(0x0203f010, -1),
                    c.raw_read_32(0x0203f014, -1),
                    c.raw_read_32(0x0203f018, -1),
                );
                println!("           overworld_tick_hits={}", c.raw_read_32(0x0203f01c, -1));
                c.raw_write_32(0x0203f01c, -1, 0);
                c.raw_write_32(0x0203f014, -1, 0);
                c.raw_write_32(0x0203f018, -1, 0);
                c.raw_write_32(0x0203f00c, -1, 0);
                c.raw_write_32(0x0203f010, -1, 0);
                c.raw_write_32(0x0203f000, -1, 0);
                c.raw_write_32(0x0203f008, -1, 0);
            }
        };
        // Walk off the story trigger this save is parked on, then stand
        // still so the field tick's key site keeps running.
        for (dir, n) in [(0x80u32, 90u32), (0x20, 90), (0x80, 60)] {
            for _ in 0..n {
                t += 1;
                pair.tick(&[dir, 0]);
            }
        }
        for _ in 0..60 {
            t += 1;
            pair.tick(&[0, 0]);
        }
        {
            let c = pair.core_mut(0);
            c.raw_write_32(0x0203f000, -1, 0);
        }
        for _ in 0..120 {
            t += 1;
            pair.tick(&[0, 0]);
        }
        {
            let c = pair.core_mut(0);
            println!("[field] after walking away: site hits {}/120", c.raw_read_32(0x0203f000, -1));
            c.raw_write_32(0x0203f000, -1, 0);
        }
        {
            let dir = std::env::var("PVP_PROBE_DUMP_DIR").unwrap_or_else(|_| ".".into());
            let c = pair.core_mut(0);
            let mut vram = vec![0u8; 0x18000];
            c.raw_read_range(0x06000000, -1, &mut vram);
            std::fs::write(format!("{dir}/vram_field.bin"), &vram).unwrap();
            let mut io = vec![0u8; 0x60];
            c.raw_read_range(0x04000000, -1, &mut io);
            std::fs::write(format!("{dir}/io_field.bin"), &io).unwrap();
            let mut pal = vec![0u8; 0x400];
            c.raw_read_range(0x05000000, -1, &mut pal);
            std::fs::write(format!("{dir}/pal_field.bin"), &pal).unwrap();
            let mut oam = vec![0u8; 0x400];
            c.raw_read_range(0x07000000, -1, &mut oam);
            std::fs::write(format!("{dir}/oam_field.bin"), &oam).unwrap();
            let mut shadow = vec![0u8; 0x400];
            c.raw_read_range(0x03003d70, -1, &mut shadow);
            std::fs::write(format!("{dir}/oamshadow_field.bin"), &shadow).unwrap();
        }
        shot(&mut pair, 0, "field_roam");
        // one key at a time now that SELECT is the menu key
        let mut tap = |pair: &mut mgba_rollback::Link, t: &mut u32, key: u32, label: &str| {
            for _ in 0..6 {
                *t += 1;
                pair.tick(&[key, 0]);
            }
            for _ in 0..24 {
                *t += 1;
                pair.tick(&[0, 0]);
            }
            let c = pair.core_mut(0);
            println!(
                "[field] {label}: menu={:02x} cfg={:?}",
                c.raw_read_8(0x02007c13, -1),
                [c.raw_read_8(0x02007c10, -1), c.raw_read_8(0x02007c11, -1)],
            );
        };
        tap(&mut pair, &mut t, 0x4, "SELECT (open)");
        shot(&mut pair, 0, "menu_open");
        tap(&mut pair, &mut t, 0x80, "Down");
        tap(&mut pair, &mut t, 0x200, "L (slot 1)");
        tap(&mut pair, &mut t, 0x80, "Down");
        tap(&mut pair, &mut t, 0x80, "Down");
        tap(&mut pair, &mut t, 0x100, "R (slot 2)");
        shot(&mut pair, 0, "menu_slot2");
        tap(&mut pair, &mut t, 0x4, "SELECT (close)");
        shot(&mut pair, 0, "menu_closed");
        shot(&mut pair, 0, "field_end");
        std::process::exit(0);
    }

    // A: prime + wiggle into the running fight.
    let mut fight_streak = 0;
    let mut prev_tick = 0u32;
    loop {
        t += 1;
        assert!(t < 20000, "never reached fight");
        let keys = if primed[0].is_set() && primed[1].is_set() {
            [wiggle(t, 0), wiggle(t, 1)]
        } else {
            [0, 0]
        };
        pair.tick(&keys);
        let c = pair.core_mut(0);
        let cust = [
            c.raw_read_8(BATTLE_STATE + 0x14, -1),
            c.raw_read_8(BATTLE_STATE + 0x15, -1),
        ];
        let tick = c.raw_read_32(BATTLE_STATE + 0x60, -1);
        let advancing = tick == prev_tick + 1;
        prev_tick = tick;
        let pb = c.raw_read_8(PHASE_BLOCK, -1);
        if advancing && tick > 400 && cust == [0, 0] && (pb == 4 || pb == 8) {
            fight_streak += 1;
            if fight_streak >= 40 {
                break;
            }
        } else {
            fight_streak = 0;
        }
    }
    let turn_restarts = std::cell::Cell::new(0u32);
    let cfg_before = {
        let c = pair.core_mut(0);
        [c.raw_read_8(0x02007c10, -1), c.raw_read_8(0x02007c11, -1)]
    };
    // NSWITCH_PARTS: pretend each player installed a Party Support
    // program — player 0 NavChg+2 (config +0x35), player 1 NavChg+1
    // (+0x25) — each on its OWN console, the way a real NaviCust would.
    // The allowance must then follow over the link on both sides.
    if std::env::var("NSWITCH_PARTS").is_ok() {
        pair.core_mut(0).raw_write_8(LIVE_CFG + 0x35, -1, 1);
        pair.core_mut(1).raw_write_8(LIVE_CFG + 0x60 + 0x25, -1, 1);
        for _ in 0..8 {
            t += 1;
            pair.tick(&[0, 0]);
        }
        let tags = |c: &mut mgba::core::Core| {
            [c.raw_read_8(PSTATE + 0xc, -1), c.raw_read_8(PSTATE + 0xd, -1)]
        };
        let t0 = tags(pair.core_mut(0));
        let t1 = tags(pair.core_mut(1));
        println!("[parts] core0 sees {t0:?}, core1 sees {t1:?} (expect [2,1] on both)");
        assert_eq!(t0, t1, "the two consoles disagree about the teams");
        assert_eq!(t0, [2, 1], "programs did not cross the link");
        println!("[parts] ok");
    }
    let allowance = 3 + (pair.core_mut(0).raw_read_8(PSTATE + 0xc, -1) & 3);
    let programs = [
        pair.core_mut(0).raw_read_8(PSTATE + 0xc, -1),
        pair.core_mut(0).raw_read_8(PSTATE + 0xd, -1),
    ];
    let latched_slots = {
        let c = pair.core_mut(0);
        [c.raw_read_8(PSTATE + 0xe, -1), c.raw_read_8(PSTATE + 0x10, -1)]
    };
    let cfg_flags = {
        let c = pair.core_mut(0);
        [
            c.raw_read_8(LIVE_CFG + 0x25, -1),
            c.raw_read_8(LIVE_CFG + 0x34, -1),
            c.raw_read_8(LIVE_CFG + 0x35, -1),
        ]
    };
    println!(
        "[A] fight running at pair tick {t}, save cfg={cfg_before:?}, allowance={allowance}, programs={programs:?}, cfg(NavChg+1,Spport,NavChg+2)={cfg_flags:?} latched_slots={latched_slots:?}"
    );
    status(pair.core_mut(0), "[A]");

    // NSWITCH_BANNER: at gauge-full, sample the banner's tile data and
    // map entries every frame and report the distinct variants.
    if std::env::var("NSWITCH_BANNER").is_ok() {
        let mut waited = 0;
        while pair.core_mut(0).raw_read_16(BATTLE_STATE + 0x32, -1) & 2 == 0 {
            t += 1;
            waited += 1;
            assert!(waited < 4000, "gauge never filled");
            pair.tick(&[0, 0]);
        }
        let mut tile_variants: Vec<Vec<u8>> = vec![];
        let mut map_variants: Vec<Vec<u8>> = vec![];
        for _ in 0..300 {
            t += 1;
            pair.tick(&[0, 0]);
            let c = pair.core_mut(0);
            let mut tiles = vec![0u8; 0x80];
            c.raw_read_range(0x06000000 + 0xc2c0, -1, &mut tiles);
            if !tile_variants.contains(&tiles) {
                println!("tile variant #{}: {:02x?}", tile_variants.len(), &tiles[..32]);
                tile_variants.push(tiles);
            }
            let mut map = vec![0u8; 0x30];
            c.raw_read_range(0x06000000 + 0xf840, -1, &mut map);
            if !map_variants.contains(&map) {
                println!("map variant #{}: {:02x?}", map_variants.len(), &map);
                map_variants.push(map);
            }
        }
        let dir = std::env::var("PVP_PROBE_DUMP_DIR").unwrap_or_else(|_| ".".into());
        for (i, v) in tile_variants.iter().enumerate() {
            std::fs::write(format!("{dir}/banner_tiles_{i}.bin"), v).unwrap();
        }
        println!("{} tile variants, {} map variants", tile_variants.len(), map_variants.len());
        std::process::exit(0);
    }

    // NSWITCH_VRAM: dump VRAM/palette/OAM before and at gauge-full, plus
    // screenshots, then exit.
    if std::env::var("NSWITCH_VRAM").is_ok() {
        let dir = std::env::var("PVP_PROBE_DUMP_DIR").unwrap_or_else(|_| ".".into());
        let dump = |c: &mut mgba::core::Core, tag: &str| {
            let mut vram = vec![0u8; 0x18000];
            c.raw_read_range(0x06000000, -1, &mut vram);
            std::fs::write(format!("{dir}/vram_{tag}.bin"), &vram).unwrap();
            let mut pal = vec![0u8; 0x400];
            c.raw_read_range(0x05000000, -1, &mut pal);
            std::fs::write(format!("{dir}/pal_{tag}.bin"), &pal).unwrap();
            let mut oam = vec![0u8; 0x400];
            c.raw_read_range(0x07000000, -1, &mut oam);
            std::fs::write(format!("{dir}/oam_{tag}.bin"), &oam).unwrap();
            let mut io = vec![0u8; 0x60];
            c.raw_read_range(0x04000000, -1, &mut io);
            std::fs::write(format!("{dir}/io_{tag}.bin"), &io).unwrap();
        };
        dump(pair.core_mut(0), "fight");
        shot(&mut pair, 0, "vram_fight");
        let mut waited = 0;
        while pair.core_mut(0).raw_read_16(BATTLE_STATE + 0x32, -1) & 2 == 0 {
            t += 1;
            waited += 1;
            assert!(waited < 4000);
            pair.tick(&[0, 0]);
        }
        for _ in 0..30 {
            t += 1;
            pair.tick(&[0, 0]);
        }
        dump(pair.core_mut(0), "full");
        shot(&mut pair, 0, "vram_full");
        std::process::exit(0);
    }

    // NSWITCH_SCAN: locate the EWRAM save image (game-name anchor) and
    // exit.
    if std::env::var("NSWITCH_SCAN").is_ok() {
        let c = pair.core_mut(0);
        let mut ewram = vec![0u8; 0x40000];
        c.raw_read_range(0x02000000, -1, &mut ewram);
        let needle = b"REXE5TOB 20041006 US";
        for i in 0..ewram.len() - needle.len() {
            if &ewram[i..i + needle.len()] == needle {
                println!(
                    "game name at {:#x} -> image base {:#x}",
                    0x02000000 + i,
                    0x02000000 + i - 0x29e0
                );
            }
        }
        std::process::exit(0);
    }

    // Wait for the gauge (modeflags bit 1), then press a key on core 0
    // and wait for the swap/custom to play out.
    // Snapshot player 0's live config at the running fight, before any
    // switch — the reference for "switching back to MegaMan restores
    // exactly what he had".
    let mega_cfg0: Vec<u8> = {
        let c = pair.core_mut(0);
        let mut buf = vec![0u8; 0x60];
        c.raw_read_range(LIVE_CFG, -1, &mut buf);
        buf
    };

    // Press a key on core 0 at once (no gauge wait — switching is meant
    // to work any time), then let the swap play out while watching for a
    // turn restart (phase 4) or a custom screen opening.
    let mut do_step = |pair: &mut mgba_rollback::Link, t: &mut u32, key: u32, label: &str| -> bool {
        let mut waited = 0;
        while pair.core_mut(0).raw_read_8(PHASE_BLOCK, -1) != 8 {
            *t += 1;
            waited += 1;
            if waited > 2000 {
                println!("[{label}] fight phase never came back");
                return false;
            }
            pair.tick(&[0, 0]);
        }
        let gauge = pair.core_mut(0).raw_read_16(BATTLE_STATE + 0x32, -1) & 2 != 0;
        for _ in 0..4 {
            *t += 1;
            pair.tick(&[key, 0]);
        }
        let mut saw_turn_start = false;
        let mut saw_custom = false;
        let mut prev = (0xffu8, 0xffu8, 0xffu8);
        for i in 0..240 {
            *t += 1;
            pair.tick(&[0, 0]);
            let c = pair.core_mut(0);
            let now = (
                c.raw_read_8(PHASE_BLOCK, -1),
                c.raw_read_8(PSTATE, -1),
                c.raw_read_8(LIVE_CFG + 0x29, -1),
            );
            if now != prev && std::env::var("NSWITCH_TRACE").is_ok() {
                println!("    [{label} +{i}] phase={:02x} commit={} navi={}", now.0, now.1, now.2);
                prev = now;
            }
            if c.raw_read_8(PHASE_BLOCK, -1) == 4 {
                saw_turn_start = true;
            }
            if c.raw_read_8(BATTLE_STATE + 0x14, -1) == 4 {
                saw_custom = true;
            }
        }
        println!(
            "[{label}] gauge_was_full={gauge} turn_restarted={saw_turn_start} custom_opened={saw_custom}"
        );
        if saw_turn_start || saw_custom {
            turn_restarts.set(turn_restarts.get() + 1);
        }
        true
    };

    // NSWITCH_KO: bring a team navi out, kill it, and trace what the
    // engine does — the input for the "a team navi dying sends MegaMan
    // back in, only MegaMan dying loses the round" rules.
    if std::env::var("NSWITCH_KO").is_ok() {
        if !do_step(&mut pair, &mut t, 0x200, "KO-setup") {
            std::process::exit(1);
        }
        let team_navi = pair.core_mut(0).raw_read_8(LIVE_CFG + 0x29, -1);
        let mega_cfg: Vec<u8> = mega_cfg0.clone();
        println!("[KO] team navi {team_navi} is out");
        // Real damage, not a poked zero: leave it on 1 HP and let the
        // opponent land the hit (that is how it dies in a real match, and
        // the death then runs inside the same frame as the damage).
        for ci in 0..2 {
            p.debug_set_hp(pair.core_mut(ci), 0, 1);
        }
        let mut died_at = None;
        for i in 0..3000u32 {
            t += 1;
            pair.tick(&[wiggle(t, 0), wiggle(t, 1)]);
            let c = pair.core_mut(0);
            if died_at.is_none() && c.raw_read_8(LIVE_CFG + 0x29, -1) == 0 {
                died_at = Some(i);
                println!("[KO] MegaMan is back at +{i}");
            }
            let ph = c.raw_read_8(PHASE_BLOCK, -1);
            if ph == 0x0c || ph == 0x10 || ph == 0x18 {
                println!("[KO] round ended at +{i} (phase {ph:02x}) — the rule did not hold");
                break;
            }
            if died_at.is_some_and(|d| i > d + 120) {
                break;
            }
        }
        let c = pair.core_mut(0);
        let back = c.raw_read_8(LIVE_CFG + 0x29, -1);
        let phase = c.raw_read_8(PHASE_BLOCK, -1);
        let alive = [
            c.raw_read_8(BATTLE_STATE + 4, -1),
            c.raw_read_8(BATTLE_STATE + 5, -1),
        ];
        let diff: Vec<String> = {
            let mut now = vec![0u8; 0x60];
            c.raw_read_range(LIVE_CFG, -1, &mut now);
            (0..0x60)
                .filter(|&i| now[i] != mega_cfg[i])
                .map(|i| format!("+{i:#04x}: {:02x}->{:02x}", mega_cfg[i], now[i]))
                .collect()
        };
        println!(
            "[KO] after the KO: navi={back} phase={phase:02x} alive={alive:?} mega cfg diff: {}",
            if diff.is_empty() { "clean".into() } else { diff.join(" ") }
        );

        // The downed navi must be out for the round: pressing its key
        // again does nothing.
        do_step(&mut pair, &mut t, 0x200, "KO-retry");
        let retried = pair.core_mut(0).raw_read_8(LIVE_CFG + 0x29, -1);

        // ...but the other slot is still available.
        do_step(&mut pair, &mut t, 0x100, "KO-other");
        let other = pair.core_mut(0).raw_read_8(LIVE_CFG + 0x29, -1);

        // Now KO MegaMan himself: that ends the round.
        do_step(&mut pair, &mut t, 0x100, "KO-back-to-mega");
        let before_mega_ko = pair.core_mut(0).raw_read_8(LIVE_CFG + 0x29, -1);
        for ci in 0..2 {
            p.debug_set_hp(pair.core_mut(ci), 0, 1);
        }
        let mut round_ended = false;
        for i in 0..400u32 {
            t += 1;
            pair.tick(&[wiggle(t, 0), wiggle(t, 1)]);
            if i == 2 {
                for ci in 0..2 {
                    p.debug_set_hp(pair.core_mut(ci), 0, 0);
                }
            }
            let ph = pair.core_mut(0).raw_read_8(PHASE_BLOCK, -1);
            if ph == 0x0c || ph == 0x10 || ph == 0x18 {
                round_ended = true;
                break;
            }
        }
        println!(
            "[KO] retry_blocked={} other_slot={other} mega_out={before_mega_ko} mega_ko_ends_round={round_ended}",
            retried == 0
        );
        let ok = back == 0
            && diff.is_empty()
            && alive == [1, 1]
            && retried == 0
            && other == SLOT_NAVIS[1]
            && before_mega_ko == 0
            && round_ended;
        println!("KO RESULT: {}", if ok { "pass" } else { "FAIL" });
        std::process::exit(if ok { 0 } else { 1 });
    }

    // NSWITCH_SPPORT: give player 0 the Spport program, bring out a team
    // navi, and watch for MegaMan's support attacks landing on the
    // opponent.
    if std::env::var("NSWITCH_SPPORT").is_ok() {
        pair.core_mut(0).raw_write_8(LIVE_CFG + 0x34, -1, 1);
        for _ in 0..8 {
            t += 1;
            pair.tick(&[0, 0]);
        }
        pair.core_mut(0).raw_write_32(0x0203f120, -1, 0);
        pair.core_mut(0).raw_write_32(0x0203f124, -1, 0);
        pair.core_mut(0).raw_write_32(0x0203f128, -1, 0);
        let tag0 = pair.core_mut(0).raw_read_8(PSTATE + 0xc, -1);
        let tag1 = pair.core_mut(1).raw_read_8(PSTATE + 0xc, -1);
        let cfg34 = [
            pair.core_mut(0).raw_read_8(LIVE_CFG + 0x34, -1),
            pair.core_mut(1).raw_read_8(LIVE_CFG + 0x34, -1),
        ];
        println!("[spport] player 0's programs: core0 {tag0:#04x}, core1 {tag1:#04x}; cfg+0x34 {cfg34:?}");
        if !do_step(&mut pair, &mut t, 0x200, "spport-setup") {
            std::process::exit(1);
        }
        println!("[spport] team navi {} is out", pair.core_mut(0).raw_read_8(LIVE_CFG + 0x29, -1));
        let opp_hp = |pair: &mut mgba_rollback::Link| {
            let c = pair.core_mut(0);
            unit_of(c, 1).map(|u| c.raw_read_16(u + 0x24, -1)).unwrap_or(0)
        };
        // line the two up so the covering fire can actually connect
        for ci in 0..2 {
            let c = pair.core_mut(ci);
            if let (Some(a), Some(b)) = (unit_of(c, 0), unit_of(c, 1)) {
                let row = c.raw_read_8(a + 0x13, -1);
                c.raw_write_8(b + 0x13, -1, row);
                c.raw_write_8(b + 0x15, -1, row);
            }
        }
        let before = opp_hp(&mut pair);
        let mut fired = 0;
        let mut prev_state = 0u8;
        let mut prev_cd = 0u8;
        let mut prev_hp = opp_hp(&mut pair);
        for i in 0..1200 {
            t += 1;
            pair.tick(&[0, 0]);
            let c = pair.core_mut(0);
            let cd = c.raw_read_8(PSTATE + 0xa, -1);
            if i % 200 == 0 || (prev_cd == 0 && cd != 0) {
                let u = unit_of(c, 0);
                println!(
                    "[spport] t+{i}: cooldown={cd} unit={:?} state={:?} staged_chip={:?}",
                    u.map(|u| format!("{u:#x}")),
                    u.map(|u| c.raw_read_8(u + 0x18, -1)),
                    u.map(|u| c.raw_read_16(u + 0x36, -1)),
                );
            }
            prev_cd = cd;
            // the covering fire shows up as a B press injected into
            // player 0's exchanged record
            let hp = unit_of(c, 1).map(|u| c.raw_read_16(u + 0x24, -1)).unwrap_or(0);
            if hp != prev_hp {
                fired += 1;
                println!("[spport] support hit #{fired} at t+{i}: opponent {prev_hp} -> {hp}");
                prev_hp = hp;
            }
            let _ = prev_state;
        }
        let after = opp_hp(&mut pair);
        let agree = pair.core_mut(0).raw_read_8(LIVE_CFG + 0x29, -1)
            == pair.core_mut(1).raw_read_8(LIVE_CFG + 0x29, -1);

        println!(
            "[spport] navi-gate passed {} times",
            pair.core_mut(0).raw_read_32(0x0203f128, -1)
        );
        println!("[spport] fired {fired} times, opponent {before} -> {after}, cores agree: {agree}");
        std::process::exit(if fired >= 2 && after < before && agree { 0 } else { 1 });
    }

    let navi = |pair: &mut mgba_rollback::Link| pair.core_mut(0).raw_read_8(LIVE_CFG + 0x29, -1);
    let hp0 = |pair: &mut mgba_rollback::Link| {
        let c = pair.core_mut(0);
        unit_of(c, 0).map(|u| c.raw_read_16(u + 0x24, -1)).unwrap_or(0)
    };

    // B: L brings out slot 1 (change 1 of 3).
    if !do_step(&mut pair, &mut t, 0x200, "B") {
        std::process::exit(1);
    }
    let navi_b = navi(&mut pair);
    status(pair.core_mut(0), "[B] slot 1 out");
    shot(&mut pair, 0, "b_slot1");

    // C: fight, so slot 1 takes damage worth remembering.
    for _ in 0..600 {
        t += 1;
        pair.tick(&[wiggle(t, 0), wiggle(t, 1)]);
    }
    let slot1_hp = hp0(&mut pair);
    println!("[C] slot 1 fought down to {slot1_hp}");

    // D: L again toggles slot 1 off — MegaMan returns (change 2), and
    // must come back byte-identical to how he started.
    let mega_bench = {
        let c = pair.core_mut(0);
        c.raw_read_16(PSTATE + 0x20, -1)
    };
    if !do_step(&mut pair, &mut t, 0x200, "D") {
        std::process::exit(1);
    }
    let navi_d = navi(&mut pair);
    let mega_hp = hp0(&mut pair);
    let mega_diff: Vec<String> = {
        let c = pair.core_mut(0);
        let mut now = vec![0u8; 0x60];
        c.raw_read_range(LIVE_CFG, -1, &mut now);
        (0..0x60)
            .filter(|&i| now[i] != mega_cfg0[i])
            .map(|i| format!("+{i:#04x}: {:02x}->{:02x}", mega_cfg0[i], now[i]))
            .collect()
    };
    println!(
        "[D] MegaMan back as navi {navi_d} at {mega_hp} (benched {mega_bench}); config diff: {}",
        if mega_diff.is_empty() { "clean".into() } else { mega_diff.join(" ") }
    );
    shot(&mut pair, 0, "d_mega_back");

    // E: keep toggling until the allowance is spent (changes 3..N).
    let mut used = 2u8;
    while used < allowance {
        if !do_step(&mut pair, &mut t, 0x200, "E") {
            std::process::exit(1);
        }
        used += 1;
    }
    let navi_e = navi(&mut pair);
    let slot1_hp_back = hp0(&mut pair);
    println!("[E] slot 1 back as navi {navi_e} at {slot1_hp_back} (left on {slot1_hp})");

    // F: the allowance is spent — a fourth change is refused.
    if !do_step(&mut pair, &mut t, 0x100, "F") {
        std::process::exit(1);
    }
    let navi_f = navi(&mut pair);
    let used = pair.core_mut(0).raw_read_8(PSTATE + 4, -1);
    let changes_left = allowance.saturating_sub(used);
    println!("[F] extra change refused: navi still {navi_f}, used {used}/{allowance}");
    status(pair.core_mut(0), "[F] limit reached");

    // G: SELECT still opens the custom screen, and closing it resumes.
    let mut custom_opened = false;
    for _ in 0..4 {
        t += 1;
        pair.tick(&[0x4, 0]);
    }
    for _ in 0..300 {
        t += 1;
        pair.tick(&[0, 0]);
        if pair.core_mut(0).raw_read_8(BATTLE_STATE + 0x14, -1) == 4 {
            custom_opened = true;
            break;
        }
    }
    // Close it: both sides mash A over the chip cursor (L/R can't be in
    // the mash any more — they switch navis).
    let mut closed = false;
    for i in 0..4000u32 {
        t += 1;
        let k = |p: usize| if i % 8 < 3 { 0x1 } else { wiggle(t, p) };
        pair.tick(&[k(0), k(1)]);
        let c = pair.core_mut(0);
        if c.raw_read_8(BATTLE_STATE + 0x14, -1) == 0 && c.raw_read_8(BATTLE_STATE + 0x15, -1) == 0 {
            closed = true;
            break;
        }
    }
    shot(&mut pair, 0, "g_custom");

    // H: still live, both cores still agree, save config untouched.
    let mut live = true;
    let mut prev = pair.core_mut(0).raw_read_32(BATTLE_STATE + 0x60, -1);
    for _ in 0..240 {
        t += 1;
        pair.tick(&[wiggle(t, 0), wiggle(t, 1)]);
        let tick = pair.core_mut(0).raw_read_32(BATTLE_STATE + 0x60, -1);
        live &= tick == prev + 1;
        prev = tick;
    }
    let agree = {
        let a = pair.core_mut(0).raw_read_8(LIVE_CFG + 0x29, -1);
        let b = pair.core_mut(1).raw_read_8(LIVE_CFG + 0x29, -1);
        a == b
    };
    let cfg_after = {
        let c = pair.core_mut(0);
        [c.raw_read_8(0x02007c10, -1), c.raw_read_8(0x02007c11, -1)]
    };
    let cfg_stable = cfg_after == cfg_before;
    status(pair.core_mut(0), "[H] final");

    println!(
        "RESULT: L->{navi_b} L->{navi_d} L->{navi_e} R->{navi_f}(refused) | slot1 HP {slot1_hp}->{slot1_hp_back} | mega {mega_hp} vs benched {mega_bench} cfg={} | changes_left={changes_left} custom={custom_opened} closed={closed} live={live} agree={agree} cfg_stable={cfg_stable} turn_restarts={} digest={:08x}",
        if mega_diff.is_empty() { "clean" } else { "DIRTY" },
        turn_restarts.get(),
        pair.save().unwrap().digest()
    );
    // With an odd allowance the last toggle leaves slot 1 out; with an
    // even one, MegaMan.
    let navi_e_expected = if allowance % 2 == 1 { expect[0] } else { 0 };
    let ok = navi_b == expect[0]
        && navi_d == 0
        && navi_e == navi_e_expected
        && navi_f == navi_e_expected
        && changes_left == 0
        && slot1_hp_back == slot1_hp
        && mega_hp == mega_bench
        && mega_diff.is_empty()
        && custom_opened
        && closed
        && live
        && agree
        && cfg_stable
        && turn_restarts.get() == 0;
    std::process::exit(if ok { 0 } else { 1 });
}
