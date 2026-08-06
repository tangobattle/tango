//! The mgba half of the rollback-fidelity drill — bn5ds's
//! `rollback_probe` asked of a GBA family.
//!
//! A pair that speculated and rolled back must be the pair that never
//! speculated at all. `gba_landing_probe` restores exactly once — a
//! landing — and `pvp_probe` never restores; neither models what PvP
//! actually does, which is snapshot, run ahead on a guess, and restore,
//! hundreds of times a minute. And it is the only stage that differs
//! *between the two peers*: both simulate the same pair from the same
//! confirmed inputs, but each mispredicts on its own network timing, so
//! their restore histories never match. Anything a restore fails to
//! carry is therefore not a local glitch — it is a desync, and an
//! intermittent one, because which ticks get rolled back is a property
//! of the wire.
//!
//! Pair A is the reference: it consumes the recording's rows straight
//! through and never snapshots. Pair B consumes the identical rows, but
//! every `--every` ticks it snapshots, runs `--depth` ticks ahead with
//! the remote seat's input *predicted* (repeat-last, the engine's own
//! predictor), then restores and re-simulates those ticks for real. The
//! two are compared every tick. B is what a peer looks like; A is what
//! that peer is supposed to be indistinguishable from.
//!
//! Comparing reference-against-churn is stronger than comparing two
//! churn schedules against each other: if B matches A under any
//! schedule, then any two peers match too, whatever they each rolled
//! back.
//!
//! **Nothing is masked, and that is deliberate.** mgba writes a
//! savestate into an *uninitialized* 0x61000-byte buffer and leaves its
//! reserved fields (and the special-cartridge union a normal cart never
//! uses) untouched, so a raw byte compare of two states would trip over
//! whatever heap garbage each buffer happened to inherit. This probe
//! installs a global allocator that hands back zeroed memory for every
//! allocation, which makes those holes read as zero in both pairs by
//! construction — untouched bytes are equal instead of merely
//! ignorable. That is why this probe can fail on the *first* differing
//! byte, and why it needs no equivalent of the DS drill's instance
//! cookie mask. (A byte mgba does not write is also a byte mgba does
//! not read on load, so it cannot carry simulation state either way.)
//!
//! The comparison runs at the `mgba_rollback::Link` level — the pair,
//! its lockstep driver blobs, and the digest covering the audio-FIFO
//! and DMA lanes `Snapshot` captures outside the core states. The
//! wrapper above it (`tango_backend_mgba::Link`) adds audio revocation
//! and telemetry, both host-side playback/reporting state that a
//! savestate deliberately does not carry, and its snapshot type is
//! private — so the rollback unit itself is the right altitude.
//!
//! Usage: gba_rollback_probe <rom.gba> <replay.tangoreplay> [ticks]
//!            [--every N] [--depth K] [--peer 0|1] [--first N]
//!            [--random SEED]
//!
//! `--peer` is which seat is the local player, whose input is known
//! rather than guessed. `--every 0` never speculates: the harness's own
//! baseline. `--depth 0` snapshots and restores without running ahead —
//! the sharpest question there is, since it asks what a restore alone
//! fails to carry. `--first N` pins the tick of the first speculation,
//! so a single rollback can be isolated and bisected. `--random SEED`
//! replaces the recording's rows with a seeded pseudo-random stream
//! (identical for both pairs), which keeps both games under constant
//! input churn and can run past the recording's length.
//!
//! Set `FRAME_DUMP_DIR` to have a frame divergence leave both sides'
//! screens on disk.
//!
//! Three verdicts come out, deliberately kept apart:
//!
//! * `STATE` — the machine. Any difference here is a desync and the
//!   probe exits 1 on the tick it appears.
//! * `SAVE` — how often the reference pair wrote a cartridge save. A
//!   GBA savestate does not carry the save image, so a write on a
//!   speculative tick is one no rollback can take back; the compare
//!   covers the saves, and this counter says whether the game goes
//!   anywhere near that hole in the first place.
//! * `FRAMES` — presentation. A tick ends on a *reference core* frame
//!   boundary, so console 1 is generally parked mid-frame and its
//!   framebuffer is a torn composite by construction; a run-ahead draws
//!   past the seam and the restore cannot un-draw it. Expect at most one
//!   stale scanline on console 1, on the tick after a rollback only.

