//! Headless scripted-input driver for BCC RE.
//!
//! Usage: menu_probe <rom> [options]
//!   --save FILE          load battery SRAM from FILE
//!   --script FILE        input script: lines "<frame> <keys>" — from that
//!                        frame on, hold <keys> (e.g. "A", "ST", "A+R", "-").
//!                        Keys: A B SEL ST RI LE UP DO RB LB, "-" = none.
//!   --frames N           run N frames (default 600)
//!   --shot-every N       dump a PNG every N frames
//!   --shot-at F,F,...    dump PNGs at exactly these frames
//!   --dump-dir DIR       where PNGs go (default .)
//!   --sram-out FILE      write battery SRAM to FILE at exit
//!   --ewram-out FILE     write EWRAM (256 KiB) to FILE at exit
//!   --watch ADDR:LEN     print hex when this range changes (repeatable)
//!   --ewram-at F:FILE    dump EWRAM at frame F (repeatable)
//!   --iwram-at F:FILE    dump IWRAM (32 KiB) at frame F (repeatable)
//!   --vram-at F:FILE     dump VRAM + palette RAM at frame F (repeatable);
//!                        writes FILE (96 KiB VRAM) and FILE.pal (1 KiB)
//!   --state-in FILE      load a savestate before running (script frames
//!                        still count from 0)
//!   --state-out FILE     write a savestate at exit
//!   --trap ADDR          log r0-r3 whenever execution reaches ADDR
//!                        (repeatable) — for following the game's own
//!                        copies back to their ROM source
//!   --trace-pc           print PC each time it enters a new 0x1000 page

use std::collections::HashMap;

fn parse_keys(s: &str) -> u32 {
    if s == "-" {
        return 0;
    }
    s.split('+')
        .map(|k| match k {
            "A" => 1 << 0,
            "B" => 1 << 1,
            "SEL" => 1 << 2,
            "ST" => 1 << 3,
            "RI" => 1 << 4,
            "LE" => 1 << 5,
            "UP" => 1 << 6,
            "DO" => 1 << 7,
            "RB" => 1 << 8,
            "LB" => 1 << 9,
            k => panic!("unknown key {k:?}"),
        })
        .fold(0, |a, b| a | b)
}

