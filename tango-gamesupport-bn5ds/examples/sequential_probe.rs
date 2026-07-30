//! Play replays back to back in one process, the way an app session
//! does when the viewer closes one recording and opens the next.
//! Exercises whatever state survives a ReplaySet being dropped —
//! engine globals, air routing, anything process-wide.
//!
//! Usage: sequential_probe <rom.nds> <replay.tangoreplay>... [--overlap]
//!
//! Each replay is booted, stepped a few hundred ticks, and dropped
//! before the next opens. With --overlap, each set is kept alive while
//! the next boots instead.

use std::sync::Arc;

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

fn open(rom: &[u8], path: &str) -> tango_match::ReplaySet {
    let f = std::fs::File::open(path).expect("replay unreadable");
    let replay = tango_replay::Replay::decode(std::io::BufReader::new(f)).expect("replay undecodable");
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
    let game = game_of(&replay);
    game.pvp
        .open_replay(tango_match::ReplayConfig {
            roms: [rom.to_vec(), rom.to_vec()],
            saves: replay.srams.clone(),
            inputs,
            rng_seed: replay.rng_seed,
            rtc: replay.rtc_time(),
            match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            local_player: replay.local_player_index as usize,
            peer_rom: tango_match::PeerRom {
                code: *game.rom_code,
                revision: game.revision,
            },
            want_stats: true,
            want_round_marks: true,
            disable_bgm: false,
        })
        .expect("open_replay")
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let overlap = args
        .iter()
        .position(|a| a == "--overlap")
        .map(|i| {
            args.remove(i);
        })
        .is_some();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");

    let mut kept = Vec::new();
    for (i, path) in args[1..].iter().enumerate() {
        let name = path.rsplit('/').next().unwrap();
        println!("=== session {i}: {name}");
        let started = std::time::Instant::now();
        let set = open(&rom, path);
        let mut pb = set.playback().expect("playback boot");
        while pb.cursor() < 200 && pb.step() {}
        println!(
            "    session {i}: played to tick {} in {:.1?}",
            pb.cursor(),
            started.elapsed()
        );
        assert!(
            pb.frames().frames.iter().all(|fb| !fb.is_empty()),
            "both consoles should render"
        );
        if overlap {
            kept.push((set, pb));
        }
    }
    println!("SUCCESS ({} sessions{})", args[1..].len(), if overlap { ", overlapped" } else { "" });
}