use std::alloc::{GlobalAlloc, Layout, System};

use tango_backend_mgba::GameSupport as _;
use tango_gamesupport_bn6::pvp;

/// Every allocation comes back zeroed, so mgba's untouched savestate
/// holes read the same in both pairs (see the module docs). Costs one
/// memset per allocation, which against a GBA frame is nothing.
struct ZeroFill;

unsafe impl GlobalAlloc for ZeroFill {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        System.alloc_zeroed(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        System.alloc_zeroed(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    // `realloc` is left to the trait default, which routes through
    // `alloc` above — so a grown buffer's tail is zeroed too.
}

#[global_allocator]
static ALLOC: ZeroFill = ZeroFill;

/// One GBA savestate, `sizeof(struct GBASerializedState)`.
const STATE_LEN: usize = 0x6_1000;

/// The registration's engine support for the cart this ROM image is,
/// off its own header code — the family ships four (Gregar/Falzar ×
/// JP/US) and priming is per-cartridge.
fn pvp_of(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"BR5J" => &pvp::PVP_BR5J_00,
        b"BR6J" => &pvp::PVP_BR6J_00,
        b"BR5E" => &pvp::PVP_BR5E_00,
        b"BR6E" => &pvp::PVP_BR6E_00,
        code => panic!("not a bn6 rom (code {:02x?})", code),
    }
}

/// Boot a pair on this ROM/save/rtc and prime both games into their
/// link battle — `tango_backend_mgba::backend::boot_pair`, which is
/// crate-private, open-coded at the rollback unit's own level.
fn boot_primed(
    rom: &[u8],
    saves: &[Vec<u8>; 2],
    rtc: std::time::SystemTime,
    config: &tango_backend_mgba::PrimeConfig,
) -> mgba_rollback::Link {
    let support = pvp_of(rom);
    let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
        sides: vec![
            mgba_rollback::SideOptions {
                rom: rom.to_vec(),
                save: Some(saves[0].clone()),
            },
            mgba_rollback::SideOptions {
                rom: rom.to_vec(),
                save: Some(saves[1].clone()),
            },
        ],
        rtc: Some(rtc),
        peripheral: mgba_rollback::Peripheral::Cable,
    })
    .expect("pair boot");

    let events = tango_match::telemetry::EventSink::new();
    let primed = [
        tango_backend_mgba::PrimedLatch::new(),
        tango_backend_mgba::PrimedLatch::new(),
    ];
    pair.set_traps(0, support.primer_traps(config, 0, &events, &primed[0]));
    pair.set_traps(1, support.primer_traps(config, 1, &events, &primed[1]));

    let mut ticks = 0u32;
    while !(primed[0].is_set() && primed[1].is_set()) {
        assert!(ticks < 3600, "priming did not reach a link battle");
        pair.tick(&[0, 0]);
        ticks += 1;
    }
    pair
}

