//! Headless scripted-input driver for Operate Shooting Star RE.
//!
//! Drives a linked pair the way priming does, with observation and poke
//! points between ticks — BN5DS's probe of the same name, aimed at this
//! cart. Everything here is RE tooling; nothing it does ships.
//!
//! Usage: menu_probe <rom> [options]
//!   --save FILE          both consoles' save memory (console 1 overridable
//!                        with --save2)
//!   --save2 FILE         console 1's save memory
//!   --frames N           run N frames (default 600)
//!   --walk               drive the scripted menu walk (host on console 0,
//!                        join on console 1)
//!   --prime              run the crate's real priming and report its cost
//!   --prime-only         the same, then stop and dump the primed screens
//!   --rng-seed HEX       first word of the match seed priming reseeds
//!                        the game's rng from (default 0)
//!   --script FILE        input script: lines "<frame> <seat> <input>";
//!                        seat 0/1/b; input is keys "A+ST" (A B SEL ST RI
//!                        LE UP DO RB LB X Y), a stylus tap "T:x,y", or
//!                        "-" for none. From that frame on, hold it.
//!   --watch ADDR:LEN     print hex when this main-RAM range changes on
//!                        either console (repeatable)
//!   --ram-at F,F,...     dump both consoles' full main RAM at these
//!                        frames (repeatable) as ram-f<F>-s<seat>.bin
//!   --poke F:SEAT:ADDR:HEXBYTES  write bytes at frame F (repeatable);
//!                        seat 0/1/b
//!   --splice F:SEAT:DUMP:A-B  at frame F, write main RAM A..B out of
//!                        another run's --ram-at dump (repeatable) — a
//!                        poke too big to spell out, for bisecting one
//!                        run's state against another's. `@FILE` in
//!                        place of the range reads whitespace-separated
//!                        ranges from a file.
//!   --shot-at F,F,...    dump both consoles' screens as PNGs
//!   --shot-every N       dump PNGs every N frames
//!   --dump-dir DIR       where dumps go (default .)
//!   --status-every N     print connected() and both PCs every N frames
//!   --save-out PREFIX    dump both consoles' save memory at the end as
//!                        PREFIX-s<seat>.sav
//!   --diff A-B:FILE      over frames A..=B, count per-address changes in
//!                        console 0's main RAM; at B, write addresses that
//!                        changed 1-3 times (with first/last values and
//!                        change frames) to FILE (repeatable)
//!   --steppers A-B:FILE  like --diff, but write addresses that changed on
//!                        at least a third of the window's frames — fade
//!                        counters, typewriters (repeatable)
//!   --warp ADDR:STEP     every frame, add STEP to the u32 at ADDR on both
//!                        consoles (repeatable) — timer acceleration
//!
//! The cover-style instruments, on the pair. SEAT is 0/1/b:
//!   --redirect SEAT:S:T  whenever that console's ARM9 reaches S, jump
//!                        to T instead (repeatable)
//!   --guard SEAT:S:T:ADDR:VAL[:MAX]  the same, but only while the u32
//!                        at ADDR reads VAL, and at most MAX times if
//!                        given — for an anchor whose effect lands
//!                        asynchronously, where firing again before it
//!                        does just restarts it (repeatable)
//!   --site-poke SEAT:S:ADDR:HEXBYTES  write bytes when S is reached,
//!                        before any redirect on the same site
//!   --setreg SEAT:S:R:VAL  set register R when S is reached, before
//!                        any redirect on the same site
//!   --probe SEAT:ADDR    print frame, r0-r7 and lr the first few times
//!                        ADDR is reached
//!   --probe-limit N      how many times each probe prints (default 8)
//!   --probe-from F       don't start probing until frame F, so a probe
//!                        can sample a screen that only exists later
//!   --cover SEAT:A-B:FILE  record every ARM9 address that console
//!                        executes in frames A..=B (repeatable)
//!   --pcwatch SEAT:ADDR  trap every address in --range and print the
//!                        first pc reached after that main-RAM byte
//!                        changes — a write watch for a store whose
//!                        base register no literal pool names
//!   --range LO-HI        the cover recording range (default cart code)
//!
//! The same instruments on the **ARM7**, which is where the cartridge's
//! backup server runs — the save's own clock lives there, not in
//! anything the ARM9 sites above can reach:
//!   --redirect7 SEAT:S:T   ARM7 jump (repeatable)
//!   --setreg7 SEAT:S:R:VAL set an ARM7 register when S is reached
//!   --probe7 SEAT:ADDR     print frame, r0-r7 and lr at an ARM7 site
//!   --cover7 SEAT:A-B:FILE record every ARM7 address executed in
//!                        frames A..=B (repeatable)
//!   --range7 LO-HI       the ARM7 cover range (default ARM7 WRAM)
//!   --ram7-at F:ADDR:LEN dump ARM7-visible memory at frame F

use std::collections::HashMap;

use tango_backend_melonds::Link;
use tango_match::{HostInput, Link as _};

