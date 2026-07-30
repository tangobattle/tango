//! Headless probe for DS replay playback: open a recorded bn5ds match
//! through the seam's [`tango_match::ReplaySet`] and drive it the way
//! the app does — display pair AND stats pass side by side, with a
//! backward seek — checking the result against a clean single-pair
//! reference run of the same recording.
//!
//! The two-pair shape is the point: melonds-rollback used to route the
//! platform callbacks through a single global slot, so booting the
//! stats pass cut the display pair off the air mid-replay and the game
//! desynced from the recording. The frame-hash comparison at the end
//! is what catches that class of bug.
//!
//! Usage: replay_probe <rom.nds> <replay.tangoreplay> [ticks]

use std::hash::{Hash, Hasher};
use std::sync::Arc;



/// Which build the recording is of, off its own metadata — the two
/// builds are separate families, and a JP replay walked with the US
/// layout primes into the weeds.
fn game_of(replay: &tango_replay::Replay) -> &'static tango_gamesupport::Game {
    let family = replay
        .metadata
        .side(replay.local_player_index)
        .and_then(|s| s.game_info.as_ref())
        .map(|gi| gi.rom_family.as_str())
        .expect("replay carries no game info");
    match family {
        "bn5ds" => &tango_gamesupport_bn5ds::BN5DS,
        "exe5ds" => &tango_gamesupport_bn5ds::EXE5DS,
        other => panic!("not this crate's replay: {other}"),
    }
}

fn open_set(
    rom: &[u8],
    replay: &tango_replay::Replay,
    inputs: &Arc<Vec<[tango_match::HostInput; 2]>>,
) -> tango_match::ReplaySet {
    let game = game_of(replay);
    game.pvp
        .open_replay(tango_match::ReplayConfig {
            roms: [rom.to_vec(), rom.to_vec()],
            saves: replay.srams.clone(),
            session_payloads: tango_match::parse_session_payloads([game.pvp, game.pvp], &replay.session_payloads())
                .expect("session payloads"),
            inputs: inputs.clone(),
            rng_seed: replay.rng_seed,
            rtc: replay.rtc_time(),
            match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            local_player: replay.local_player_index as usize,
            peer_rom: tango_match::PeerRom {
                code: *game.rom_code,
                revision: game.revision,
            },
            want_stats: true,
            disable_bgm: false,
        })
        .expect("open_replay")
}

fn frame_hash(frames: &tango_match::LiveFrames) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    frames.frames.hash(&mut h);
    h.finish()
}

fn seek(pb: &mut tango_match::Replay, target: u32) {
    let ctrl = tango_match::seek::SeekController::new();
    ctrl.request(target, false);
    loop {
        match pb.seek_step(&ctrl, 64, &mut |_| {}, &mut |_| {}, &mut || {}) {
            tango_match::SeekStep::Working => continue,
            tango_match::SeekStep::Landed => break,
            tango_match::SeekStep::Idle => panic!("seek went idle before landing"),
        }
    }
    assert_eq!(pb.cursor(), target, "the chase should land on the target");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let f = std::fs::File::open(&args[1]).expect("replay unreadable");
    let replay = tango_replay::Replay::decode(std::io::BufReader::new(f)).expect("replay undecodable");
    let check: u32 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(u32::MAX);

    // Widen the recorded rows into the seam's vocabulary, exactly as
    // the app's replay session does.
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
    let total = inputs.len() as u32;
    let check = check.min(total);
    println!(
        "replay: {total} ticks, local player {}, complete: {}",
        replay.local_player_index, replay.is_complete
    );

    // Reference: one pair, straight through to the checkpoint.
    let started = std::time::Instant::now();
    let reference = {
        let set = open_set(&rom, &replay, &inputs);
        let mut pb = set.playback().expect("reference boot");
        while pb.cursor() < check && pb.step() {}
        assert_eq!(pb.cursor(), check);
        let frames = pb.frames();
        assert!(
            frames.frames.iter().all(|fb| !fb.is_empty()),
            "both consoles should render"
        );
        // A deterministic wrong picture hashes as stably as a right one,
        // so leave the checkpoint on disk where a human can look at it.
        if let Ok(dir) = std::env::var("FRAME_DUMP_DIR") {
            for (seat, fb) in frames.frames.iter().enumerate() {
                let img = image::RgbaImage::from_raw(512, 192, fb.clone()).expect("framebuffer shape");
                img.save(format!("{dir}/check-t{check}-s{seat}.png")).expect("frame dump");
            }
        }
        frame_hash(&frames)
    };
    println!("reference pass done in {:.1?}", started.elapsed());

    // The app's shape: display pair and stats pass on one recording at
    // the same time, plus a backward seek through the chase.
    let set = open_set(&rom, &replay, &inputs);
    let mut pb = set.playback().expect("playback boot");
    while pb.cursor() < check / 4 && pb.step() {}

    // Second pair on the air — this is the boot that used to cut the
    // display pair's wireless dead. Through the app's own entry point:
    // a bare pair landed on the display pair's primed capture, the
    // landing whose raster reconstruction landing_probe drills.
    let mut stats = set.stats_reusing_playback().expect("stats boot");
    println!("both pairs up at display tick {}", pb.cursor());

    // Race the stats pass ahead (like the prefetch worker), then finish
    // the display pair's run with a seek in the middle.
    while stats.step(64).expect("stats step") && stats.progress() < check {}
    assert!(
        stats.progress() >= check.min(total),
        "the stats pass should cover the checkpoint"
    );
    while pb.cursor() < check / 2 && pb.step() {}
    seek(&mut pb, check / 4);
    while pb.cursor() < check && pb.step() {}
    let frames = pb.frames();
    assert_eq!(
        frame_hash(&frames),
        reference,
        "tick {check} after two pairs + a seek must match the clean run — a mismatch is a desync"
    );
    println!("two-pair + seek pass matches reference at tick {check} ({:.1?} total)", started.elapsed());

    // The hover thumbnail's lookup still stands.
    let cap = pb.nearest_capture(check).expect("capture near the playhead");
    assert!(cap.tick() <= check, "capture can't be past the playhead");
    assert!(
        cap.frames.frames.iter().all(|fb| !fb.is_empty()),
        "captures should carry both framebuffers"
    );
    println!("SUCCESS");
}
