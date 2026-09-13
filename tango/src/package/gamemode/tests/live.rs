//! Optional local-ROM check through the desktop's actual lobby and session path.
use super::*;
use std::time::{Duration, Instant};
use tango_session::Session as _;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TANGO_TEST_ROM and TANGO_TEST_SAVE; connects two local UDP peers"]
async fn selected_gamemode_runs_and_records_through_desktop_handoff() -> anyhow::Result<()> {
    run(false).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TANGO_TEST_ROM and TANGO_TEST_SAVE; connects two local UDP peers"]
async fn selected_gamemode_runs_without_any_native_registration() -> anyhow::Result<()> {
    run(true).await
}

async fn run(unregistered: bool) -> anyhow::Result<()> {
    let mut rom = std::fs::read(std::env::var("TANGO_TEST_ROM")?)?;
    let save = std::fs::read(std::env::var("TANGO_TEST_SAVE")?)?;
    if unregistered {
        // Change only unused cartridge padding in our in-memory copy. The
        // native CRC registry now rejects it; the package still recognizes it.
        let tail = rom.last_mut().ok_or_else(|| anyhow::anyhow!("empty ROM"))?;
        anyhow::ensure!(*tail == 0xff, "test fixture needs trailing padding");
        *tail = 0xfe;
        assert!(crate::library::game::detect(&mut rom).is_none());
    }
    let root = std::env::temp_dir().join(format!("tango-gamemode-live-{:016x}", rand::random::<u64>()));
    std::fs::create_dir(&root)?;
    let configs = ["host", "peer"].map(|name| crate::config::Config {
        library: tango_library::config::Config::with_data_path(root.join(name)),
        ..Default::default()
    });
    let scanners = Scanners::new();
    let rom_id = crate::library::rom::Id::of(&rom);
    assert!(crate::library::game::detect(&mut rom).is_none());
    let path = configs[0].roms_path().join("custom.gba");
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, &rom)?;
    let listing = crate::library::storage().list(&[configs[0].roms_path()]).await;
    scanners.roms.rescan(|| {
        Some(crate::library::rom::scan_roms(
            crate::library::storage(),
            &listing,
            &configs[0].roms_path(),
        ))
    });
    assert!(scanners.roms.read().image(rom_id).unwrap().native_game.is_none());
    scanners.rescan_packages(&configs[0], &Default::default());
    // Exercise the same cartridge/save pickers and editor snapshot as the UI.
    let save_path = configs[0].saves_path().join("selected.sav");
    std::fs::create_dir_all(save_path.parent().unwrap())?;
    std::fs::write(&save_path, &save)?;
    let listing = crate::library::storage().list(&[configs[0].saves_path()]).await;
    scanners
        .saves
        .rescan(|| Some(crate::library::save::scan_saves(crate::library::storage(), &listing)));
    let selections = [false, true].map(|disable_bgm| {
        let mut config = configs[0].clone();
        config.disable_bgm_in_pvp = disable_bgm;
        let option = crate::loadout::game_options(&config.language, &scanners)
            .into_iter()
            .find(|option| option.choice == crate::loadout::GameChoice::Package(rom_id))
            .unwrap();
        let mut loadout = crate::loadout::Loadout::default();
        loadout.update(crate::loadout::Message::GameSelected(option), &scanners, &config);
        let mut loaded = None;
        loadout.refresh_package(&scanners, &config, &mut loaded);
        assert!(loadout.game.is_none());
        assert_eq!(loadout.gamemodes.selected.as_ref().unwrap().name, "triple");
        assert!(loadout.gamemodes.error.is_none(), "{:?}", loadout.gamemodes.error);
        assert!(loadout.package_save.error.is_none(), "{:?}", loadout.package_save.error);
        let options = crate::loadout::save_options(&loadout, &config.language, &scanners, &config);
        let selected = options.into_iter().find(|option| option.path == save_path).unwrap();
        loadout.update(crate::loadout::Message::SaveSelected(selected), &scanners, &config);
        loadout.refresh_package(&scanners, &config, &mut loaded);
        assert!(loadout.has_save(loaded.as_ref()));
        assert!(loaded.as_ref().unwrap().native_game.is_none());
        let sram = loadout.save_sram(loaded.as_ref()).unwrap();
        assert_eq!(sram, save);
        let settings = loadout.make_local_settings(&config, &Default::default());
        assert!(settings.game_info.is_none());
        (settings, sram)
    });
    let settings = selections.each_ref().map(|(settings, _)| settings.clone());
    let port = std::net::UdpSocket::bind("127.0.0.1:0")?.local_addr()?.port();
    let roles = [
        crate::netplay::DirectRole::Host { port },
        crate::netplay::DirectRole::Connect {
            addr: format!("127.0.0.1:{port}"),
        },
    ];
    let mut lobbies = [crate::netplay::State::default(), crate::netplay::State::default()];
    for (lobby, role) in lobbies.iter_mut().zip(roles) {
        let (cancel, progress) = lobby.begin_direct(&role);
        tokio::spawn(tango_lobby::connect_direct(role, cancel, progress));
    }
    let mut incoming = lobbies.each_ref().map(|lobby| lobby.take_incoming().unwrap());
    let mut handoffs = [None, None];
    let deadline = Instant::now() + Duration::from_secs(30);
    while handoffs.iter().any(Option::is_none) {
        anyhow::ensure!(Instant::now() < deadline, "local lobby handoff timed out");
        for player in 0..2 {
            let lobby = &mut lobbies[player];
            while let Ok(message) = incoming[player].try_recv() {
                lobby.apply(message);
            }
            anyhow::ensure!(
                !matches!(lobby.phase, crate::netplay::Phase::Failed { .. }),
                "{:?}",
                lobby.phase
            );
            if matches!(lobby.phase, crate::netplay::Phase::Lobby { .. }) && lobby.lobby.local.is_none() {
                lobby.send_local_settings(settings[player].clone());
            }
            if let (Some(local), Some(remote)) = (&lobby.lobby.local, &lobby.lobby.remote) {
                assert_eq!(
                    verdict(&scanners, local, remote),
                    crate::netplay::compat::Verdict::Compatible
                );
                if !lobby.local_ready() {
                    lobby.commit(selections[player].1.clone());
                }
            }
            if handoffs[player].is_none() {
                handoffs[player] = lobby.take_pre_match();
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let [host, peer] = handoffs.map(Option::unwrap);
    assert_ne!(host.local_player_index(), peer.local_player_index());
    let expected = if host.local_player_index() == 0 {
        [
            settings[0].gamemode.clone().unwrap(),
            settings[1].gamemode.clone().unwrap(),
        ]
    } else {
        [
            settings[1].gamemode.clone().unwrap(),
            settings[0].gamemode.clone().unwrap(),
        ]
    };
    let launch = |pre_match, config| {
        crate::session::spawn_pvp(
            scanners.clone(),
            config,
            crate::platform::audio::LateBinder::new(),
            None,
            None,
            pre_match,
        )
    };
    let (host, peer) = tokio::join!(launch(host, configs[0].clone()), launch(peer, configs[1].clone()));
    let host = host?;
    let peer = peer?;
    let sessions = [host, peer];
    for (session, panes, _, _) in &sessions {
        assert_eq!(session.backend().gamemodes(), Some(&expected));
        assert!(panes.local_loaded.is_some() && panes.opponent_loaded.is_some());
        if unregistered {
            assert!(panes.local_game.is_none());
            assert!(panes.local_loaded.as_ref().unwrap().native_game.is_none());
            assert!(panes.opponent_loaded.as_ref().unwrap().native_game.is_none());
        }
    }
    let deadline = Instant::now() + Duration::from_secs(45);
    let result = async {
        let mut live_since = None;
        loop {
            for (session, _, _, _) in &sessions {
                anyhow::ensure!(session.prime_error().is_none(), "{:?}", session.prime_error());
            }
            if sessions.iter().all(|(session, _, _, _)| {
                !session.is_booting() && !session.waiting_for_peer() && session.round_stats().is_some()
            }) {
                if live_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(3) {
                    break;
                }
            }
            anyhow::ensure!(Instant::now() < deadline, "package match did not advance");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        for (session, _, _, _) in &sessions {
            assert!(session.frame().chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]));
            assert_eq!(session.stats_snapshot().rounds.len(), 1);
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    for (session, _, _, _) in &sessions {
        session.request_close();
    }
    for (session, _, binding, thread) in sessions {
        tokio::time::timeout(Duration::from_secs(5), session.supervisor_done().cancelled()).await?;
        drop(binding);
        drop(session);
        tokio::task::spawn_blocking(move || thread.join().unwrap()).await?;
    }
    result?;
    for config in configs {
        let files: Vec<_> = std::fs::read_dir(config.replays_path())?.collect::<Result<_, _>>()?;
        assert_eq!(files.len(), 1);
        let replay = tango_replay::Replay::decode(std::fs::File::open(files[0].path())?)?;
        assert!(!replay.is_complete); // Closing an unfinished match preserves its partial recording.
        assert!(
            replay.inputs.len() >= 120,
            "only {} confirmed frames",
            replay.inputs.len()
        );
        assert_eq!(replay.metadata.gamemodes()?, Some(expected.clone()));
        assert_eq!(replay.srams, [save.clone(), save.clone()]);
        let loaded = crate::selection::for_replay_local(&scanners, &config, &replay)?;
        assert!(loaded.chips.is_empty()); // Package editor; no native model was constructed.
        let binder = crate::platform::audio::LateBinder::new();
        let (playback, presentation, binding, threads) =
            crate::session::build_playback(&scanners, &config, &binder, &files[0].path(), None, Vec::new())?;
        if unregistered {
            assert!(presentation.is_none());
            assert!(loaded.native_game.is_none());
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let result = async {
            while playback.current_tick() < 60 {
                anyhow::ensure!(playback.prime_error().is_none(), "{:?}", playback.prime_error());
                anyhow::ensure!(Instant::now() < deadline, "package playback did not advance");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            playback.set_paused(true);
            playback.seek_to(15, false);
            while playback.current_tick() != 15 || playback.pending_seek_target().is_some() {
                anyhow::ensure!(playback.prime_error().is_none(), "{:?}", playback.prime_error());
                anyhow::ensure!(Instant::now() < deadline, "package replay seek did not finish");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        drop(playback);
        drop(binding);
        for thread in threads {
            tokio::task::spawn_blocking(move || thread.join().unwrap()).await?;
        }
        result?;
    }
    eprintln!(
        "Package lobby, session, and replay handoff passed; recordings: {}",
        root.display()
    );
    Ok(())
}
