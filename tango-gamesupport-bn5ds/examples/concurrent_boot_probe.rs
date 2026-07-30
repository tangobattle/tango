//! Boot two replay pairs at the same time, the way the app's playback
//! and prefetch workers do — the shape the sequential replay_probe
//! never exercises. If priming's board leg (the wireless association)
//! only fails when two pairs walk concurrently, the fault is shared
//! state between links, not the walk.
//!
//! Usage: concurrent_boot_probe <rom.nds> <replay.tangoreplay> [rtc-unix-secs]
//!
//! The optional third argument overrides the recording's match clock,
//! for telling a clock-dependent boot failure from a save-dependent one.

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

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let f = std::fs::File::open(&args[1]).expect("replay unreadable");
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
    let set: Arc<tango_match::ReplaySet> = Arc::new(
        game.pvp
            .open_replay(tango_match::ReplayConfig {
                roms: [rom.to_vec(), rom.to_vec()],
                saves: replay.srams.clone(),
                session_payloads: tango_match::parse_session_payloads([game.pvp, game.pvp], &replay.session_payloads())
                    .expect("session payloads"),
                inputs: inputs.clone(),
                rng_seed: replay.rng_seed,
                rtc: match args.get(2) {
                    Some(secs) => std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs.parse().unwrap()),
                    None => replay.rtc_time(),
                },
                match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
                local_player: replay.local_player_index as usize,
                peer_rom: tango_match::PeerRom {
                    code: *game.rom_code,
                    revision: game.revision,
                },
                want_stats: false,
                want_round_marks: false,
                disable_bgm: false,
            })
            .expect("open_replay"),
    );

    // The app's shape: playback boots on one worker while the prefetch's
    // stats pass boots on another, both priming at once. With X2=1 in
    // the environment, a second whole set boots alongside — four pairs
    // priming concurrently, the load the app reaches when a playing
    // session's pairs are still hot while the next session opens.
    let sets = if std::env::var("X2").is_ok() {
        vec![set.clone(), set]
    } else {
        vec![set]
    };
    let mut threads = Vec::new();
    for (i, set) in sets.iter().cloned().enumerate() {
        let stats_set = set.clone();
        threads.push(std::thread::spawn(move || match stats_set.stats() {
            Ok(_) => println!("set{i} stats boot: OK"),
            Err(e) => println!("set{i} stats boot: FAILED: {e:?}"),
        }));
        threads.push(std::thread::spawn(move || match set.playback() {
            Ok(_) => println!("set{i} playback boot: OK"),
            Err(e) => println!("set{i} playback boot: FAILED: {e:?}"),
        }));
    }
    for t in threads {
        t.join().unwrap();
    }
}
