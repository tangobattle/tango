use super::*;
use crate::package::replay::resolve as for_replay;
use crate::package::Catalog;
use tango_script::Package;

mod live;

fn package() -> Package {
    Package::load(
        [
            (
                "package.toml".into(),
                br#"api=1
name='picker-test'
version='1.0.0'
default_gamemode='first'
[[gamemode]]
name='second'
path='./mode'
[[gamemode]]
name='first'
path='./mode'
"#
                .to_vec(),
            ),
            ("init.luau".into(), b"--!strict\nreturn {}".to_vec()),
            (
                "mode.luau".into(),
                br#"--!strict
return {
    platform = "gba",
    detect_rom = function(rom: buffer): boolean return buffer.len(rom) == 1 end,
    setup = function(_context: GameModeContext, game: Game)
        game.hook(0x08000000, game.ready)
    end,
} :: GameMode
"#
                .to_vec(),
            ),
            (
                "locales/en-US/package.ftl".into(),
                b"package-name = Test game\ngamemode-second = Second mode\ngamemode-first = First mode\n".to_vec(),
            ),
            (
                "locales/ja-JP/package.ftl".into(),
                "package-name = テスト\ngamemode-second = セカンド\n"
                    .as_bytes()
                    .to_vec(),
            ),
        ]
        .into(),
    )
    .unwrap()
}

fn scanners() -> (Scanners, [rom::Id; 2]) {
    let scanners = Scanners::new();
    let mut roms = rom::Catalog::default();
    let games = [roms.insert(vec![1], None), roms.insert(vec![2], None)];
    scanners.roms.rescan(|| Some(roms));
    let packages = futures::executor::block_on(tango_library::package::Catalog::scan(
        crate::library::storage(),
        std::path::Path::new("unused"),
        &Default::default(),
        &[package()],
    ));
    assert!(packages.issues().is_empty());
    scanners.packages.rescan(|| Some(packages));
    *scanners.package_roms.write().unwrap() = Catalog::scan(&scanners);
    (scanners, games)
}

#[test]
fn selector_preserves_named_modes_and_never_falls_back_after_removal() {
    let (scanners, games) = scanners();
    let mut loadout = crate::loadout::Loadout {
        package_rom: Some(games[0]),
        ..Default::default()
    };
    loadout
        .gamemodes
        .refresh(&scanners, loadout.rom(&scanners), false, false);
    assert_eq!(
        loadout
            .gamemodes
            .choices
            .iter()
            .map(|c| c.reference.name.as_str())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );
    assert_eq!(
        loadout.gamemodes.choices[0].label("ja-JP"),
        "テスト · セカンド (v1.0.0)"
    );
    assert_eq!(loadout.gamemodes.selected.as_ref().unwrap().name, "first");
    let first = loadout.gamemodes.choices[0].reference.clone();
    loadout
        .gamemodes
        .select(first.clone(), &scanners, loadout.rom(&scanners), false);
    let selected = loadout.gamemodes.prepared.as_ref().unwrap().configuration().unwrap();
    assert_eq!(selected.identity.export, "second");
    loadout
        .gamemodes
        .refresh(&scanners, loadout.rom(&scanners), false, true);
    assert_eq!(loadout.gamemodes.selected.as_ref(), Some(&first));
    assert_ne!(
        loadout.gamemodes.prepared.as_ref().unwrap().configuration().unwrap(),
        selected
    );
    scanners.packages.rescan(|| Some(Default::default()));
    *scanners.package_roms.write().unwrap() = Catalog::scan(&scanners);
    loadout
        .gamemodes
        .refresh(&scanners, loadout.rom(&scanners), false, true);
    assert_eq!(loadout.gamemodes.selected.as_ref(), Some(&first));
    assert!(loadout.gamemodes.prepared.is_none() && loadout.gamemodes.error.is_some());
    let settings = loadout.make_local_settings(&Default::default(), &Default::default());
    assert!(settings.game_info.is_none());
}

#[test]
fn lobby_resolves_each_frozen_seat_and_rejects_different_or_stale_modes() {
    use crate::netplay::compat::Verdict;
    let (scanners, games) = scanners();
    let mut loadouts = games.map(|game| crate::loadout::Loadout {
        package_rom: Some(game),
        ..Default::default()
    });
    for (i, loadout) in loadouts.iter_mut().enumerate() {
        loadout
            .gamemodes
            .refresh(&scanners, loadout.rom(&scanners), false, i == 1);
    }
    let [local, remote] = loadouts
        .each_ref()
        .map(|loadout| loadout.make_local_settings(&Default::default(), &Default::default()));
    assert_eq!(verdict(&scanners, &local, &remote), Verdict::Compatible);
    assert_ne!(local.gamemode, remote.gamemode); // Different ROMs and per-seat settings are frozen separately.
    let mut changed = remote.clone();
    changed.gamemode.as_mut().unwrap().identity.export = "second".into();
    assert_eq!(verdict(&scanners, &local, &changed), Verdict::DifferentMatchTypes);
    changed = remote.clone();
    changed.gamemode.as_mut().unwrap().identity.environment[0] ^= 1;
    assert_eq!(verdict(&scanners, &local, &changed), Verdict::DifferentVersions);
    changed = remote.clone();
    changed.gamemode = None;
    assert_eq!(verdict(&scanners, &local, &changed), Verdict::DifferentVersions);
    let mut roms = rom::Catalog::default();
    roms.insert(vec![3], None);
    roms.insert(vec![2], None);
    scanners.roms.rescan(|| Some(roms));
    *scanners.package_roms.write().unwrap() = Catalog::scan(&scanners);
    assert_eq!(verdict(&scanners, &local, &remote), Verdict::DifferentVersions);
}

