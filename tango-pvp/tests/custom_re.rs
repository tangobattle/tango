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
//
// Watches a window of EWRAM bytes across the opening chip-select screen and
// prints, for every tick where any watched byte changes, the tick + all bytes.
// Lets us confirm/locate the "in custom screen" flag for a given game by eye.
fn watch_replay(replay_file: &str, watch: &[u32]) {
    watch_replay_with(replay_file, watch, |_, _| {});
}

// Like watch_replay, but calls `intervene(abs_tick, core)` once per tick before
// running the frame — lets us poke state mid-custom to validate a close
// mechanism (does writing X actually tear the screen down?).
fn watch_replay_with(replay_file: &str, watch: &[u32], mut intervene: impl FnMut(u32, &mut mgba::core::CoreMutRef)) {
    let roms = discover_roms();
    let replay_path = format!("{}/tests/golden/{}", env!("CARGO_MANIFEST_DIR"), replay_file);
    let replay = tango_pvp::replay::Replay::decode(&mut std::fs::File::open(&replay_path).unwrap()).unwrap();

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

    const MAX_TICK: u32 = 900;
    let mut last: i64 = -1;
    let mut prev: Vec<u8> = Vec::new();
    loop {
        let (abs_tick, round_idx, ended) = {
            let inner = stepper_state.lock_inner();
            (inner.absolute_tick(), inner.current_round_index(), inner.is_round_ended())
        };
        if round_idx > 0 || ended || abs_tick > MAX_TICK {
            break;
        }
        if (abs_tick as i64) > last {
            let now: Vec<u8> = watch.iter().map(|&a| core.as_mut().raw_read_8(a, -1)).collect();
            if now != prev {
                let cells: Vec<String> = watch
                    .iter()
                    .zip(&now)
                    .map(|(a, v)| format!("{a:08x}={v:3}"))
                    .collect();
                eprintln!("tick {abs_tick:4}  {}", cells.join("  "));
                prev = now;
            }
            last = abs_tick as i64;
        }
        {
            let mut c = core.as_mut();
            intervene(abs_tick, &mut c);
        }
        core.as_mut().run_frame();
    }
    eprintln!("ran to tick {last}");
}

// Detect the family/variant of every ROM in TANGO_TEST_ROMS_DIR, so we can tell
// whether two same-looking files map to different variants (different EWRAM).
#[test]
#[ignore]
fn re_detect() {
    for entry in std::fs::read_dir(roms_dir()).unwrap().flatten() {
        let p = entry.path();
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "gba" && ext != "srl" {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else { continue };
        let det = tango_gamedb::detect(&bytes).map(|g| g.family_and_variant());
        eprintln!("{:40} -> {:?}", p.file_name().unwrap().to_string_lossy(), det);
    }
}

// Throwaway RE harness, not a regression test: needs TANGO_TEST_ROMS_DIR.
#[test]
#[ignore]
fn re_dump() {
    watch_replay(
        "20260516051717-test123-bn6-vs-weenie-p1.tangoreplay",
        &[0x020364c0, 0x020364c1],
    );
}

#[test]
#[ignore]
fn re_dump_bn2() {
    // Agent claimed in_custom 0x0200eb10==2, close 0x0200eef0==8. Watch those
    // plus neighbours to confirm/relocate.
    watch_replay(
        "20260516050012-test123-bn2-vs-weenie-p1.tangoreplay",
        &[
            0x0200eae0, // is_linking (known good anchor)
            0x0200eb0f, 0x0200eb10, 0x0200eb11, 0x0200eb12, //
            0x0200eeef, 0x0200eef0, 0x0200eef1,
        ],
    );
}

// Validate BN2's close: once we're well into the custom screen (eb10==2), write
// the closing sub-state every tick like the timer would, and see whether the
// screen actually tears down (eb10 leaves 2 → combat) ahead of the recorded run.
#[test]
#[ignore]
fn re_close_bn2() {
    let mut started: Option<u32> = None;
    watch_replay_with(
        "20260516050012-test123-bn2-vs-weenie-p1.tangoreplay",
        &[0x0200eb10, 0x0200eef0],
        move |tick, core| {
            let scene = core.raw_read_8(0x0200eb10, -1);
            if scene == 2 {
                let open = *started.get_or_insert(tick);
                if tick >= open + 30 {
                    core.raw_write_8(0x0200eef0, -1, 8);
                }
            }
        },
    );
}

#[test]
#[ignore]
fn re_dump_bn3() {
    // Agent claimed in_custom 0x02006ca1==8, close 0x02006ca2==8 (struct base
    // 0x02006ca0). Watch the struct head.
    watch_replay(
        "20260516050145-test123-bn3-vs-weenie-p1.tangoreplay",
        &[0x02006ca1, 0x02006ca2, 0x0200f7f0, 0x0200f7f1],
    );
}

// Validate BN3's close. Like BN2, try the *direct* sub-state write (ca2=8)
// once we're into the custom screen (ca1==8), and watch whether it tears down
// (ca1 -> 12 combat) without injecting Start.
#[test]
#[ignore]
fn re_close_bn3() {
    let mut started: Option<u32> = None;
    // RE_MODE selects what to poke; RE_HOLD=1 holds it every tick, else a short
    // ~4-tick pulse (mimicking a button press) starting at open+30.
    let mode = std::env::var("RE_MODE").unwrap_or_default();
    let hold = std::env::var("RE_HOLD").as_deref() == Ok("1");
    watch_replay_with(
        "20260516050145-test123-bn3-vs-weenie-p1.tangoreplay",
        &[0x02006ca1, 0x02006ca2, 0x0200f7f0, 0x0200f7f1],
        move |tick, core| {
            let scene = core.raw_read_8(0x02006ca1, -1);
            if scene != 8 {
                started = None; // reset so we anchor on the *real* custom window, not the early blip
                return;
            }
            let open = *started.get_or_insert(tick);
            let active = if hold { tick >= open + 30 } else { (open + 30..open + 34).contains(&tick) };
            if active {
                match mode.as_str() {
                    "f7f0" => core.raw_write_8(0x0200f7f0, -1, 8),
                    "f7f1" => core.raw_write_8(0x0200f7f1, -1, 12),
                    "ca2" => core.raw_write_8(0x02006ca2, -1, 8),
                    // default: replicate the timer — pin substate to selecting.
                    // (Start can't be injected via a state poke; see re_close_bn3_start.)
                    _ => core.raw_write_8(0x02006ca2, -1, 4),
                }
            }
        },
    );
}
