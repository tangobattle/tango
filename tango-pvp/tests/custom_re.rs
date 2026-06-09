//! Throwaway reverse-engineering harness (not a real test): drive the BN6
//! golden replay headlessly and dump EWRAM every tick across the opening
//! chip-select screen, plus the per-tick local/remote joyflags. Used to
//! locate the "in custom screen" flag + frame counter and the confirm input.
//!
//! Run: TANGO_TEST_ROMS_DIR=~/Documents/Tango/roms \
//!      cargo test --release -p tango-pvp --test custom_re -- --nocapture re_dump

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn roms_dir() -> PathBuf {
    std::env::var_os("TANGO_TEST_ROMS_DIR")
        .map(PathBuf::from)
        .expect("set TANGO_TEST_ROMS_DIR")
}

fn discover_roms() -> HashMap<(&'static str, u8), Vec<u8>> {
    let mut out = HashMap::new();
    for entry in std::fs::read_dir(roms_dir()).unwrap().flatten() {
        let p = entry.path();
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "gba" && ext != "srl" {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else { continue };
        if let Some(g) = tango_gamedb::detect(&bytes) {
            out.insert(g.family_and_variant(), bytes);
        }
    }
    out
}

// Throwaway RE harness, not a regression test: needs TANGO_TEST_ROMS_DIR and
// writes dumps to /tmp/re. Run explicitly with `--ignored` to re-derive offsets.
#[test]
#[ignore]
fn re_dump() {
    let roms = discover_roms();
    let replay_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/20260516051717-test123-bn6-vs-weenie-p1.tangoreplay"
    );
    let replay = tango_pvp::replay::Replay::decode(&mut std::fs::File::open(replay_path).unwrap()).unwrap();

    let local = replay.metadata.local_side.as_ref().unwrap().game_info.as_ref().unwrap();
    let remote = replay.metadata.remote_side.as_ref().unwrap().game_info.as_ref().unwrap();
    let local_game =
        tango_gamedb::find_by_family_and_variant(&local.rom_family, local.rom_variant as u8).unwrap();
    let remote_game =
        tango_gamedb::find_by_family_and_variant(&remote.rom_family, remote.rom_variant as u8).unwrap();
    let local_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(local_game).unwrap();
    let remote_hooks = tango_pvp::hooks::hooks_for_gamedb_entry(remote_game).unwrap();
    let local_rom = roms.get(&local_game.family_and_variant()).expect("local rom");
    let remote_rom = roms.get(&remote_game.family_and_variant()).expect("remote rom");
    eprintln!(
        "local {:?} remote {:?}",
        local_game.family_and_variant(),
        remote_game.family_and_variant()
    );

    let mut core = mgba::core::Core::new_gba("re", &mgba::core::Options { ..Default::default() }).unwrap();
    core.enable_video_buffer();
    core.as_mut()
        .load_rom(mgba::vfile::VFile::from_vec(local_rom.to_vec()))
        .unwrap();
    core.as_mut()
        .load_save(mgba::vfile::VFile::from_vec(replay.local_sram_dump()))
        .unwrap();
    let replay_time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(replay.metadata.ts);
    core.set_rtc_fixed(replay_time);
    core.as_mut().reset();
    local_hooks.patch(core.as_mut());

    let total_replay_ticks: u32 = replay.rounds.iter().map(|r| r.len() as u32).sum();
    let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);
    let shadow = tango_pvp::shadow::Shadow::new_for_replay(remote_rom, &replay, remote_hooks).unwrap();
    let shadow = Arc::new(Mutex::new(shadow));
    let stepper_state = tango_pvp::stepper::State::new(
        match_type,
        replay.local_player_index,
        replay.rounds.clone(),
        0,
        replay.rng_seed,
        replay.is_offerer,
        total_replay_ticks,
        shadow.clone(),
        Box::new(|| {}),
    );
    let mut traps = local_hooks.common_traps();
    traps.extend(local_hooks.stepper_traps(stepper_state.clone()));
    core.set_traps(traps);

    const MAX_TICK: u32 = 700;
    let mut last: i64 = -1;
    loop {
        let (abs_tick, round_idx, ended) = {
            let inner = stepper_state.lock_inner();
            (inner.absolute_tick(), inner.current_round_index(), inner.is_round_ended())
        };
        if round_idx > 0 || ended || abs_tick > MAX_TICK {
            break;
        }
        if (abs_tick as i64) > last {
            let subscene = core.as_mut().raw_read_8(0x020364c0, -1);
            let subphase = core.as_mut().raw_read_8(0x020364c1, -1);
            let combat_hp = core.as_mut().raw_read_8(0x02034a12, -1);
            if (255..480).contains(&abs_tick) && abs_tick % 2 == 0 {
                eprintln!("tick {abs_tick:3}  subscene={subscene} subphase={subphase} combat_hp={combat_hp}");
            }
            last = abs_tick as i64;
        }
        core.as_mut().run_frame();
    }
    eprintln!("ran to tick {last}");
}
