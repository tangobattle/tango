//! The mgba half of the cross-pair landing drill — bn5ds's
//! `landing_probe` asked of a GBA family.
//!
//! [`ReplaySet::stats_reusing_playback`] hands the statistics pair a
//! bare, unwalked pair and lands it on the display pair's primed
//! capture, skipping seconds of priming. That is only sound if a pair
//! landed on a capture plays the recording exactly as the pair that
//! took the capture did — a claim about everything a restore does
//! *not* carry. On melonDS it did not hold (see the bn5ds drill and
//! `ReplayBoot::boot_unprimed`), so this asks the same question of
//! mgba rather than assuming the answer.
//!
//! Both pairs come up through the production entry points: the walked
//! one from `ReplaySet::playback`, the landed one from
//! `playback_landed` (the same bare boot the statistics pass uses,
//! handed back with frames readable). They then consume the same
//! recorded rows in lockstep, comparing rendered frames every tick.
//!
//! Usage: landing_probe <rom.gba> <replay.tangoreplay> [ticks]

use std::sync::Arc;

/// The registration for the cart this ROM image is, off its own header
/// code — the family ships four (Gregar/Falzar × JP/US) and priming is
/// per-cartridge, so the recording's family alone doesn't pick one.
fn game_of(rom: &[u8]) -> &'static tango_gamesupport::Game {
    let code: [u8; 4] = rom[0xac..0xb0].try_into().expect("rom too short for a header");
    [
        &tango_gamesupport_bn6::BN6G,
        &tango_gamesupport_bn6::BN6F,
        &tango_gamesupport_bn6::EXE6G,
        &tango_gamesupport_bn6::EXE6F,
    ]
    .into_iter()
    .find(|game| *game.rom_code == code)
    .unwrap_or_else(|| panic!("not this crate's cart: {}", String::from_utf8_lossy(&code)))
}

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let f = std::fs::File::open(&args[1]).expect("replay unreadable");
    let replay = tango_replay::Replay::decode(std::io::BufReader::new(f)).expect("replay undecodable");
    let limit: u32 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(u32::MAX);

    let inputs: Arc<Vec<[tango_match::HostInput; 2]>> = Arc::new(
        replay
            .inputs
            .iter()
            .map(|&row| {
                row.map(|input| tango_match::HostInput {
                    keys: input.keys as u32,
                    touch: input.touch.map(|(x, y)| (x as u16, y as u16)),
                })
            })
            .collect(),
    );
    let total = (inputs.len() as u32).min(limit);
    let game = game_of(&rom);
    let set = game
        .pvp
        .open_replay(tango_match::ReplayConfig {
            roms: [rom.clone(), rom.clone()],
            saves: replay.srams.clone(),
            inputs: inputs.clone(),
            rng_seed: replay.rng_seed,
            rtc: replay.rtc_time(),
            match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            local_player: replay.local_player_index as usize,
            peer_rom: tango_match::PeerRom {
                code: *game.rom_code,
                revision: game.revision,
            },
            want_stats: false,
            disable_bgm: false,
        })
        .expect("open_replay");
    println!("replay: {} ticks, comparing {total}", inputs.len());

    let started = std::time::Instant::now();
    // The walked pair, and its primed tick-0 capture (published by this
    // call, which is what the landing below waits on).
    let mut walked = set.playback().expect("walked boot");
    let mut landed = set.playback_landed().expect("landed boot");
    println!("both pairs up in {:.1?}", started.elapsed());

    let mut diverged = false;
    for tick in 0..total {
        if !walked.step() || !landed.step() {
            break;
        }
        let (fw, fl) = (walked.frames(), landed.frames());
        if fw.frames != fl.frames {
            let seat = (0..2).find(|&s| fw.frames[s] != fl.frames[s]).unwrap_or(0);
            println!("FRAMES diverged at tick {}, console {seat}", tick + 1);
            if let Ok(dir) = std::env::var("FRAME_DUMP_DIR") {
                for (tag, frames) in [("walked", &fw), ("landed", &fl)] {
                    let fb = &frames.frames[seat];
                    if !fb.is_empty() {
                        image::RgbaImage::from_raw(240, 160, fb.clone())
                            .expect("framebuffer shape")
                            .save(format!("{dir}/gba-diverge-t{}-s{seat}-{tag}.png", tick + 1))
                            .expect("frame dump");
                    }
                }
            }
            diverged = true;
            break;
        }
        if (tick + 1) % 1200 == 0 {
            println!("  identical through tick {} ({:.1?})", tick + 1, started.elapsed());
        }
    }

    if diverged {
        std::process::exit(1);
    }
    println!(
        "SUCCESS: a landed pair matched the walked pair for {total} ticks ({:.1?})",
        started.elapsed()
    );
}