/// One scripted step of the classic walk: hold `input` for `frames`.
struct Step {
    frames: u32,
    input: HostInput,
}

const fn hold(frames: u32, keys: u32) -> Step {
    Step {
        frames,
        input: HostInput { keys, touch: None },
    }
}

#[allow(dead_code)]
const fn tap(frames: u32, x: u16, y: u16) -> Step {
    Step {
        frames,
        input: HostInput {
            keys: 0,
            touch: Some((x, y)),
        },
    }
}

const A: u32 = tango_match::keys::A;
const START: u32 = tango_match::keys::START;
const DOWN: u32 = tango_match::keys::DOWN;

/// The route both consoles share, kept as RE ground truth: power-on to
/// the host/join pick. START skips the opening movie and confirms the
/// title's own menu (whose cursor already sits on CONTINUE for a cart
/// with a save), then opens the field's START menu; five DOWNs reach its
/// Network entry; the Network menu insists on a save before it will do
/// anything, and one row down from its Trade entry is Net Battle
/// (Practice) — practice deliberately, since the real thing spends the
/// players' own records on the result.
const TO_HOST_PICK: &[Step] = &[
    hold(60, 0),
    // The movie's skip is a press, not a hold, and the title's menu
    // wants another; mashing covers both without knowing when each
    // screen arrives.
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    hold(8, START),
    hold(12, 0),
    // The field, with the save loaded.
    hold(120, 0),
    hold(8, START),
    hold(32, 0),
    // Down five rows to Network.
    hold(8, DOWN),
    hold(17, 0),
    hold(8, DOWN),
    hold(17, 0),
    hold(8, DOWN),
    hold(17, 0),
    hold(8, DOWN),
    hold(17, 0),
    hold(8, DOWN),
    hold(17, 0),
    hold(15, 0),
    hold(8, A),
    // "First, let's save" — and then the save itself.
    hold(332, 0),
    hold(8, A),
    hold(142, 0),
    // Down one row, from Trade to Net Battle (Practice).
    hold(8, DOWN),
    hold(42, 0),
    hold(8, A),
    hold(242, 0),
];

/// Console 0 hosts: take the parent row, start the wireless, then accept
/// the joiner's request when it arrives.
const HOST_TAIL: &[Step] = &[
    hold(8, A),
    hold(192, 0),
    hold(8, A),
    hold(1742, 0),
    hold(8, A),
    hold(3592, 0),
];

/// Console 1 joins: take the child row, start the wireless, then pick
/// the host out of the list it finds.
const JOIN_TAIL: &[Step] = &[
    hold(8, DOWN),
    hold(32, 0),
    hold(8, A),
    hold(152, 0),
    hold(8, A),
    hold(842, 0),
    hold(8, A),
    hold(4492, 0),
];

/// Expand a step list into one input per frame.
fn expand(steps: &[&[Step]]) -> Vec<HostInput> {
    let mut out = Vec::new();
    for list in steps {
        for step in *list {
            out.extend(std::iter::repeat_n(step.input, step.frames as usize));
        }
    }
    out
}

fn parse_input(s: &str) -> HostInput {
    if s == "-" {
        return HostInput::default();
    }
    if let Some(xy) = s.strip_prefix("T:") {
        let (x, y) = xy.split_once(',').unwrap();
        return HostInput {
            keys: 0,
            touch: Some((x.parse().unwrap(), y.parse().unwrap())),
        };
    }
    let keys = s
        .split('+')
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
            "X" => 1 << 10,
            "Y" => 1 << 11,
            "MIC" => 1 << 12,
            k => panic!("unknown key {k:?}"),
        })
        .fold(0, |a, b| a | b);
    HostInput { keys, touch: None }
}

fn parse_hex(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap()
}

fn ram_range(link: &mut Link, seat: usize, addr: u32, len: usize) -> Vec<u8> {
    let ram = link.console(seat).main_ram();
    let mask = ram.len() - 1;
    let base = (addr as usize - 0x0200_0000) & mask;
    ram[base..base + len].to_vec()
}

fn save_shot(link: &mut Link, seat: usize, path: &str) {
    let Some((top, bottom)) = link.console(seat).framebuffers() else {
        return;
    };
    let mut img = image::RgbImage::new(256, 384);
    for (half, screen) in [top, bottom].into_iter().enumerate() {
        for (i, &pixel) in screen.iter().enumerate() {
            let [r, g, b, _] = tango_backend_melonds::unpacked_bgr666_to_rgba8(pixel);
            img.put_pixel((i % 256) as u32, (half * 192 + i / 256) as u32, image::Rgb([r, g, b]));
        }
    }
    img.save(path).unwrap();
}

