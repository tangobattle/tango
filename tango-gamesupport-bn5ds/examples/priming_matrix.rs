//! Priming matrix: run the priming walk over every (host file, joiner
//! file) combination the given cartridge dumps can produce.
//!
//! This is the harness that cornered the "no battle 2400 frames past
//! the board" stall — long blamed on flash wear layouts, actually the
//! walk's own row-pick gate: the joiner's list word carries a
//! neighboring byte that reads 1 for some host×joiner save pairings,
//! and the old whole-word compare never fired on them (the walk now
//! gates on the count byte alone — see `LIST_HAS_HOST` in the game
//! crate). Which saves meet is the pairing, so the wild "sometimes"
//! flake was deterministic here, as a specific host×joiner cell. Each
//! dump's file-select slots each become an identity exactly as a live
//! session carries one: the dump untouched, plus a `PlayedFile`
//! session payload — so this also regression-tests the
//! payload-steered save select. Every cell must be OK; the walk's own
//! log lines carry the diagnostics when a cell stalls.
//!
//! Usage: priming_matrix <rom.nds> <flash.sav>... [--jp] [--type T]
//!        [--rtc SECS[,SECS...]] [--emit DIR]
//!
//! `--emit DIR` writes each identity's sram to DIR/<label>.sav (for
//! menu_probe) instead of running the matrix. Sessions no longer
//! rewrite the cart, so a two-file dump emits the same bytes under
//! both labels — the identities differ only in the row the matrix
//! steers, which a bare .sav cannot carry.
//!
//! The flags below run one walk instead, with the joiner's flash
//! hand-built from the emitted identities — the geometry surgery the
//! stall was isolated with:
//!   --host FILE --joiner FILE   the two consoles' srams
//!   --graft FILE --range LO:HI      donor's live save image bytes
//!   --graft FILE --flashrange LO:HI donor's raw flash bytes
//!   --copywithin SRC:DST:LEN        move joiner flash bytes (repeatable)
//!   --setgen BLOCK:HEX              restamp a joiner pair's counter
//!   --host-setgen BLOCK:HEX         the same on the host
//!   --erase LO:HI                   fill joiner flash with 0xff

use tango_gamesupport_bn5ds::dataview::save::{
    PlayedFile, SaveSet, BLOCK_SIZE, CHECKSUM_OFFSET, GENERATION_OFFSET, INTERIOR_CHECKSUM_OFFSET, MAGIC,
    MAGIC_OFFSET, SAVE_IMAGE_SIZE, SIZE,
};
use tango_gamesupport_common::dataview::save::Save as _;

/// The game's own checksum (see dataview save.rs).
fn checksum(buf: &[u8]) -> u16 {
    let mut remaining = buf.len() as u16;
    let mut sum = 0u16;
    for pair in buf.chunks_exact(2) {
        sum = sum.wrapping_add(u16::from_le_bytes([pair[0], pair[1]]) ^ remaining);
        remaining = remaining.wrapping_sub(2);
    }
    sum
}

fn rebuild_block(data: &mut [u8], block: usize) {
    let base = block * BLOCK_SIZE;
    // The interior byte-sum first — it lives inside the image cs2
    // covers, and the game rejects a save whose data changed without
    // it (which is what quietly invalidated grafted carts before the
    // interior checksum was known).
    let image = &data[base..][..SAVE_IMAGE_SIZE];
    let interior = image.iter().map(|&v| v as u32).sum::<u32>().wrapping_sub(
        image[INTERIOR_CHECKSUM_OFFSET..][..4]
            .iter()
            .map(|&v| v as u32)
            .sum::<u32>(),
    );
    data[base + INTERIOR_CHECKSUM_OFFSET..][..4].copy_from_slice(&interior.to_le_bytes());
    let cs2 = checksum(&data[base..][..SAVE_IMAGE_SIZE]);
    data[base + CHECKSUM_OFFSET + 4..][..2].copy_from_slice(&cs2.to_le_bytes());
    data[base + CHECKSUM_OFFSET + 6..][..2].copy_from_slice(&0u16.wrapping_sub(cs2).to_le_bytes());
    let cs1 = checksum(&data[base + CHECKSUM_OFFSET + 4..base + GENERATION_OFFSET + 4]);
    data[base + CHECKSUM_OFFSET..][..2].copy_from_slice(&cs1.to_le_bytes());
    data[base + CHECKSUM_OFFSET + 2..][..2].copy_from_slice(&0u16.wrapping_sub(cs1).to_le_bytes());
}