/// The whole-link bytes: both cores' savestates end to end, then the
/// lockstep driver blobs and the cartridge saves, length-prefixed.
/// Offsets into this blob are fixed (a GBA state is a fixed-size
/// struct), which is what lets [`locate`] attribute a byte without
/// re-walking anything.
///
/// The cartridge saves are in here because a GBA savestate does *not*
/// carry them — mgba keeps the SRAM/flash image beside the state, and
/// `Snapshot` captures neither. A speculative tick that writes the save
/// therefore survives its own rollback, so the compare has to reach the
/// save or it cannot see that happen.
fn save_link(pair: &mut mgba_rollback::Link) -> (Vec<u8>, u32) {
    let snap = pair.save().expect("link capture");
    let mut out = Vec::with_capacity(2 * STATE_LEN + 0x2_0000);
    for i in 0..2 {
        out.extend_from_slice(snap.core_state(i).as_slice());
    }
    for i in 0..2 {
        let blob = snap.driver_blob(i);
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(blob);
    }
    for i in 0..2 {
        let save = pair.export_save(i).unwrap_or_default();
        out.extend_from_slice(&(save.len() as u32).to_le_bytes());
        out.extend_from_slice(&save);
    }
    // The digest also covers the audio-FIFO and DMA lanes `Snapshot`
    // captures verbatim beside the core states (the savestate's own
    // encoding of them is lossy), which have no public accessor to
    // byte-compare.
    (out, snap.digest())
}

/// Where a byte of one core's savestate lives, per mgba's documented
/// `GBASerializedState` layout (gba/serialize.h). Guest memory is
/// named by the address the game itself uses, which is what a
/// divergence in there has to be chased with.
fn locate_state(off: usize) -> String {
    let mem = |name: &str, base: u32, start: usize| format!("{name}@{:#010x}", base + (off - start) as u32);
    if (0x0_0400..0x0_0800).contains(&off) {
        return mem("IO", 0x0400_0000, 0x0_0400);
    }
    if (0x0_0800..0x0_0c00).contains(&off) {
        return mem("PRAM", 0x0500_0000, 0x0_0800);
    }
    if (0x0_0c00..0x0_1000).contains(&off) {
        return mem("OAM", 0x0700_0000, 0x0_0c00);
    }
    if (0x0_1000..0x1_9000).contains(&off) {
        return mem("VRAM", 0x0600_0000, 0x0_1000);
    }
    if (0x1_9000..0x2_1000).contains(&off) {
        return mem("IWRAM", 0x0300_0000, 0x1_9000);
    }
    if (0x2_1000..STATE_LEN).contains(&off) {
        return mem("EWRAM", 0x0200_0000, 0x2_1000);
    }
    const FIELDS: &[(usize, &str)] = &[
        (0x000, "versionMagic"),
        (0x004, "biosChecksum"),
        (0x008, "romCrc32"),
        (0x00c, "masterCycles"),
        (0x010, "title"),
        (0x01c, "id"),
        (0x020, "cpu.gprs"),
        (0x060, "cpu.cpsr"),
        (0x064, "cpu.spsr"),
        (0x068, "cpu.cycles"),
        (0x06c, "cpu.nextEvent"),
        (0x070, "cpu.bankedRegisters"),
        (0x118, "cpu.bankedSPSRs"),
        (0x130, "audio.psg"),
        (0x18c, "audio.fifoA"),
        (0x1ac, "audio.fifoB"),
        (0x1cc, "audio.misc"),
        (0x1f0, "video"),
        (0x200, "timers"),
        (0x250, "dma"),
        (0x290, "hw.gpio"),
        (0x2c4, "hw.sioNextEvent"),
        (0x2c8, "dmaTransferRegister"),
        (0x2cc, "dmaBlockPC"),
        (0x2d0, "matrix"),
        (0x2e0, "savedata"),
        (0x2f4, "biosPrefetch"),
        (0x2f8, "cpuPrefetch"),
        (0x300, "reservedCpu"),
        (0x310, "globalCycles"),
        (0x318, "lastPrefetchedPc"),
        (0x31c, "miscFlags"),
        (0x320, "nextIrq"),
        (0x324, "biosStall"),
        (0x328, "cart.union"),
        (0x370, "samples.chA"),
        (0x380, "samples.chB"),
        (0x390, "currentSamples"),
        (0x3d0, "bus"),
        (0x3d4, "dmaLatch"),
        (0x3e0, "dmaCountLatch"),
        (0x3e8, "reserved"),
    ];
    let (at, name) = FIELDS
        .iter()
        .rev()
        .find(|(at, _)| *at <= off)
        .copied()
        .unwrap_or((0, "?"));
    format!("{name}+{:#x}", off - at)
}