#[test]
fn replay_uses_recorded_exports_and_requires_their_exact_backend() {
    let (scanners, games) = scanners();
    let mut selection = Selection::default();
    selection.refresh(&scanners, Some(games[1]), false, true);
    selection.select(selection.choices[0].reference.clone(), &scanners, Some(games[1]), true);
    let p0 = selection.prepared.as_ref().unwrap().configuration().unwrap();
    selection.refresh(&scanners, Some(games[0]), false, false);
    selection.select(selection.choices[0].reference.clone(), &scanners, Some(games[0]), false);
    let p1 = selection.prepared.as_ref().unwrap().configuration().unwrap();
    let [p0_side, p1_side] = [&p0, &p1].map(|configuration| {
        Some(tango_replay::metadata::Side {
            gamemode: configuration.encode().unwrap(),
            ..Default::default()
        })
    });
    let mut metadata = tango_replay::Metadata {
        p1_side: p0_side,
        p2_side: p1_side,
        ..Default::default()
    };
    let resolved = for_replay(&scanners, &metadata).unwrap().unwrap();
    assert_eq!(resolved.roms, [games[1], games[0]]);
    assert_eq!(resolved.backend.gamemodes(), Some(&[p0, p1]));
    assert_eq!(resolved.seats[0].identity().export, "second");

    let open = |backend, metadata| {
        tango_session::replay::ReplaySession::new(tango_session::replay::ReplaySessionArgs {
            backend,
            match_type: 0,
            peer_rom: None,
            roms: resolved.seats.each_ref().map(|seat| Arc::new(seat.rom().to_vec())),
            replay: Arc::new(tango_replay::Replay {
                metadata,
                is_complete: false,
                local_player_index: 1,
                rng_seed: [0; 16],
                srams: Default::default(),
                inputs: vec![[Default::default(); 2]],
            }),
            expected_fps: 60.0,
            sample_rate: 48000,
            show_pip: false,
            stats_job: None,
            round_boundaries: Vec::new(),
            record_factory: None,
        })
    };
    use tango_session::SessionBackend;
    let reversed = Arc::new(
        tango_backend_mgba::gamemode::Backend::new([resolved.seats[1].clone(), resolved.seats[0].clone()]).unwrap(),
    );
    assert!(open(SessionBackend::Owned(reversed), metadata.clone()).is_err());
    assert!(open(SessionBackend::Owned(resolved.backend.clone()), metadata.clone()).is_ok());
    let mut changed = metadata.gamemodes().unwrap().unwrap()[0].clone();
    changed.identity.environment[0] ^= 1;
    metadata.p1_side.as_mut().unwrap().gamemode = changed.encode().unwrap();
    assert!(open(SessionBackend::Owned(resolved.backend.clone()), metadata.clone()).is_err());
    assert!(for_replay(&scanners, &metadata).is_err());
    metadata.p1_side.as_mut().unwrap().gamemode.clear();
    assert!(for_replay(&scanners, &metadata).is_err());
    metadata.p2_side.as_mut().unwrap().gamemode.clear();
    assert!(open(SessionBackend::Owned(resolved.backend), metadata).is_err());
}

#[test]
fn package_discovery_and_replay_resolution_accept_unregistered_roms() {
    let (scanners, _) = scanners();
    let mut roms = rom::Catalog::default();
    let ids = [
        roms.insert(vec![1], Some("custom/a.rom".into())),
        roms.insert(vec![2], Some("custom/b.rom".into())),
    ];
    scanners.roms.rescan(|| Some(roms));
    *scanners.package_roms.write().unwrap() = Catalog::scan(&scanners);
    let configurations = ids.map(|id| {
        let mut selected = Selection::default();
        selected.refresh(&scanners, Some(id), false, false);
        assert_eq!(selected.selected.as_ref().unwrap().name, "first");
        selected.prepared.as_ref().unwrap().configuration().unwrap()
    });
    let [p1_side, p2_side] = configurations.each_ref().map(|configuration| {
        Some(tango_replay::metadata::Side {
            gamemode: configuration.encode().unwrap(),
            ..Default::default()
        })
    });
    let replay = tango_replay::Replay {
        metadata: tango_replay::Metadata {
            p1_side,
            p2_side,
            ..Default::default()
        },
        is_complete: false,
        local_player_index: 1,
        rng_seed: [0; 16],
        srams: Default::default(),
        inputs: vec![[Default::default(); 2]],
    };
    let resolved =
        crate::library::replays::resolve(&scanners, std::path::Path::new("unused"), Arc::new(replay)).unwrap();
    assert!(resolved.local_game.is_none());
    assert!(resolved.peer_rom.is_none());
    assert_eq!(resolved.roms, [vec![1], vec![2]]);
    assert_eq!(resolved.backend.gamemodes(), Some(&configurations));
    let (session, workers, _) = tango_session::replay::ReplaySession::new(tango_session::replay::ReplaySessionArgs {
        backend: resolved.backend,
        match_type: resolved.match_type,
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