/// The live (highest-generation formatted) block of a stamped session
/// sram — the file the game will mount as current.
fn live_block(data: &[u8]) -> usize {
    (0..SIZE / BLOCK_SIZE)
        .filter(|&b| &data[b * BLOCK_SIZE + MAGIC_OFFSET..][..MAGIC.len()] == MAGIC)
        .max_by_key(|&b| u32::from_le_bytes(data[b * BLOCK_SIZE + GENERATION_OFFSET..][..4].try_into().unwrap()))
        .expect("no formatted block")
}

/// Both screens of one console as a PNG, for eyeballing what the
/// post-walk pair is actually showing.
fn save_shot(link: &mut tango_backend_melonds::Link, seat: usize, path: &str) {
    let Some((top, bottom)) = link.console(seat).framebuffers() else {
        return;
    };
    let mut img = image::RgbImage::new(256, 384);
    for (half, screen) in [top, bottom].into_iter().enumerate() {
        for (i, &pixel) in screen.iter().enumerate() {
            let [b, g, r, _] = pixel.to_le_bytes();
            img.put_pixel((i % 256) as u32, (half * 192 + i / 256) as u32, image::Rgb([r, g, b]));
        }
    }
    img.save(path).unwrap();
}

struct Identity {
    label: String,
    sram: Vec<u8>,
    /// The session payload a live commit of this file would send — the
    /// file-select row the walk steers this console into.
    played: PlayedFile,
}