fn save_png(core: &mgba::core::OwnedCore, path: &str) {
    let Some(buf) = core.video_buffer() else { return };
    let img = image::RgbImage::from_fn(240, 160, |x, y| {
        let off = ((y * 240 + x) * 2) as usize;
        let v = u16::from_le_bytes([buf[off], buf[off + 1]]);
        image::Rgb([
            ((v & 31) as u8) << 3,
            (((v >> 5) & 31) as u8) << 3,
            (((v >> 10) & 31) as u8) << 3,
        ])
    });
    img.save(path).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom_path = args[0].clone();
    let mut opt: HashMap<String, Vec<String>> = HashMap::new();
    let mut i = 1;
    while i < args.len() {
        let key = args[i].trim_start_matches("--").to_string();
        if key == "trace-pc" {
            opt.entry(key).or_default().push("1".into());
            i += 1;
        } else {
            opt.entry(key).or_default().push(args[i + 1].clone());
            i += 2;
        }
    }
    let one = |k: &str| opt.get(k).map(|v| v[0].clone());

    let mut core = mgba::core::OwnedCore::new_gba("bcc-probe", &mgba::core::Options::default()).unwrap();
    core.enable_video_buffer();
    core.load_rom(mgba::vfile::VFile::from_vec(std::fs::read(&rom_path).unwrap()))
        .unwrap();
    if let Some(save) = one("save") {
        core.load_save(mgba::vfile::VFile::from_vec(std::fs::read(save).unwrap()))
            .unwrap();
    }
    core.set_rtc_fixed(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000));
    core.reset();
    if let Some(path) = one("state-in") {
        // Safety: the file is a state this probe wrote via `state-out`
        // from the same rom on the same build.
        let state = unsafe { mgba::state::State::from_slice(&std::fs::read(path).unwrap()) };
        core.load_state(&state).unwrap();
    }

    if let Some(traps) = opt.get("trap") {
        let addrs: Vec<u32> = traps
            .iter()
            .map(|a| u32::from_str_radix(a.trim_start_matches("0x"), 16).unwrap())
            .collect();
        core.set_traps(
            addrs
                .iter()
                .map(|&addr| {
                    let t: (u32, Box<dyn Fn(&mut mgba::core::Core)>) = (
                        addr,
                        Box::new(move |core: &mut mgba::core::Core| {
                            let g = core.gba().cpu();
                            println!(
                                "trap {addr:08x}: r0={:08x} r1={:08x} r2={:08x} r3={:08x} \
                                 r4={:08x} r5={:08x} r6={:08x} lr={:08x}",
                                g.gpr(0) as u32,
                                g.gpr(1) as u32,
                                g.gpr(2) as u32,
                                g.gpr(3) as u32,
                                g.gpr(4) as u32,
                                g.gpr(5) as u32,
                                g.gpr(6) as u32,
                                g.gpr(14) as u32,
                            );
                        }),
                    );
                    t
                })
                .collect(),
        );
    }

    let mut script: Vec<(u32, u32)> = Vec::new();
    if let Some(path) = one("script") {
        for line in std::fs::read_to_string(path).unwrap().lines() {
            let line = line.split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            let mut it = line.split_whitespace();
            let frame: u32 = it.next().unwrap().parse().unwrap();
            let keys = parse_keys(it.next().unwrap());
            script.push((frame, keys));
        }
        script.sort_by_key(|e| e.0);
    }

    let frames: u32 = one("frames").map(|v| v.parse().unwrap()).unwrap_or(600);
    let dump_dir = one("dump-dir").unwrap_or_else(|| ".".into());
    let shot_every: u32 = one("shot-every").map(|v| v.parse().unwrap()).unwrap_or(0);
    let shot_at: Vec<u32> = one("shot-at")
        .map(|v| v.split(',').map(|x| x.parse().unwrap()).collect())
        .unwrap_or_default();
    let watches: Vec<(u32, usize)> = opt
        .get("watch")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let (a, l) = w.split_once(':').unwrap();
                    (
                        u32::from_str_radix(a.trim_start_matches("0x"), 16).unwrap(),
                        usize::from_str_radix(l.trim_start_matches("0x"), 16).unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let trace_pc = opt.contains_key("trace-pc");
    let vram_at: Vec<(u32, String)> = opt
        .get("vram-at")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let (f, p) = w.split_once(':').unwrap();
                    (f.parse().unwrap(), p.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    let ewram_at: Vec<(u32, String)> = opt
        .get("ewram-at")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let (f, p) = w.split_once(':').unwrap();
                    (f.parse().unwrap(), p.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    let iwram_at: Vec<(u32, String)> = opt
        .get("iwram-at")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let (f, p) = w.split_once(':').unwrap();
                    (f.parse().unwrap(), p.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    let mut watch_prev: Vec<Vec<u8>> = watches.iter().map(|&(_, l)| vec![0; l]).collect();
    let mut last_page = 0u32;
    let mut cur_keys = 0u32;
    let mut si = 0usize;

    for f in 0..frames {
        while si < script.len() && script[si].0 <= f {
            cur_keys = script[si].1;
            si += 1;
        }
        core.set_keys(cur_keys);
        core.run_frame();

        for (wi, &(addr, len)) in watches.iter().enumerate() {
            let mut buf = vec![0u8; len];
            core.raw_read_range(addr, -1, &mut buf);
            if buf != watch_prev[wi] {
                println!("[{f:5}] {addr:08x}: {}", hex(&buf));
                watch_prev[wi] = buf;
            }
        }
        if trace_pc {
            let pc = core.gba().cpu().thumb_pc() & !0xfff;
            if pc != last_page {
                println!("[{f:5}] pc page {pc:08x}");
                last_page = pc;
            }
        }
        if (shot_every != 0 && f % shot_every == 0) || shot_at.contains(&f) {
            save_png(&core, &format!("{dump_dir}/f{f:06}.png"));
        }
        for (df, path) in &vram_at {
            if *df == f {
                let mut vram = vec![0u8; 0x18000];
                core.raw_read_range(0x06000000, -1, &mut vram);
                std::fs::write(path, vram).unwrap();
                let mut pal = vec![0u8; 0x400];
                core.raw_read_range(0x05000000, -1, &mut pal);
                std::fs::write(format!("{path}.pal"), pal).unwrap();
            }
        }
        for (df, path) in &ewram_at {
            if *df == f {
                let mut buf = vec![0u8; 0x40000];
                core.raw_read_range(0x02000000, -1, &mut buf);
                std::fs::write(path, buf).unwrap();
            }
        }
        for (df, path) in &iwram_at {
            if *df == f {
                let mut buf = vec![0u8; 0x8000];
                core.raw_read_range(0x03000000, -1, &mut buf);
                std::fs::write(path, buf).unwrap();
            }
        }
    }

    save_png(&core, &format!("{dump_dir}/final.png"));
    if let Some(path) = one("state-out") {
        std::fs::write(path, core.save_state().unwrap().as_slice()).unwrap();
    }
    if let Some(path) = one("sram-out") {
        std::fs::write(path, core.savedata_clone().unwrap_or_default()).unwrap();
    }
    if let Some(path) = one("ewram-out") {
        let mut buf = vec![0u8; 0x40000];
        core.raw_read_range(0x02000000, -1, &mut buf);
        std::fs::write(path, buf).unwrap();
    }
    println!("done at frame {frames}, pc={:08x}", core.gba().cpu().thumb_pc());
}

fn hex(buf: &[u8]) -> String {
    buf.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}