/// Which part of a whole-link blob ([`save_link`]) a byte offset falls
/// in: one of the two core savestates, or one of the lockstep driver
/// blobs.
fn locate(bytes: &[u8], off: usize) -> String {
    if off < 2 * STATE_LEN {
        return format!("core{} {}", off / STATE_LEN, locate_state(off % STATE_LEN));
    }
    let mut at = 2 * STATE_LEN;
    for (tag, seats) in [("driver", 0..2), ("save", 0..2)] {
        for i in seats {
            let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            at += 4;
            if off < at + len {
                return format!("{tag}{i}+{:#x}", off - at);
            }
            at += len;
        }
    }
    format!("?+{off:#x}")
}

/// Differing byte runs, attributed through [`locate`], with both sides'
/// values.
fn diff_runs(a: &[u8], b: &[u8], max: usize) -> (usize, Vec<String>) {
    let n = a.len().min(b.len());
    let total = (0..n).filter(|&i| a[i] != b[i]).count();
    let mut runs = Vec::new();
    let mut i = 0usize;
    while i < n && runs.len() < max {
        if a[i] == b[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && a[i] != b[i] && i - start < 16 {
            i += 1;
        }
        let hex = |s: &[u8]| s.iter().map(|v| format!("{v:02x}")).collect::<String>();
        runs.push(format!(
            "{}({}B) a={} b={}",
            locate(a, start),
            i - start,
            hex(&a[start..i]),
            hex(&b[start..i])
        ));
    }
    (total, runs)
}

fn flag<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|v| v.parse().unwrap_or_else(|_| panic!("bad value for {name}")))
        .unwrap_or(default)
}