fn main() {
    env_logger::Builder::from_default_env()
        .filter(Some("tango_gamesupport_bn5ds"), log::LevelFilter::Info)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");

    let mut layout = &tango_gamesupport_bn5ds::pvp::priming::US;
    let mut jp = false;
    let mut match_type = 0u8;
    let mut rtcs: Vec<u64> = vec![1_770_000_000];
    let mut emit: Option<String> = None;
    let mut graft_host: Option<String> = None;
    let mut graft_joiner: Option<String> = None;
    let mut graft_from: Option<String> = None;
    let mut graft_range: Option<(usize, usize)> = None;
    let mut flash_range: Option<(usize, usize)> = None;
    let mut erase_range: Option<(usize, usize)> = None;
    let mut copy_within: Vec<(usize, usize, usize)> = Vec::new();
    let mut set_gen: Vec<(usize, u32)> = Vec::new();
    let mut host_set_gen: Vec<(usize, u32)> = Vec::new();
    let mut dumps: Vec<&str> = Vec::new();
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        if a == "--jp" {
            layout = &tango_gamesupport_bn5ds::pvp::priming::JP;
            jp = true;
        } else if a == "--type" {
            match_type = it.next().expect("--type needs a value").parse().expect("type");
        } else if a == "--emit" {
            emit = Some(it.next().expect("--emit needs a directory").clone());
        } else if a == "--host" {
            graft_host = Some(it.next().expect("--host needs a file").clone());
        } else if a == "--joiner" {
            graft_joiner = Some(it.next().expect("--joiner needs a file").clone());
        } else if a == "--graft" {
            graft_from = Some(it.next().expect("--graft needs a file").clone());
        } else if a == "--copywithin" {
            let r = it.next().expect("--copywithin needs SRC:DST:LEN");
            let mut parts = r.split(':').map(|s| usize::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex"));
            copy_within.push((parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap()));
        } else if a == "--setgen" {
            let r = it.next().expect("--setgen needs BLOCK:HEX");
            let (b, g) = r.split_once(':').expect("BLOCK:HEX");
            set_gen.push((b.parse().expect("block"), u32::from_str_radix(g.trim_start_matches("0x"), 16).expect("hex")));
        } else if a == "--host-setgen" {
            let r = it.next().expect("--host-setgen needs BLOCK:HEX");
            let (b, g) = r.split_once(':').expect("BLOCK:HEX");
            host_set_gen.push((b.parse().expect("block"), u32::from_str_radix(g.trim_start_matches("0x"), 16).expect("hex")));
        } else if a == "--erase" {
            let r = it.next().expect("--erase needs LO:HI");
            let (lo, hi) = r.split_once(':').expect("LO:HI");
            erase_range = Some((
                usize::from_str_radix(lo.trim_start_matches("0x"), 16).expect("hex"),
                usize::from_str_radix(hi.trim_start_matches("0x"), 16).expect("hex"),
            ));
        } else if a == "--flashrange" {
            let r = it.next().expect("--flashrange needs LO:HI");
            let (lo, hi) = r.split_once(':').expect("LO:HI");
            flash_range = Some((
                usize::from_str_radix(lo.trim_start_matches("0x"), 16).expect("hex"),
                usize::from_str_radix(hi.trim_start_matches("0x"), 16).expect("hex"),
            ));
        } else if a == "--range" {
            let r = it.next().expect("--range needs LO:HI");
            let (lo, hi) = r.split_once(':').expect("LO:HI");
            graft_range = Some((
                usize::from_str_radix(lo.trim_start_matches("0x"), 16).expect("hex"),
                usize::from_str_radix(hi.trim_start_matches("0x"), 16).expect("hex"),
            ));
        } else if a == "--rtc" {
            rtcs = it
                .next()
                .expect("--rtc needs seconds")
                .split(',')
                .map(|s| s.parse().expect("seconds"))
                .collect();
        } else {
            dumps.push(a);
        }
    }

    // Graft mode: one walk, with a byte range of the joiner's live save
    // image replaced by the graft donor's — both srams pre-stamped
    // identity images (from --emit).
    if let (Some(host), Some(joiner)) = (&graft_host, &graft_joiner) {
        let mut host_sram = std::fs::read(host).expect("host sram");
        let mut joiner_sram = std::fs::read(joiner).expect("joiner sram");
        for (block, gen) in host_set_gen {
            for b in [block, block ^ 1] {
                host_sram[b * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&gen.to_le_bytes());
                rebuild_block(&mut host_sram, b);
            }
            println!("host blocks {block}/{} gen set to {gen:#x}", block ^ 1);
        }
        if let (Some(from), Some((lo, hi))) = (&graft_from, flash_range) {
            let donor = std::fs::read(from).expect("graft sram");
            joiner_sram[lo..hi].copy_from_slice(&donor[lo..hi]);
            println!("flash graft [{lo:#x}..{hi:#x}) into joiner");
        } else if let (Some(from), Some((lo, hi))) = (&graft_from, graft_range) {
            let donor = std::fs::read(from).expect("graft sram");
            let src = live_block(&donor) * BLOCK_SIZE;
            let dst = live_block(&joiner_sram);
            let patch = donor[src + lo..src + hi].to_vec();
            for block in [dst, dst ^ 1] {
                joiner_sram[block * BLOCK_SIZE + lo..block * BLOCK_SIZE + hi].copy_from_slice(&patch);
                rebuild_block(&mut joiner_sram, block);
            }
            println!("graft [{lo:#x}..{hi:#x}) into joiner block {dst}");
        }
        for (src, dst, len) in copy_within {
            joiner_sram.copy_within(src..src + len, dst);
            println!("copied joiner flash [{src:#x}..{:#x}) to {dst:#x}", src + len);
        }
        for (block, gen) in set_gen {
            for b in [block, block ^ 1] {
                joiner_sram[b * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&gen.to_le_bytes());
                rebuild_block(&mut joiner_sram, b);
            }
            println!("blocks {block}/{} gen set to {gen:#x}", block ^ 1);
        }
        if let Some((lo, hi)) = erase_range {
            joiner_sram[lo..hi].fill(0xff);
            println!("erased joiner flash [{lo:#x}..{hi:#x})");
        }
        let rtc = std::time::UNIX_EPOCH + std::time::Duration::from_secs(rtcs[0]);
        let mut link =
            tango_backend_melonds::Link::new(&rom, [Some(host_sram.as_slice()), Some(joiner_sram.as_slice())], rtc)
                .expect("link boot");
        // No payloads: the emitted identity images and the geometry
        // surgery are hand-built carts whose current file is the one
        // under test.
        match layout.walk(&mut link, (match_type, 0), [None, None], [0; 16], None) {
            Ok(()) => println!("RESULT: OK"),
            Err(e) => println!("RESULT: FAILED {e:?}"),
        }
        return;
    }

    let mut identities: Vec<Identity> = Vec::new();
    for (d, path) in dumps.iter().enumerate() {
        let data = std::fs::read(path).expect("flash dump unreadable");
        let set = SaveSet::parse(&data).expect("not this game's flash");
        let current = set.current().slot();
        println!(
            "dump {}: {path} (slots {:?}, current file {})",
            (b'A' + d as u8) as char,
            set.slots(),
            current + 1,
        );
        for slot in set.slots() {
            identities.push(Identity {
                label: format!("{}/file{}", (b'A' + d as u8) as char, slot + 1),
                sram: set.save(slot).unwrap().to_sram_dump(),
                played: PlayedFile(slot),
            });
        }
    }

    if let Some(dir) = emit {
        for id in &identities {
            let path = format!("{dir}/{}.sav", id.label.replace('/', "-"));
            std::fs::write(&path, &id.sram).expect("emit");
            println!("wrote {path}");
        }
        return;
    }

    let mut failures = Vec::new();
    for rtc_secs in &rtcs {
        let rtc = std::time::UNIX_EPOCH + std::time::Duration::from_secs(*rtc_secs);
        for host in &identities {
            for joiner in &identities {
                println!("--- host {} vs joiner {} (rtc {rtc_secs}, type {match_type})", host.label, joiner.label);
                let mut link = tango_backend_melonds::Link::new(
                    &rom,
                    [Some(host.sram.as_slice()), Some(joiner.sram.as_slice())],
                    rtc,
                )
                .expect("link boot");
                match layout.walk(
                    &mut link,
                    (match_type, 0),
                    [Some(&host.played), Some(&joiner.played)],
                    [0; 16],
                    None,
                ) {
                    Ok(()) => {
                        // The walk's finish line is leaving the board
                        // with the link up — run on and report where the
                        // pair actually is, so a flow whose tail is
                        // still negotiating past that line (and would
                        // crawl under the session's rollback) shows up
                        // here instead of in a live match.
                        use tango_match::Link as _;
                        // The builds' comm substate words
                        // (`RAMOffsets::substate` in the game crate,
                        // which examples can't see). NOT the scene word:
                        // that one reads the overworld area the save is
                        // standing in, so it is a property of the
                        // cartridge rather than of the session, and two
                        // saves park under different values at the
                        // identical moment. The substate is one value
                        // for every save — `0x0003_0102` is the link
                        // battle, and anything else here is a pair that
                        // left it.
                        let substate_word = if jp { 0x021e_f06c } else { 0x021f_66ec };
                        let substate = |link: &mut tango_backend_melonds::Link, seat: usize| {
                            link.console(seat).read32(substate_word)
                        };
                        let mut trace = String::new();
                        for step in 0..=30 {
                            if step > 0 {
                                for _ in 0..30 {
                                    link.tick([tango_match::HostInput::default(); 2]);
                                }
                            }
                            if step % 5 == 0 || step == 1 {
                                let m = [substate(&mut link, 0), substate(&mut link, 1)];
                                trace += &format!(" +{}:{:#010x}/{:#010x}", step * 30, m[0], m[1]);
                            }
                        }
                        println!("    OK (substates{trace}, connected={})", link.connected());
                    }
                    Err(e) => {
                        println!("    FAILED: {e:?}");
                        // Leave the stuck pair's screens on disk — a
                        // parked screen names the stall faster than any
                        // amount of substate reading.
                        for seat in 0..2 {
                            let path = format!(
                                "wedge-{}-vs-{}-s{seat}.png",
                                host.label.replace('/', "-"),
                                joiner.label.replace('/', "-")
                            );
                            save_shot(&mut link, seat, &path);
                            println!("    wrote {path}");
                        }
                        failures.push(format!("host {} vs joiner {} rtc {rtc_secs}", host.label, joiner.label));
                    }
                }
            }
        }
    }
    println!("=== {} failures", failures.len());
    for f in &failures {
        println!("    {f}");
    }
}
