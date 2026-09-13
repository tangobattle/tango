use super::*;
use std::sync::{atomic::AtomicBool, Arc};
use tango_match::gamemode::Program;
use tango_match::Link as _;
use tango_script::{Package, PackageRef, PreparedGameMode, Profile};

fn rom() -> Vec<u8> {
    let mut rom = mgba_rollback::testrom::build_idle();
    // ARM entry loads a Thumb address and branches to it.
    for (offset, word) in [
        (0xc0, 0xe59f0000u32),
        (0xc4, 0xe12fff10),
        (0xc8, 0x080000cd),
        (0xf0, 0x04000004),
    ] {
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    // r4 increments once per VBlank. The hook is on that instruction, not on
    // either polling loop, so it runs once per emulated video frame.
    for (offset, half) in [
        (0xcc, 0x2400u16),
        (0xce, 0x4808),
        (0xd0, 0x8801),
        (0xd2, 0x2201),
        (0xd4, 0x4211),
        (0xd6, 0xd1fb),
        (0xd8, 0x8801),
        (0xda, 0x4211),
        (0xdc, 0xd0fc),
        (0xde, 0x3401),
        (0xe0, 0xe7f6),
    ] {
        rom[offset..offset + 2].copy_from_slice(&half.to_le_bytes());
    }
    rom
}

const SOURCE: &str = r#"--!strict
return {
    setup = function(context: GameModeContext, game: Game)
        game.hook(0x080000de, function()
            local count = buffer.readu32(game.read("main", 0x02000000, 4), 0) + 1
            local fail = buffer.readu8(game.read("main", 0x02000004, 1), 0)
            if fail == 1 then error("intentional hook failure") end
            local bytes = buffer.create(4)
            buffer.writeu32(bytes, 0, count)
            game.write("main", 0x02000000, bytes)
            game.write_register("r5", count)
            assert(game.read_register("r5") == count)
            assert(buffer.readu32(game.read("main", 0x02000000, 4), 0) == count)
            assert(buffer.readu32(game.read("main", 0x02040000, 4), 0) == count)
            local boundary = game.read("main", 0x0203fffe, 6)
            assert(buffer.readu32(boundary, 2) == count)
            if fail == 2 then game.write_register("thumb_pc", 0xffffffff) end
            if fail == 3 or fail == 4 then game.write_register("thumb_pc", 0x080000de) end
            if fail == 3 then return end
            if fail == 4 then
                for _ = 1, 128 do game.round_started() end
                return
            end
            game.ready()
            if context.player == 1 then
                if count == 1 or count == 3 then game.round_started() end
                if count == 2 then game.round_outcome(1) end
                if count == 4 then game.match_ended() end
            end
        end)
    end,
}
"#;

fn profile() -> Profile {
    let package = Package::load(
        [
            (
                "package.toml".into(),
                b"api=1\nname='boot-test'\nversion='1.0.0'\ndefault_gamemode='main'\n[[gamemode]]\nname='main'\npath='./init'\n".to_vec(),
            ),
            ("init.luau".into(), SOURCE.as_bytes().to_vec()),
        ]
        .into(),
    )
    .unwrap();
    Profile::resolve(
        &[package],
        &PackageRef {
            name: "boot-test".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
}
fn boot(seed: u8) -> crate::gamemode::Booted {
    let prepared =
        PreparedGameMode::prepare(profile(), crate::gamemode::runtime(), rom(), false, Default::default()).unwrap();
    let programs = [1, 2].map(|player| Box::new(prepared.program(player, [seed; 16]).unwrap()) as Box<dyn Program>);
    crate::gamemode::boot(
        [prepared.rom().to_vec(), prepared.rom().to_vec()],
        [vec![], vec![]],
        programs,
        std::time::UNIX_EPOCH,
        false,
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn counters(link: &mut Link) -> [u32; 2] {
    [0, 1].map(|player| {
        let core = link.inner.core_mut(player);
        let count = core.raw_read_32(0x02000000, -1);
        assert_eq!(core.gba().cpu().gpr(5) as u32, count);
        count
    })
}

fn session_backend() -> crate::gamemode::Backend {
    let prepared = Arc::new(
        PreparedGameMode::prepare(profile(), crate::gamemode::runtime(), rom(), false, Default::default()).unwrap(),
    );
    crate::gamemode::Backend::new([prepared.clone(), prepared]).unwrap()
}

#[test]
fn package_backend_starts_the_shared_match_without_native_registration() {
    use tango_match::Backend as _;
    let backend = session_backend();
    let rom = rom();
    let config = || tango_match::StartConfig {
        record_factory: None,
        roms: [&rom, &rom],
        saves: [None, None],
        rng_seed: [7; 16],
        rtc: std::time::UNIX_EPOCH,
        match_type: 0,
        peer_rom: None,
        local_player: 0,
        present_delay: 0,
        disable_bgm: false,
        audio: None,
        cancel: None,
    };
    let mut session = backend.start(config()).unwrap();
    for _ in 0..5 {
        session.add_remote_input(HostInput::default(), 0);
        session.advance(HostInput::default()).unwrap();
    }
    assert!(session
        .telemetry()
        .unwrap()
        .lock()
        .unwrap()
        .events()
        .iter()
        .any(|(_, event)| *event == tango_match::telemetry::Event::MatchEnded));
    let mut bad = config();
    bad.roms[1] = &[0];
    assert!(backend.start(bad).is_err());
    let mut bad = config();
    bad.match_type = 1;
    assert!(backend.start(bad).is_err());
    let cancel = AtomicBool::new(true);
    let mut cancelled = config();
    cancelled.cancel = Some(&cancel);
    assert!(matches!(backend.start(cancelled), Err(tango_match::Error::Cancelled)));
}

#[test]
fn package_replay_workers_restore_unprimed_cores_and_keep_lifecycle() {
    use tango_match::Backend as _;
    let backend = session_backend();
    let config = tango_match::ReplayConfig {
        record_factory: None,
        roms: [rom(), rom()],
        saves: Default::default(),
        inputs: Arc::new(vec![[HostInput::default(); 2]; 8]),
        rng_seed: [7; 16],
        rtc: std::time::UNIX_EPOCH,
        match_type: 0,
        local_player: 0,
        peer_rom: None,
        want_stats: true,
        disable_bgm: false,
    };
    let set = backend.open_replay(config).unwrap();
    let mut playback = set.playback().unwrap();
    // This entry point requires landing on the display capture; it cannot
    // quietly re-prime and conceal an incomplete unprimed boot implementation.
    let mut landed = set.playback_landed().unwrap();
    let mut stats = set.stats_reusing_playback().unwrap();
    for _ in 0..8 {
        assert_eq!(playback.step().unwrap(), landed.step().unwrap());
        assert_eq!(playback.cursor(), landed.cursor());
        assert_eq!(playback.frames().frames, landed.frames().frames);
        let tick = playback.cursor();
        let original = playback.nearest_capture(tick).unwrap();
        let restored = landed.nearest_capture(tick).unwrap();
        assert_eq!(
            emulator_snapshot(original.snapshot()).unwrap().digest(),
            emulator_snapshot(restored.snapshot()).unwrap().digest()
        );
    }
    assert!(!stats.step(16).unwrap());
    assert_eq!(
        stats
            .finish()
            .unwrap()
            .rounds
            .iter()
            .map(|round| round.start)
            .collect::<Vec<_>>(),
        [0, 2]
    );
}

#[test]
fn package_hooks_run_in_real_cores_and_rewind_with_lifecycle() {
    let mut booted = boot(7);
    let initial = booted.link.snapshot(None).unwrap();
    assert_eq!(counters(&mut booted.link), [1, 1]);
    assert_eq!(
        booted.telemetry.lock().unwrap().events(),
        &[(0, tango_match::telemetry::Event::RoundStarted)]
    );
    booted.link.tick([HostInput::default(); 2]).unwrap();
    let capture = booted.link.snapshot(None).unwrap();
    for _ in 0..2 {
        booted.link.tick([HostInput::default(); 2]).unwrap();
    }
    let expected = counters(&mut booted.link);
    let events = booted.telemetry.lock().unwrap().events().to_vec();
    assert_eq!(expected, [4, 4]);
    booted.link.restore(&capture).unwrap();
    for _ in 0..2 {
        booted.link.tick([HostInput::default(); 2]).unwrap();
    }
    assert_eq!(counters(&mut booted.link), expected);
    assert_eq!(booted.telemetry.lock().unwrap().events(), events);
    booted.link.restore(&initial).unwrap();
    assert_eq!(
        booted.telemetry.lock().unwrap().events(),
        &[(0, tango_match::telemetry::Event::RoundStarted)]
    );
    booted.link.tick([HostInput::default(); 2]).unwrap();
    assert_eq!(counters(&mut booted.link), [2, 2]);

    let mut other = boot(7);
    other.link.restore(&capture).unwrap();
    other.link.tick([HostInput::default(); 2]).unwrap();
    assert_eq!(counters(&mut other.link), [3, 3]);
    other.link.tick([HostInput::default(); 2]).unwrap();
    assert_eq!(other.telemetry.lock().unwrap().events(), events);
    let mut incompatible = boot(8);
    assert!(incompatible.link.restore(&capture).is_err());
    assert_eq!(counters(&mut incompatible.link), [1, 1]);
    let native_pair = mgba_rollback::Link::new(vec![rom(), rom()]).unwrap();
    let mut native = Link::new(native_pair, None);
    assert!(native.restore(&capture).is_err());
    assert!(other.link.restore(&native.snapshot(None).unwrap()).is_err());
}

#[test]
fn failed_hooks_stop_frames_and_do_not_commit_partial_writes() {
    for fail in [1, 2, 3, 4] {
        let mut booted = boot(7);
        let healthy = booted.link.snapshot(None).unwrap();
        booted.link.inner.core_mut(0).raw_write_8(0x02000004, -1, fail);
        let error = booted.link.tick([HostInput::default(); 2]).unwrap_err();
        assert!(
            error.to_string().contains(if fail == 4 {
                "lifecycle event limit"
            } else if fail == 3 {
                "call limit"
            } else if fail == 2 {
                "program counter"
            } else {
                "intentional"
            }),
            "{error}"
        );
        assert_eq!(booted.link.live_tick, 0);
        if fail <= 2 {
            assert_eq!(counters(&mut booted.link)[0], 1);
        }
        assert!(booted.link.snapshot(None).is_err());
        assert!(booted.link.tick([HostInput::default(); 2]).is_err());
        assert_eq!(
            booted.telemetry.lock().unwrap().events(),
            &[(0, tango_match::telemetry::Event::RoundStarted)]
        );
        booted.link.restore(&healthy).unwrap();
        booted.link.tick([HostInput::default(); 2]).unwrap();
        assert_eq!(counters(&mut booted.link), [2, 2]);
    }
}

#[test]
fn replay_failure_keeps_the_cursor_and_recovers_only_from_a_capture() {
    let mut booted = boot(7);
    booted.link.inner.core_mut(0).raw_write_8(0x02000004, -1, 2);
    let mut playback = tango_match::Playback::new(Box::new(booted.link), Arc::new(vec![[HostInput::default(); 2]; 3]));
    assert!(playback.step().is_err());
    assert_eq!(playback.cursor(), 0);
    assert!(playback.step_muted().is_err());
    assert!(playback.capture().is_err());
    let mut clean = tango_match::Playback::new(Box::new(boot(7).link), Arc::new(vec![]));
    playback.load(&clean.capture().unwrap()).unwrap();
    assert!(playback.step().unwrap());
    assert_eq!(playback.cursor(), 1);
}
