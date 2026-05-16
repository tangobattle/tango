//! Regression suite for the committed golden replays in `tests/golden/`.
//!
//! For each `*.tangoreplay`, decode it, look up its local + remote ROMs
//! by family/variant against a directory of `*.gba` files, and drive
//! `tango_pvp::replay::playback::run_prefetch` end-to-end. The replay
//! passes if the playback worker reaches the end without erroring.
//!
//! ROMs are sourced from `$TANGO_TEST_ROMS_DIR` (default: `repo/roms/`).
//! Copyrighted ROMs aren't committed, so any replay whose ROM isn't on
//! disk is reported as `skip` rather than counted as failure. With no
//! ROMs found at all the whole test short-circuits to a skip.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .to_path_buf()
}

fn roms_dir() -> PathBuf {
    std::env::var_os("TANGO_TEST_ROMS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("roms"))
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

/// (family, variant) -> verified ROM bytes. `tango_gamedb::detect`
/// validates the CRC32, so a hit guarantees the bytes match a known
/// gamedb entry.
fn discover_roms() -> HashMap<(&'static str, u8), Vec<u8>> {
    let mut out = HashMap::new();
    let dir = roms_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("gba") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else { continue };
        if let Some(g) = tango_gamedb::detect(&bytes) {
            out.insert(g.family_and_variant(), bytes);
        }
    }
    out
}

enum CaseResult {
    Ok,
    Skip(String),
    Fail(anyhow::Error),
}

fn resolve_side(
    side: Option<&tango_pvp::replay::metadata::Side>,
    label: &str,
) -> anyhow::Result<(
    &'static (dyn tango_gamedb::Game + Send + Sync),
    &'static (dyn tango_pvp::hooks::Hooks + Send + Sync),
)> {
    let info = side
        .and_then(|s| s.game_info.as_ref())
        .ok_or_else(|| anyhow::anyhow!("missing {} game info", label))?;
    let game = tango_gamedb::find_by_family_and_variant(&info.rom_family, info.rom_variant as u8)
        .ok_or_else(|| anyhow::anyhow!("unknown {} game {}/{}", label, info.rom_family, info.rom_variant))?;
    let hooks = tango_pvp::hooks::hooks_for_gamedb_entry(game)
        .ok_or_else(|| anyhow::anyhow!("no hooks registered for {} game", label))?;
    Ok((game, hooks))
}

fn run_one(path: &Path, roms: &HashMap<(&'static str, u8), Vec<u8>>) -> CaseResult {
    let replay = match std::fs::File::open(path)
        .map_err(anyhow::Error::from)
        .and_then(|mut f| Ok(tango_pvp::replay::Replay::decode(&mut f)?))
    {
        Ok(r) => r,
        Err(e) => return CaseResult::Fail(e),
    };

    let (local_game, local_hooks) = match resolve_side(replay.metadata.local_side.as_ref(), "local") {
        Ok(v) => v,
        Err(e) => return CaseResult::Fail(e),
    };
    let (remote_game, remote_hooks) = match resolve_side(replay.metadata.remote_side.as_ref(), "remote") {
        Ok(v) => v,
        Err(e) => return CaseResult::Fail(e),
    };

    let local_key = local_game.family_and_variant();
    let remote_key = remote_game.family_and_variant();
    let Some(local_rom) = roms.get(&local_key) else {
        return CaseResult::Skip(format!("missing local ROM {}/{}", local_key.0, local_key.1));
    };
    let Some(remote_rom) = roms.get(&remote_key) else {
        return CaseResult::Skip(format!("missing remote ROM {}/{}", remote_key.0, remote_key.1));
    };

    let store = tango_pvp::replay::playback::SnapshotStore::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tango_pvp::replay::playback::run_prefetch(
            local_rom,
            remote_rom,
            &replay,
            local_hooks,
            remote_hooks,
            store,
            cancel,
            progress,
        )
    }));

    match result {
        Ok(Ok(())) => CaseResult::Ok,
        Ok(Err(e)) => CaseResult::Fail(e),
        Err(panic) => {
            let msg = if let Some(s) = panic.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = panic.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            CaseResult::Fail(anyhow::anyhow!("panic during playback: {}", msg))
        }
    }
}

#[test]
fn golden_replays() {
    let roms = discover_roms();

    let golden = golden_dir();
    let mut replays: Vec<PathBuf> = std::fs::read_dir(&golden)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", golden.display(), e))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("tangoreplay"))
        .collect();
    replays.sort();

    if roms.is_empty() {
        eprintln!(
            "no ROMs in {} -- skipping {} replay(s). \
             Set TANGO_TEST_ROMS_DIR to a directory of *.gba files to run this suite.",
            roms_dir().display(),
            replays.len(),
        );
        return;
    }

    let mut passed: usize = 0;
    let mut skipped: Vec<(String, String)> = vec![];
    let mut failed: Vec<(String, anyhow::Error)> = vec![];

    for replay_path in &replays {
        let name = replay_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        match run_one(replay_path, &roms) {
            CaseResult::Ok => {
                eprintln!("ok   {}", name);
                passed += 1;
            }
            CaseResult::Skip(reason) => {
                eprintln!("skip {} ({})", name, reason);
                skipped.push((name, reason));
            }
            CaseResult::Fail(err) => {
                eprintln!("FAIL {}: {:#}", name, err);
                failed.push((name, err));
            }
        }
    }

    eprintln!(
        "\nsummary: {} ok, {} failed, {} skipped (of {} total)",
        passed,
        failed.len(),
        skipped.len(),
        replays.len(),
    );

    if !failed.is_empty() {
        let detail = failed
            .iter()
            .map(|(n, e)| format!("  {}: {:#}", n, e))
            .collect::<Vec<_>>()
            .join("\n");
        panic!("{} replay(s) failed:\n{}", failed.len(), detail);
    }
}
