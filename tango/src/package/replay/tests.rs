use super::*;
use tango_script::Package;

fn package(ambiguous: bool) -> Package {
    let extra = if ambiguous {
        "[[gamemode]]\nname='other'\npath='./init'\n"
    } else {
        ""
    };
    Package::load([
        ("package.toml".into(), format!("api=1\nname='import-test'\nversion='1.0.0'\ndefault_gamemode='main'\n[[gamemode]]\nname='main'\npath='./init'\n{extra}").into_bytes()),
        ("init.luau".into(), br#"--!strict
return {
    platform = 'gba',
    detect_rom = function(rom: buffer): boolean return buffer.len(rom) == 1 end,
    import_replay = function(old: LegacyReplay, rom: buffer): ReplayImport?
        if old.game.rom_family ~= 'custom' or old.game.sim_version ~= 2 or old.game.patch ~= nil
            or old.match_type ~= 9 or old.match_subtype ~= 3
            or old.game.rom_variant ~= buffer.readu8(rom, 0) then return nil end
        return {options = {imported = true}}
    end,
    setup = function(_context: GameModeContext, game: Game) game.hook(0x08000000, game.ready) end,
} :: GameMode
"#.to_vec()),
    ].into()).unwrap()
}

fn install(scanners: &Scanners, packages: &[Package]) {
    scanners.packages.rescan(|| {
        Some(futures::executor::block_on(tango_library::package::Catalog::scan(
            crate::library::storage(),
            std::path::Path::new("unused"),
            &Default::default(),
            packages,
        )))
    });
    *scanners.package_roms.write().unwrap() = crate::package::Catalog::scan(scanners);
}

fn fixture() -> (Scanners, Arc<tango_replay::Replay>) {
    let scanners = Scanners::new();
    let mut roms = rom::Catalog::default();
    roms.insert(vec![42], None);
    roms.insert(vec![43], None);
    scanners.roms.rescan(|| Some(roms));
    install(&scanners, &[package(false)]);
    let [p1_side, p2_side] = [42, 43].map(|variant| {
        Some(tango_replay::metadata::Side {
            nickname: format!("Player {variant}"),
            reveal_setup: true,
            game_info: Some(tango_replay::metadata::GameInfo {
                rom_family: "custom".into(),
                rom_variant: variant,
                sim_version: 2,
                patch: None,
            }),
            ..Default::default()
        })
    });
    (
        scanners,
        Arc::new(tango_replay::Replay {
            metadata: tango_replay::Metadata {
                ts: 123,
                link_code: "recorded".into(),
                p1_side,
                p2_side,
                match_type: 9,
                match_subtype: 3,
            },
            local_player_index: 1,
            is_complete: true,
            rng_seed: [3; 16],
            srams: [vec![5], vec![6]],
            inputs: vec![[Default::default(); 2]],
        }),
    )
}

#[test]
fn imports_normalize_a_copy_and_pin_both_seats_before_session_construction() {
    let (scanners, original) = fixture();
    let resolved =
        crate::library::replays::resolve(&scanners, std::path::Path::new("unused"), original.clone()).unwrap();
    assert!(resolved.backend.is_package() && resolved.peer_rom.is_none());
    assert_eq!(resolved.roms, [vec![42], vec![43]]);
    let imported = &resolved.replay;
    assert_eq!(imported.metadata.match_type, 0);
    assert_eq!(imported.metadata.match_subtype, 0);
    assert_eq!(
        imported.metadata.container_version().unwrap(),
        tango_replay::PACKAGE_VERSION
    );
    assert_eq!(imported.srams, original.srams);
    assert_eq!(imported.inputs, original.inputs);
    assert_eq!(imported.rng_seed, original.rng_seed);
    assert_eq!(imported.local_player_index, original.local_player_index);
    for player in 0..2 {
        assert_eq!(
            imported.metadata.side(player).unwrap().nickname,
            original.metadata.side(player).unwrap().nickname
        );
        assert!(imported.metadata.side(player).unwrap().reveal_setup);
    }
    assert_eq!(original.metadata.match_type, 9);
    assert!(original.metadata.gamemodes().unwrap().is_none());
    let configurations = imported.metadata.gamemodes().unwrap().unwrap();
    assert_eq!(resolved.backend.gamemodes(), Some(&configurations));
    let again = resolve(&scanners, &imported.metadata).unwrap().unwrap();
    assert_eq!(again.backend.gamemodes(), Some(&configurations));
    let (session, workers, _) = tango_session::replay::ReplaySession::new(tango_session::replay::ReplaySessionArgs {
        backend: resolved.backend,
        match_type: 0,
        peer_rom: None,
        roms: resolved.roms.map(Arc::new),
        replay: resolved.replay,
        expected_fps: 60.0,
        sample_rate: 48000,
        show_pip: false,
        stats_job: None,
        round_boundaries: Vec::new(),
        record_factory: None,
    })
    .unwrap();
    drop(session);
    drop(workers);
}

#[test]
fn imports_reject_partial_ambiguous_and_stale_selections() {
    let (scanners, replay) = fixture();
    let imported = resolve(&scanners, &replay.metadata).unwrap().unwrap();
    let mut partial = replay.metadata.clone();
    partial
        .p2_side
        .as_mut()
        .unwrap()
        .game_info
        .as_mut()
        .unwrap()
        .sim_version += 1;
    assert!(resolve(&scanners, &partial).is_err());
    partial = replay.metadata.clone();
    partial.p1_side.as_mut().unwrap().gamemode = imported.metadata.p1_side.as_ref().unwrap().gamemode.clone();
    assert!(resolve(&scanners, &partial).is_err());
    install(&scanners, &[package(true)]);
    assert!(resolve(&scanners, &replay.metadata)
        .err()
        .unwrap()
        .to_string()
        .contains("multiple"));
    install(&scanners, &[]);
    assert!(resolve(&scanners, &replay.metadata).unwrap().is_none());
    assert!(resolve(&scanners, &imported.metadata).is_err());
}

#[test]
#[ignore = "requires TANGO_TEST_REPLAY, TANGO_TEST_ROMS, and TANGO_TEST_REPLAY_REFERENCE"]
fn legacy_replay_matches_native_frames_and_restores_captures() {
    let path = std::path::PathBuf::from(std::env::var_os("TANGO_TEST_REPLAY").unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let original = Arc::new(tango_replay::Replay::decode(std::io::Cursor::new(&bytes)).unwrap());
    assert!(original.metadata.gamemodes().unwrap().is_none());
    let reference = std::fs::read_to_string(std::env::var_os("TANGO_TEST_REPLAY_REFERENCE").unwrap()).unwrap();
    let digests: Vec<_> = reference.lines().collect();
    assert_eq!(digests.len(), original.inputs.len());
    let scanners = Scanners::new();
    let mut roms = rom::Catalog::default();
    for path in std::env::split_paths(&std::env::var_os("TANGO_TEST_ROMS").unwrap()) {
        roms.insert(std::fs::read(&path).unwrap(), Some(path));
    }
    scanners.roms.rescan(|| Some(roms));
    install(&scanners, crate::package::editor::bundled::packages().unwrap());
    let resolved =
        crate::library::replays::resolve(&scanners, std::path::Path::new("unused"), original.clone()).unwrap();
    let replay = resolved.replay.clone();
    assert!(replay.metadata.gamemodes().unwrap().is_some());
    assert!(resolved.local_game.is_none() && resolved.peer_rom.is_none());
    let set = resolved
        .backend
        .open_replay(tango_match::ReplayConfig {
            record_factory: None,
            roms: resolved.roms,
            saves: replay.srams.clone(),
            inputs: Arc::new(
                replay
                    .inputs
                    .iter()
                    .map(|row| row.map(|input| tango_match::HostInput::keys(input.keys as u32)))
                    .collect(),
            ),
            rng_seed: replay.rng_seed,
            rtc: replay.rtc_time(),
            match_type: 0,
            local_player: replay.local_player_index as usize,
            peer_rom: None,
            want_stats: true,
            disable_bgm: false,
        })
        .unwrap();
    let mut playback = set.linear(None).unwrap();
    for (tick, expected) in digests.iter().enumerate() {
        assert!(playback.step().unwrap());
        let capture = playback.capture().unwrap();
        let actual = format!(
            "{:?}",
            tango_backend_mgba::link::emulator_snapshot(capture.snapshot())
                .unwrap()
                .digest()
        );
        assert_eq!(&actual, expected, "frame {tick}");
        if tick == digests.len() / 2 {
            playback.step().unwrap();
            playback = set.linear(Some(&capture)).unwrap();
            assert_eq!(playback.cursor(), tick as u32 + 1);
        }
    }
    assert!(!playback.step().unwrap());
    let stats = set.analyze(&mut |_, _, _| {}, &Default::default()).unwrap();
    assert!(stats.rounds.iter().any(|round| round.outcome.is_some()));
    let config = crate::config::Config::default();
    for player in 0..2 {
        let editor = crate::selection::for_replay_player(&scanners, &config, &original, player).unwrap();
        assert!(editor.chips.is_empty());
        assert_eq!(editor.editor.sram(&editor).unwrap(), original.srams[player as usize]);
    }
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert!(original.metadata.gamemodes().unwrap().is_none());
    eprintln!(
        "Imported {} recorded frames; every native emulator digest matched, including capture restoration.",
        digests.len()
    );
}