fn hex(buf: &[u8]) -> String {
    buf.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    // Priming's own log lines, at RUST_LOG=info.
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom_path = args[0].clone();
    let mut opt: HashMap<String, Vec<String>> = HashMap::new();
    let mut i = 1;
    while i < args.len() {
        let key = args[i].trim_start_matches("--").to_string();
        if key == "walk" || key == "prime" || key == "prime-only" {
            opt.entry(key).or_default().push("1".into());
            i += 1;
        } else {
            opt.entry(key).or_default().push(args[i + 1].clone());
            i += 2;
        }
    }
    let one = |k: &str| opt.get(k).map(|v| v[0].clone());

    let rom = std::fs::read(&rom_path).unwrap();
    let save = std::fs::read(one("save").expect("--save is required")).unwrap();
    let save2 = one("save2").map(|p| std::fs::read(p).unwrap());
    // 2026-07-01T12:00:00Z, fixed so runs are comparable.
    let mut link = Link::new(
        &rom,
        [Some(&save), Some(save2.as_deref().unwrap_or(&save))],
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_782_907_200),
    )
    .unwrap();

    let save_out = |link: &mut Link| {
        if let Some(prefix) = one("save-out") {
            for seat in 0..2 {
                std::fs::write(format!("{prefix}-s{seat}.sav"), link.console(seat).save_memory()).unwrap();
            }
        }
    };

    if opt.contains_key("prime") || opt.contains_key("prime-only") {
        let mut rng_seed = [0u8; 16];
        if let Some(v) = one("rng-seed") {
            rng_seed[..4].copy_from_slice(&parse_hex(&v).to_le_bytes());
        }
        let started = std::time::Instant::now();
        match tango_gamesupport_exeoss::pvp::priming::walk(&mut link, rng_seed, &tango_match::telemetry::EventSink::new(), None) {
            Ok(()) => println!(
                "primed in {:.1}s wall, connected={}",
                started.elapsed().as_secs_f64(),
                link.connected()
            ),
            Err(e) => println!("priming failed after {:.1}s wall: {e}", started.elapsed().as_secs_f64()),
        }
        if opt.contains_key("prime-only") {
            for seat in 0..2 {
                save_shot(
                    &mut link,
                    seat,
                    &format!("{}/primed-s{seat}.png", one("dump-dir").unwrap_or_else(|| ".".into())),
                );
            }
            save_out(&mut link);
            return;
        }
    }

    // The cover-style instruments, installed after any priming so they
    // survive it (the walk clears the consoles' traps when it finishes).
    let seats_of = |s: &str| -> Vec<usize> {
        match s {
            "b" => vec![0, 1],
            s => vec![s.parse().unwrap()],
        }
    };
    let frame_now = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let probe_limit: usize = one("probe-limit").map(|v| v.parse().unwrap()).unwrap_or(8);
    let probe_from: u32 = one("probe-from").map(|v| v.parse().unwrap()).unwrap_or(0);
    let fired: std::sync::Arc<std::sync::Mutex<HashMap<(usize, u32), usize>>> = Default::default();

    // (seat, site) -> bytes to write / registers to set when reached.
    let mut site_pokes: HashMap<(usize, u32), Vec<(u32, Vec<u8>)>> = HashMap::new();
    for w in opt.get("site-poke").into_iter().flatten() {
        let mut it = w.split(':');
        let seats = seats_of(it.next().unwrap());
        let site = parse_hex(it.next().unwrap());
        let addr = parse_hex(it.next().unwrap());
        let bytes = it.next().unwrap();
        let bytes: Vec<u8> = (0..bytes.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&bytes[i..i + 2], 16).unwrap())
            .collect();
        for seat in seats {
            site_pokes.entry((seat, site)).or_default().push((addr, bytes.clone()));
        }
    }
    let mut site_regs: HashMap<(usize, u32), Vec<(u32, u32)>> = HashMap::new();
    for w in opt.get("setreg").into_iter().flatten() {
        let mut it = w.split(':');
        let seats = seats_of(it.next().unwrap());
        let site = parse_hex(it.next().unwrap());
        let reg: u32 = it.next().unwrap().parse().unwrap();
        let val = parse_hex(it.next().unwrap());
        for seat in seats {
            site_regs.entry((seat, site)).or_default().push((reg, val));
        }
    }

    // Per seat: (site, guard, max fires, target). A plain redirect is a
    // guard that always passes, with no limit.
    let mut jumps: [Vec<(u32, Option<(u32, u32)>, Option<usize>, u32)>; 2] = [Vec::new(), Vec::new()];
    for w in opt.get("redirect").into_iter().flatten() {
        let mut it = w.split(':');
        let seats = seats_of(it.next().unwrap());
        let site = parse_hex(it.next().unwrap());
        let target = parse_hex(it.next().unwrap());
        for seat in seats {
            jumps[seat].push((site, None, None, target));
        }
    }
    for w in opt.get("guard").into_iter().flatten() {
        let mut it = w.split(':');
        let seats = seats_of(it.next().unwrap());
        let site = parse_hex(it.next().unwrap());
        let target = parse_hex(it.next().unwrap());
        let addr = parse_hex(it.next().unwrap());
        let val = parse_hex(it.next().unwrap());
        let max = it.next().map(|m| m.parse().unwrap());
        for seat in seats {
            jumps[seat].push((site, Some((addr, val)), max, target));
        }
    }
    let mut probes: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    for w in opt.get("probe").into_iter().flatten() {
        let (s, addr) = w.split_once(':').unwrap();
        for seat in seats_of(s) {
            probes[seat].push(parse_hex(addr));
        }
    }
    // Per seat: (start, end, path) coverage windows over the ARM9.
    let mut covers: [Vec<(u32, u32, String)>; 2] = [Vec::new(), Vec::new()];
    for w in opt.get("cover").into_iter().flatten() {
        let mut it = w.splitn(3, ':');
        let seats = seats_of(it.next().unwrap());
        let (a, b) = it.next().unwrap().split_once('-').unwrap();
        let path = it.next().unwrap();
        for seat in seats {
            covers[seat].push((a.parse().unwrap(), b.parse().unwrap(), path.to_string()));
        }
    }
    let cover_range = one("range")
        .map(|r| {
            let (a, b) = r.split_once('-').unwrap();
            (parse_hex(a), parse_hex(b))
        })
        .unwrap_or((0x0200_0000, 0x0214_0000));
    let recordings: [std::sync::Arc<std::sync::atomic::AtomicBool>; 2] = Default::default();
    let hits: [std::sync::Arc<std::sync::Mutex<std::collections::HashSet<u32>>>; 2] = Default::default();

    // (seat, addr): watch a main-RAM byte from *inside* the trap
    // dispatch, so a change is reported against the code that made it.
    // Every address in `--range` gets a trap that reads the byte and
    // prints when it moves — the first instruction to run after the
    // store is what names the store.
    let mut pcwatches: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    for w in opt.get("pcwatch").into_iter().flatten() {
        let (s, addr) = w.split_once(':').unwrap();
        for seat in seats_of(s) {
            pcwatches[seat].push(parse_hex(addr));
        }
    }

    for seat in 0..2 {
        let mut traps: Vec<(u32, Box<dyn FnMut(&mut tango_backend_melonds::Nds)>)> = Vec::new();
        for &addr in &pcwatches[seat] {
            let last = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(u32::MAX));
            let frame_now = frame_now.clone();
            traps.extend((cover_range.0..cover_range.1).step_by(2).map(|site| {
                let last = last.clone();
                let frame_now = frame_now.clone();
                let f: Box<dyn FnMut(&mut tango_backend_melonds::Nds)> = Box::new(move |nds| {
                    let now = nds.read8(addr) as u32;
                    let was = last.swap(now, std::sync::atomic::Ordering::Relaxed);
                    if was != now && was != u32::MAX {
                        println!(
                            "pcwatch s{seat} {addr:08x}: {was:02x} -> {now:02x} \
                             [f{}] first pc after: {:08x}",
                            frame_now.load(std::sync::atomic::Ordering::Relaxed),
                            nds.pc()
                        );
                    }
                });
                (site, f)
            }));
        }
        if !covers[seat].is_empty() {
            traps.extend((cover_range.0..cover_range.1).step_by(2).map(|addr| {
                let recording = recordings[seat].clone();
                let hits = hits[seat].clone();
                let f: Box<dyn FnMut(&mut tango_backend_melonds::Nds)> = Box::new(move |nds| {
                    if recording.load(std::sync::atomic::Ordering::Relaxed) {
                        hits.lock().unwrap().insert(nds.pc());
                    }
                });
                (addr, f)
            }));
        }
        // One handler per site: pokes and setregs first, then the first
        // passing jump.
        let mut sites: Vec<u32> = jumps[seat].iter().map(|&(s, ..)| s).collect();
        sites.extend(site_pokes.keys().filter(|(s, _)| *s == seat).map(|&(_, site)| site));
        sites.extend(site_regs.keys().filter(|(s, _)| *s == seat).map(|&(_, site)| site));
        sites.sort_unstable();
        sites.dedup();
        for site in sites {
            traps.retain(|(addr, _)| *addr != site);
            let pokes = site_pokes.get(&(seat, site)).cloned().unwrap_or_default();
            let regs = site_regs.get(&(seat, site)).cloned().unwrap_or_default();
            let mut rules: Vec<(Option<(u32, u32)>, Option<usize>, u32, usize)> = jumps[seat]
                .iter()
                .filter(|&&(s, ..)| s == site)
                .map(|&(_, guard, max, target)| (guard, max, target, 0))
                .collect();
            let fired = fired.clone();
            traps.push((
                site,
                Box::new(move |nds| {
                    let apply = |nds: &mut tango_backend_melonds::Nds| {
                        for &(addr, ref bytes) in &pokes {
                            for (off, b) in bytes.iter().enumerate() {
                                nds.write8(addr + off as u32, *b);
                            }
                        }
                        for &(reg, val) in &regs {
                            nds.set_reg(reg, val);
                        }
                    };
                    if rules.is_empty() {
                        apply(nds);
                        return;
                    }
                    for (guard, max, target, count) in rules.iter_mut() {
                        if let Some((addr, val)) = *guard {
                            if nds.read32(addr) != val {
                                continue;
                            }
                        }
                        if max.is_some_and(|max| *count >= max) {
                            continue;
                        }
                        *count += 1;
                        apply(nds);
                        *fired.lock().unwrap().entry((seat, site)).or_default() += 1;
                        nds.jump_here(*target);
                        return;
                    }
                }),
            ));
        }
        for &site in &probes[seat] {
            traps.retain(|(addr, _)| *addr != site);
            let mut seen = 0usize;
            let frame_now = frame_now.clone();
            traps.push((
                site,
                Box::new(move |nds| {
                    let frame = frame_now.load(std::sync::atomic::Ordering::Relaxed);
                    if seen >= probe_limit || frame < probe_from {
                        return;
                    }
                    seen += 1;
                    let regs: Vec<String> = (0..8).map(|i| format!("r{i}={:08x}", nds.reg(i))).collect();
                    println!(
                        "probe s{seat} {site:08x} #{seen} [f{frame}]: {} lr={:08x}",
                        regs.join(" "),
                        nds.reg(14)
                    );
                }),
            ));
        }
        if !traps.is_empty() {
            link.console(seat).set_traps(traps);
        }
    }

    // The same three instruments on the ARM7. Kept separate rather than
    // folded into the loop above because every call they make is a
    // different one — the two processors have their own registers, their
    // own PC and their own trap list.
    let mut jumps7: [Vec<(u32, u32)>; 2] = [Vec::new(), Vec::new()];
    for w in opt.get("redirect7").into_iter().flatten() {
        let mut it = w.split(':');
        let seats = seats_of(it.next().unwrap());
        let site = parse_hex(it.next().unwrap());
        let target = parse_hex(it.next().unwrap());
        for seat in seats {
            jumps7[seat].push((site, target));
        }
    }
    let mut regs7: HashMap<(usize, u32), Vec<(u32, u32)>> = HashMap::new();
    for w in opt.get("setreg7").into_iter().flatten() {
        let mut it = w.split(':');
        let seats = seats_of(it.next().unwrap());
        let site = parse_hex(it.next().unwrap());
        let reg: u32 = it.next().unwrap().parse().unwrap();
        let val = parse_hex(it.next().unwrap());
        for seat in seats {
            regs7.entry((seat, site)).or_default().push((reg, val));
        }
    }
    let mut probes7: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    for w in opt.get("probe7").into_iter().flatten() {
        let (s, addr) = w.split_once(':').unwrap();
        for seat in seats_of(s) {
            probes7[seat].push(parse_hex(addr));
        }
    }
    let mut covers7: [Vec<(u32, u32, String)>; 2] = [Vec::new(), Vec::new()];
    for w in opt.get("cover7").into_iter().flatten() {
        let mut it = w.splitn(3, ':');
        let seats = seats_of(it.next().unwrap());
        let (a, b) = it.next().unwrap().split_once('-').unwrap();
        let path = it.next().unwrap();
        for seat in seats {
            covers7[seat].push((a.parse().unwrap(), b.parse().unwrap(), path.to_string()));
        }
    }
    // The ARM7's own WRAM, which is where the cartridge backup server
    // is relocated to.
    let cover7_range = one("range7")
        .map(|r| {
            let (a, b) = r.split_once('-').unwrap();
            (parse_hex(a), parse_hex(b))
        })
        .unwrap_or((0x037f_8000, 0x0381_0000));
    let recordings7: [std::sync::Arc<std::sync::atomic::AtomicBool>; 2] = Default::default();
    let hits7: [std::sync::Arc<std::sync::Mutex<std::collections::HashSet<u32>>>; 2] = Default::default();

    for seat in 0..2 {
        let mut traps: Vec<(u32, Box<dyn FnMut(&mut tango_backend_melonds::Nds)>)> = Vec::new();
        if !covers7[seat].is_empty() {
            traps.extend((cover7_range.0..cover7_range.1).step_by(2).map(|addr| {
                let recording = recordings7[seat].clone();
                let hits = hits7[seat].clone();
                let f: Box<dyn FnMut(&mut tango_backend_melonds::Nds)> = Box::new(move |nds| {
                    if recording.load(std::sync::atomic::Ordering::Relaxed) {
                        hits.lock().unwrap().insert(nds.arm7_pc());
                    }
                });
                (addr, f)
            }));
        }
        let mut sites: Vec<u32> = jumps7[seat].iter().map(|&(s, _)| s).collect();
        sites.extend(regs7.keys().filter(|(s, _)| *s == seat).map(|&(_, site)| site));
        sites.sort_unstable();
        sites.dedup();
        for site in sites {
            traps.retain(|(addr, _)| *addr != site);
            let regs = regs7.get(&(seat, site)).cloned().unwrap_or_default();
            let target = jumps7[seat].iter().find(|&&(s, _)| s == site).map(|&(_, t)| t);
            traps.push((
                site,
                Box::new(move |nds: &mut tango_backend_melonds::Nds| {
                    for &(reg, val) in &regs {
                        nds.arm7_set_reg(reg, val);
                    }
                    if let Some(target) = target {
                        nds.arm7_jump_here(target);
                    }
                }),
            ));
        }
        for &site in &probes7[seat] {
            traps.retain(|(addr, _)| *addr != site);
            let mut seen = 0usize;
            let frame_now = frame_now.clone();
            traps.push((
                site,
                Box::new(move |nds| {
                    let frame = frame_now.load(std::sync::atomic::Ordering::Relaxed);
                    if seen >= probe_limit || frame < probe_from {
                        return;
                    }
                    seen += 1;
                    let regs: Vec<String> = (0..8).map(|i| format!("r{i}={:08x}", nds.arm7_reg(i))).collect();
                    println!(
                        "probe7 s{seat} {site:08x} #{seen} [f{frame}]: {} lr={:08x}",
                        regs.join(" "),
                        nds.arm7_reg(14)
                    );
                }),
            ));
        }
        if !traps.is_empty() {
            link.console(seat).set_traps7(traps);
        }
    }

    // (frame, addr, len, path): ARM7-visible memory, for disassembling
    // code the ARM9's main-RAM dump can't reach.
    let ram7_at: Vec<(u32, u32, usize)> = opt
        .get("ram7-at")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let mut it = w.split(':');
                    let frame = it.next().unwrap().parse().unwrap();
                    let addr = parse_hex(it.next().unwrap());
                    let len = parse_hex(it.next().unwrap()) as usize;
                    (frame, addr, len)
                })
                .collect()
        })
        .unwrap_or_default();

    let walk: [Vec<HostInput>; 2] = if opt.contains_key("walk") {
        [expand(&[TO_HOST_PICK, HOST_TAIL]), expand(&[TO_HOST_PICK, JOIN_TAIL])]
    } else {
        [Vec::new(), Vec::new()]
    };

    // (frame, seat, input), seat 2 = both.
    let mut script: Vec<(u32, usize, HostInput)> = Vec::new();
    if let Some(path) = one("script") {
        for line in std::fs::read_to_string(path).unwrap().lines() {
            let line = line.split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            let mut it = line.split_whitespace();
            let frame: u32 = it.next().unwrap().parse().unwrap();
            let seat = match it.next().unwrap() {
                "b" => 2,
                s => s.parse().unwrap(),
            };
            script.push((frame, seat, parse_input(it.next().unwrap())));
        }
        script.sort_by_key(|e| e.0);
    }

    let frames: u32 = one("frames").map(|v| v.parse().unwrap()).unwrap_or(600);
    let dump_dir = one("dump-dir").unwrap_or_else(|| ".".into());
    let shot_every: u32 = one("shot-every").map(|v| v.parse().unwrap()).unwrap_or(0);
    let shot_at: Vec<u32> = one("shot-at")
        .map(|v| v.split(',').map(|x| x.parse().unwrap()).collect())
        .unwrap_or_default();
    let ram_at: Vec<u32> = opt
        .get("ram-at")
        .map(|v| {
            v.iter()
                .flat_map(|x| x.split(','))
                .map(|x| x.parse().unwrap())
                .collect()
        })
        .unwrap_or_default();
    let status_every: u32 = one("status-every").map(|v| v.parse().unwrap()).unwrap_or(0);
    let watches: Vec<(u32, usize)> = opt
        .get("watch")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let (a, l) = w.split_once(':').unwrap();
                    (parse_hex(a), parse_hex(l) as usize)
                })
                .collect()
        })
        .unwrap_or_default();
    // (frame, seat, addr, bytes)
    let pokes: Vec<(u32, usize, u32, Vec<u8>)> = opt
        .get("poke")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let mut it = w.split(':');
                    let frame = it.next().unwrap().parse().unwrap();
                    let seat = match it.next().unwrap() {
                        "b" => 2,
                        s => s.parse().unwrap(),
                    };
                    let addr = parse_hex(it.next().unwrap());
                    let bytes = it.next().unwrap();
                    let bytes = (0..bytes.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&bytes[i..i + 2], 16).unwrap())
                        .collect();
                    (frame, seat, addr, bytes)
                })
                .collect()
        })
        .unwrap_or_default();

    // (frame, seat, dump, ranges): pieces of another run's main-RAM
    // dump, written over this one at that frame — a poke too big to
    // spell out on a command line, which is what bisecting one run's
    // state against another's takes.
    let splices: Vec<(u32, usize, Vec<u8>, Vec<(u32, u32)>)> = opt
        .get("splice")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let mut it = w.split(':');
                    let frame = it.next().unwrap().parse().unwrap();
                    let seat = match it.next().unwrap() {
                        "b" => 2,
                        s => s.parse().unwrap(),
                    };
                    let dump = std::fs::read(it.next().unwrap()).unwrap();
                    // One range, or `@file` naming a file of them — a
                    // bisect over two hundred of them is more than a
                    // command line holds.
                    let spec = it.next().unwrap().to_string();
                    let text = match spec.strip_prefix('@') {
                        Some(path) => std::fs::read_to_string(path).unwrap(),
                        None => spec,
                    };
                    let ranges = text
                        .split_whitespace()
                        .map(|r| {
                            let (a, b) = r.split_once('-').unwrap();
                            (parse_hex(a), parse_hex(b))
                        })
                        .collect();
                    (frame, seat, dump, ranges)
                })
                .collect()
        })
        .unwrap_or_default();

    // (start, end, out): per-address change counters over a window.
    let diffs: Vec<(u32, u32, String)> = opt
        .get("diff")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let (range, path) = w.split_once(':').unwrap();
                    let (a, b) = range.split_once('-').unwrap();
                    (a.parse().unwrap(), b.parse().unwrap(), path.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    struct DiffState {
        prev: Vec<u8>,
        count: Vec<u8>,
        first: Vec<u8>,
        frames: HashMap<u32, Vec<u32>>,
    }
    let mut diff_states: Vec<Option<DiffState>> = diffs.iter().map(|_| None).collect();
    let steppers: Vec<(u32, u32, String)> = opt
        .get("steppers")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let (range, path) = w.split_once(':').unwrap();
                    let (a, b) = range.split_once('-').unwrap();
                    (a.parse().unwrap(), b.parse().unwrap(), path.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    let mut stepper_states: Vec<Option<DiffState>> = steppers.iter().map(|_| None).collect();

    let warps: Vec<(u32, u32)> = opt
        .get("warp")
        .map(|v| {
            v.iter()
                .map(|w| {
                    let (a, s) = w.split_once(':').unwrap();
                    (parse_hex(a), s.parse().unwrap())
                })
                .collect()
        })
        .unwrap_or_default();

    let mut watch_prev: Vec<[Vec<u8>; 2]> = watches.iter().map(|&(_, l)| [vec![0; l], vec![0; l]]).collect();
    let mut cur: [HostInput; 2] = [HostInput::default(); 2];
    let mut si = 0usize;

    for f in 0..frames {
        frame_now.store(f, std::sync::atomic::Ordering::Relaxed);
        for seat in 0..2 {
            let active = covers[seat].iter().any(|&(a, b, _)| f >= a && f <= b);
            recordings[seat].store(active, std::sync::atomic::Ordering::Relaxed);
            let active7 = covers7[seat].iter().any(|&(a, b, _)| f >= a && f <= b);
            recordings7[seat].store(active7, std::sync::atomic::Ordering::Relaxed);
        }
        for seat in 0..2 {
            if let Some(input) = walk[seat].get(f as usize) {
                cur[seat] = *input;
            } else if opt.contains_key("walk") {
                cur[seat] = HostInput::default();
            }
        }
        while si < script.len() && script[si].0 <= f {
            let (_, seat, input) = script[si];
            match seat {
                2 => cur = [input; 2],
                s => cur[s] = input,
            }
            si += 1;
        }
        for (pf, seat, addr, bytes) in &pokes {
            if *pf != f {
                continue;
            }
            for s in 0..2 {
                if *seat != 2 && *seat != s {
                    continue;
                }
                let nds = link.console(s);
                for (off, b) in bytes.iter().enumerate() {
                    nds.write8(addr + off as u32, *b);
                }
            }
        }

        for (sf, seat, dump, ranges) in &splices {
            if *sf != f {
                continue;
            }
            for s in 0..2 {
                if *seat != 2 && *seat != s {
                    continue;
                }
                let nds = link.console(s);
                for &(addr, end) in ranges {
                    for a in addr..end {
                        nds.write8(a, dump[(a - 0x0200_0000) as usize]);
                    }
                }
            }
        }

        for &(addr, step) in &warps {
            for s in 0..2 {
                let nds = link.console(s);
                let v = nds.read32(addr).wrapping_add(step);
                nds.write32(addr, v);
            }
        }

        link.tick(cur);

        for seat in 0..2 {
            for &(a, b, ref path) in &covers[seat] {
                if f == b {
                    let mut sorted: Vec<u32> = hits[seat].lock().unwrap().iter().copied().collect();
                    sorted.sort_unstable();
                    let text: String = sorted.iter().map(|a| format!("{a:08x}\n")).collect();
                    std::fs::write(path, text).unwrap();
                    println!("[{f:5}] cover s{seat} {a}-{b}: {} addresses -> {path}", sorted.len());
                    hits[seat].lock().unwrap().clear();
                }
            }
        }

        for (&(addr, len), seats) in watches.iter().zip(watch_prev.iter_mut()) {
            for (seat, prev) in seats.iter_mut().enumerate() {
                let buf = ram_range(&mut link, seat, addr, len);
                if buf != *prev {
                    println!("[{f:5}] s{seat} {addr:08x}: {}", hex(&buf));
                    *prev = buf;
                }
            }
        }
        if (shot_every != 0 && f % shot_every == 0) || shot_at.contains(&f) {
            for seat in 0..2 {
                save_shot(&mut link, seat, &format!("{dump_dir}/f{f:06}-s{seat}.png"));
            }
        }
        if ram_at.contains(&f) {
            for seat in 0..2 {
                let ram = link.console(seat).main_ram().to_vec();
                std::fs::write(format!("{dump_dir}/ram-f{f:06}-s{seat}.bin"), ram).unwrap();
            }
        }
        for &(at, addr, len) in &ram7_at {
            if at != f {
                continue;
            }
            for seat in 0..2 {
                let nds = link.console(seat);
                let buf: Vec<u8> = (0..len as u32).map(|i| nds.arm7_read8(addr + i)).collect();
                std::fs::write(format!("{dump_dir}/arm7-f{f:06}-{addr:08x}-s{seat}.bin"), buf).unwrap();
            }
        }
        for seat in 0..2 {
            for &(a, b, ref path) in &covers7[seat] {
                if f == b {
                    let mut sorted: Vec<u32> = hits7[seat].lock().unwrap().iter().copied().collect();
                    sorted.sort_unstable();
                    let text: String = sorted.iter().map(|a| format!("{a:08x}\n")).collect();
                    std::fs::write(path, text).unwrap();
                    println!("[{f:5}] cover7 s{seat} {a}-{b}: {} addresses -> {path}", sorted.len());
                    hits7[seat].lock().unwrap().clear();
                }
            }
        }
        for (di, &(start, end, ref path)) in diffs.iter().enumerate() {
            if f < start || f > end {
                continue;
            }
            let ram = link.console(0).main_ram().to_vec();
            let st = diff_states[di].get_or_insert_with(|| DiffState {
                prev: ram.clone(),
                count: vec![0; ram.len()],
                first: ram.clone(),
                frames: HashMap::new(),
            });
            for (addr, (&new, &old)) in ram.iter().zip(st.prev.iter()).enumerate() {
                if new != old {
                    st.count[addr] = st.count[addr].saturating_add(1);
                    if st.count[addr] <= 4 {
                        st.frames.entry(addr as u32).or_default().push(f);
                    }
                }
            }
            st.prev = ram;
            if f == end {
                let st = diff_states[di].take().unwrap();
                let mut out = String::new();
                for (addr, &count) in st.count.iter().enumerate() {
                    if !(1..=3).contains(&count) {
                        continue;
                    }
                    use std::fmt::Write;
                    writeln!(
                        &mut out,
                        "0x{:08x} {}x {:02x}->{:02x} @{:?}",
                        0x0200_0000 + addr,
                        count,
                        st.first[addr],
                        st.prev[addr],
                        st.frames.get(&(addr as u32)).unwrap()
                    )
                    .unwrap();
                }
                std::fs::write(path, out).unwrap();
                println!("[{f:5}] diff window {start}-{end} written to {path}");
            }
        }
        for (si_, &(start, end, ref path)) in steppers.iter().enumerate() {
            if f < start || f > end {
                continue;
            }
            let ram = link.console(0).main_ram().to_vec();
            let st = stepper_states[si_].get_or_insert_with(|| DiffState {
                prev: ram.clone(),
                count: vec![0; ram.len()],
                first: ram.clone(),
                frames: HashMap::new(),
            });
            for (addr, (&new, &old)) in ram.iter().zip(st.prev.iter()).enumerate() {
                if new != old {
                    st.count[addr] = st.count[addr].saturating_add(1);
                }
            }
            st.prev = ram;
            if f == end {
                let st = stepper_states[si_].take().unwrap();
                let need = ((end - start) / 3).min(250) as u8;
                let mut out = String::new();
                for (addr, &count) in st.count.iter().enumerate() {
                    if count < need {
                        continue;
                    }
                    use std::fmt::Write;
                    writeln!(
                        &mut out,
                        "0x{:08x} {}x {:02x}->{:02x}",
                        0x0200_0000 + addr,
                        count,
                        st.first[addr],
                        st.prev[addr],
                    )
                    .unwrap();
                }
                std::fs::write(path, out).unwrap();
                println!("[{f:5}] steppers window {start}-{end} written to {path}");
            }
        }
        if status_every != 0 && f % status_every == 0 {
            let pcs = [link.console(0).pc(), link.console(1).pc()];
            println!(
                "[{f:5}] connected={} pc0={:08x} pc1={:08x}",
                link.connected(),
                pcs[0],
                pcs[1]
            );
        }
    }

    for (&(seat, site), count) in fired.lock().unwrap().iter() {
        println!("redirect s{seat} {site:08x} fired {count} times");
    }
    for seat in 0..2 {
        save_shot(&mut link, seat, &format!("{dump_dir}/final-s{seat}.png"));
    }
    save_out(&mut link);
    println!(
        "done at frame {frames}, connected={} pc0={:08x} pc1={:08x}",
        link.connected(),
        link.console(0).pc(),
        link.console(1).pc()
    );
}