/// A seeded pseudo-random input stream for `--random`: splitmix64 over
/// (seed, tick, seat), masked to the GBA's ten keypad bits. Both pairs
/// consume the identical rows, which is all the drill needs of an input
/// source — the recording is one, this is another that never idles.
fn random_row(seed: u64, tick: u32) -> [u32; 2] {
    let mix = |lane: u64| {
        let mut z = seed
            .wrapping_add(u64::from(tick).wrapping_mul(0x9e37_79b9_7f4a_7c15))
            .wrapping_add(lane.wrapping_mul(0xbf58_476d_1ce4_e5b9));
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    // Hold each key for a handful of ticks rather than re-rolling every
    // frame: a fresh 10-bit roll per tick averages to "everything half
    // pressed", which the game reads as noise. Rolling per 4-tick block
    // gives inputs a menu can actually act on.
    [0u64, 1].map(|lane| (mix(lane.wrapping_mul(0x1_0000) + u64::from(tick / 4)) & 0x3ff) as u32)
}

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let f = std::fs::File::open(&args[1]).expect("replay unreadable");
    let replay = tango_replay::Replay::decode(std::io::BufReader::new(f)).expect("replay undecodable");
    let limit: u32 = args
        .get(2)
        .filter(|a| !a.starts_with("--"))
        .map(|s| s.parse().unwrap())
        .unwrap_or(u32::MAX);
    let every: u32 = flag(&args, "--every", 7);
    let depth: u32 = flag(&args, "--depth", 4);
    let peer: usize = flag(&args, "--peer", 0);
    let first: u32 = flag(&args, "--first", 0);
    let random: u64 = flag(&args, "--random", 0);
    assert!(peer < 2, "--peer is a seat: 0 or 1");

    let recorded: Vec<[u32; 2]> = replay
        .inputs
        .iter()
        .map(|row| row.map(|input| u32::from(input.keys) & tango_backend_mgba::JOYFLAGS_MASK))
        .collect();
    let total = if random != 0 {
        limit.min(u32::MAX / 2)
    } else {
        (recorded.len() as u32).min(limit)
    };
    let inputs: Vec<[u32; 2]> = if random != 0 {
        (0..total + depth + 1).map(|t| random_row(random, t)).collect()
    } else {
        recorded
    };

    let gi = replay
        .metadata
        .side(replay.local_player_index)
        .and_then(|s| s.game_info.as_ref());
    let config = tango_backend_mgba::PrimeConfig {
        match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
        rng_seed: replay.rng_seed,
        disable_bgm: false,
    };
    let saves = [replay.srams[0].clone(), replay.srams[1].clone()];
    println!(
        "replay: family={:?} variant={:?} match_type={:?} {} ticks; rom={} — comparing {total} \
         (mispredict every {every} ticks from {first}, {depth} deep, local seat {peer}{})",
        gi.map(|g| g.rom_family.as_str()),
        gi.map(|g| g.rom_variant),
        config.match_type,
        inputs.len(),
        String::from_utf8_lossy(&rom[0xac..0xb0]),
        if random != 0 {
            format!(", random inputs seed {random}")
        } else {
            String::new()
        },
    );

    // Both pairs walk the real prime. Neither lands on the other: this
    // probe is about what a *restore* fails to carry, so the two must
    // start from states they each reached themselves — which the walk's
    // own determinism guarantees (`gba_landing_probe`).
    let started = std::time::Instant::now();
    let mut a = boot_primed(&rom, &saves, replay.rtc_time(), &config);
    let mut b = boot_primed(&rom, &saves, replay.rtc_time(), &config);
    println!("both pairs primed in {:.1?}", started.elapsed());

    // The baseline. Anything differing before a single rollback has
    // happened is the walk, not the churn, and this probe has nothing
    // to say about it — bail rather than report someone else's bug as
    // ours.
    let ((state_a, digest_a), (state_b, digest_b)) = (save_link(&mut a), save_link(&mut b));
    let (base, runs) = diff_runs(&state_a, &state_b, 8);
    if base != 0 || digest_a != digest_b {
        println!("BASELINE DIFFERS in {base} bytes before any rollback: {runs:?}");
        println!("(digests {digest_a:08x} / {digest_b:08x})");
        println!("(that is a walk-determinism failure, not a rollback one — run gba_landing_probe)");
        std::process::exit(2);
    }
    println!("baseline: byte-identical after priming (digest {digest_a:08x})");

    let mut rollbacks = 0u32;
    let mut speculated = 0u64;
    // Frames are presentation, not machine state — a core's framebuffer
    // is not in its savestate — so a frame divergence is reported with
    // its whole history rather than latched at the first one: how many
    // ticks it covered, and whether it healed. A tear that heals is a
    // redraw artifact; one that never does is a divergence the state
    // compare somehow missed.
    let mut frame_diffs = [0u32; 2];
    let mut frame_span = [(0u32, 0u32); 2];
    // Every tick at which the reference pair touches a cartridge save.
    // A savestate does not carry the save, so a write on a speculative
    // tick is one the rollback cannot take back — which makes "does
    // this game write its save while a match is running" a fact this
    // probe owes its reader whether or not anything diverged.
    let mut written = [
        a.export_save(0).unwrap_or_default(),
        a.export_save(1).unwrap_or_default(),
    ];
    let mut save_writes = [0u32; 2];
    for tick in 0..total {
        let row = inputs[tick as usize];

        // B speculates: it does not know the remote seat's input yet,
        // so it repeats the last one it confirmed — the engine's own
        // predictor — runs ahead, and takes it back. A never does this.
        // Everything B commits below is the confirmed row, exactly what
        // A commits, so the two are only ever separated by the fact
        // that B's cores have executed speculative ticks and A's have
        // not.
        if every > 0 && tick >= first && (tick - first) % every == 0 && (tick + depth) as usize <= inputs.len() {
            let snap = b.save().expect("speculation capture");
            let mut guess = if tick == 0 { [0, 0] } else { inputs[tick as usize - 1] };
            for ahead in 0..depth {
                guess[peer] = inputs[(tick + ahead) as usize][peer];
                b.tick(&guess);
                speculated += 1;
            }
            b.load(&snap).expect("rollback");
            rollbacks += 1;
        }

        a.tick(&row);
        b.tick(&row);

        let ((state_a, digest_a), (state_b, digest_b)) = (save_link(&mut a), save_link(&mut b));
        let (count, runs) = diff_runs(&state_a, &state_b, 12);
        if count != 0 || digest_a != digest_b {
            println!(
                "DIVERGED at tick {} after {rollbacks} rollbacks: {count} bytes (digests \
                 {digest_a:08x} / {digest_b:08x}), first runs {runs:?}",
                tick + 1
            );
            println!("({:.1?})", started.elapsed());
            if let Some(at) = (0..state_a.len()).find(|&i| state_a[i] != state_b[i]) {
                let lo = at.saturating_sub(0x40) & !0xf;
                for (tag, s) in [("a", &state_a), ("b", &state_b)] {
                    for row in 0..12 {
                        let at = lo + row * 16;
                        if at + 16 > s.len() {
                            break;
                        }
                        println!("  {tag} {} {:02x?}", locate(&state_a, at), &s[at..at + 16]);
                    }
                }
            }
            std::process::exit(1);
        }

        for seat in 0..2 {
            let now = a.export_save(seat).unwrap_or_default();
            if now != written[seat] {
                let bytes = now.iter().zip(&written[seat]).filter(|(x, y)| x != y).count();
                if save_writes[seat] < 8 {
                    println!("SAVE: pair A wrote console {seat}'s cartridge save at tick {} ({bytes} bytes)", tick + 1);
                }
                save_writes[seat] += 1;
                written[seat] = now;
            }
        }

        for seat in 0..2 {
            let (fa, fb) = (a.video_buffer(seat), b.video_buffer(seat));
            if fa == fb {
                continue;
            }
            let pixels = fa
                .zip(fb)
                .map(|(x, y)| x.chunks(2).zip(y.chunks(2)).filter(|(p, q)| p != q).count())
                .unwrap_or(0);
            if frame_diffs[seat] < 12 {
                println!("FRAMES diverged at tick {}, console {seat}: {pixels} pixels", tick + 1);
            }
            if frame_diffs[seat] == 0 {
                frame_span[seat].0 = tick + 1;
                if let Ok(dir) = std::env::var("FRAME_DUMP_DIR") {
                    for (tag, buf) in [("a", fa), ("b", fb)] {
                        if let Some(buf) = buf {
                            let mut rgba = vec![0u8; buf.len() * 2];
                            mgba::gba::bgr555_to_rgba8(buf, &mut rgba);
                            image::RgbaImage::from_raw(240, 160, rgba)
                                .expect("framebuffer shape")
                                .save(format!("{dir}/gba-rollback-t{}-s{seat}-{tag}.png", tick + 1))
                                .expect("frame dump");
                        }
                    }
                }
            }
            frame_diffs[seat] += 1;
            frame_span[seat].1 = tick + 1;
        }

        if (tick + 1) % 600 == 0 {
            println!(
                "  tick {}: identical, {rollbacks} rollbacks / {speculated} speculative ticks ({:.1?})",
                tick + 1,
                started.elapsed()
            );
        }
    }

    println!(
        "STATE: byte-identical through {total} ticks across {rollbacks} rollbacks \
         ({speculated} speculative ticks, {:.1?})",
        started.elapsed()
    );
    println!("SAVE: reference pair wrote its cartridge saves on {save_writes:?} ticks");
    if frame_diffs.iter().any(|&n| n != 0) {
        for seat in 0..2 {
            if frame_diffs[seat] != 0 {
                println!(
                    "FRAMES: console {seat} differed on {} of {total} ticks, ticks {}..={}",
                    frame_diffs[seat], frame_span[seat].0, frame_span[seat].1
                );
            }
        }
        std::process::exit(1);
    }
    println!("SUCCESS: state and frames both identical");
}
