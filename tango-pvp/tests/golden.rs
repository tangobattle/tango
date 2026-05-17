//! Regression suite for the committed golden replays in `tests/golden/`.
//!
//! For each `*.tangoreplay`, decode it, look up its local + remote ROMs by
//! family/variant against a directory of `*.gba` files, and drive the
//! replay end-to-end through a stepper + shadow pair. Compute a
//! determinism fingerprint along the way and compare against the
//! sidecar `*.tangoreplay.expected` checked in next to each replay.
//!
//! The fingerprint captures:
//!   - shadow_packets_sha256 -- SHA256 of every shadow-generated remote
//!     packet across all rounds. Catches any divergence in the shadow's
//!     per-tick behavior.
//!   - final_ram_sha256 -- SHA256 of the playback core's WRAM + IWRAM
//!     at the deterministic moment all replay inputs are consumed.
//!     The local core's RTC is pinned via `Core::set_rtc_fixed` to the
//!     replay's metadata timestamp; without that pin exe45 reads the
//!     host wallclock into WRAM and the hash drifts between runs.
//!   - per-round outcomes -- (index, tick, Win/Loss/Draw). Human-readable
//!     summary that makes failures interpretable without a hash diff.
//!
//! Set `TANGO_GOLDEN_BLESS=1` to overwrite the sidecars from the current
//! run instead of asserting against them. Use this after an intentional
//! determinism-affecting change.
//!
//! ROMs are sourced from `$TANGO_TEST_ROMS_DIR` (default: `repo/roms/`).
//! Copyrighted ROMs aren't committed, so any replay whose ROM isn't on
//! disk is reported as `skip` rather than counted as failure. With no
//! ROMs found at all the whole test short-circuits to a skip.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tango_pvp::stepper::BattleOutcome;

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

fn bless_mode() -> bool {
    matches!(std::env::var("TANGO_GOLDEN_BLESS").as_deref(), Ok("1"))
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoundFingerprint {
    index: u32,
    tick: u32,
    outcome: BattleOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    shadow_packets_sha256: [u8; 32],
    final_ram_sha256: [u8; 32],
    rounds: Vec<RoundFingerprint>,
}

fn outcome_str(o: BattleOutcome) -> &'static str {
    match o {
        BattleOutcome::Win => "Win",
        BattleOutcome::Loss => "Loss",
        BattleOutcome::Draw => "Draw",
    }
}

fn parse_outcome(s: &str) -> anyhow::Result<BattleOutcome> {
    Ok(match s {
        "Win" => BattleOutcome::Win,
        "Loss" => BattleOutcome::Loss,
        "Draw" => BattleOutcome::Draw,
        other => anyhow::bail!("unknown outcome {:?}", other),
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn hex_decode_32(s: &str) -> anyhow::Result<[u8; 32]> {
    if s.len() != 64 {
        anyhow::bail!("expected 64 hex chars, got {}", s.len());
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)?;
    }
    Ok(out)
}

impl Fingerprint {
    fn to_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("shadow_packets_sha256 = {}\n", hex_encode(&self.shadow_packets_sha256)));
        s.push_str(&format!("final_ram_sha256 = {}\n", hex_encode(&self.final_ram_sha256)));
        for r in &self.rounds {
            s.push_str(&format!("round {} = {} @ {}\n", r.index, outcome_str(r.outcome), r.tick));
        }
        s
    }

    fn from_text(text: &str) -> anyhow::Result<Self> {
        let mut shadow_packets_sha256 = None;
        let mut final_ram_sha256 = None;
        let mut rounds = vec![];
        for (lineno, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let lineno = lineno + 1;
            if let Some(rest) = line.strip_prefix("shadow_packets_sha256 = ") {
                shadow_packets_sha256 = Some(hex_decode_32(rest)?);
            } else if let Some(rest) = line.strip_prefix("final_ram_sha256 = ") {
                final_ram_sha256 = Some(hex_decode_32(rest)?);
            } else if let Some(rest) = line.strip_prefix("round ") {
                // format: "round <idx> = <outcome> @ <tick>"
                let (idx_str, after_idx) = rest
                    .split_once(" = ")
                    .ok_or_else(|| anyhow::anyhow!("line {}: missing ' = '", lineno))?;
                let (outcome_str, tick_str) = after_idx
                    .split_once(" @ ")
                    .ok_or_else(|| anyhow::anyhow!("line {}: missing ' @ '", lineno))?;
                rounds.push(RoundFingerprint {
                    index: idx_str.parse()?,
                    outcome: parse_outcome(outcome_str)?,
                    tick: tick_str.parse()?,
                });
            } else {
                anyhow::bail!("line {}: unrecognized: {:?}", lineno, line);
            }
        }
        Ok(Fingerprint {
            shadow_packets_sha256: shadow_packets_sha256
                .ok_or_else(|| anyhow::anyhow!("missing shadow_packets_sha256"))?,
            final_ram_sha256: final_ram_sha256
                .ok_or_else(|| anyhow::anyhow!("missing final_ram_sha256"))?,
            rounds,
        })
    }
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

/// Custom driver that mirrors `run_prefetch` but threads a determinism
/// fingerprint through the playback loop. Watches `output_pairs()` to
/// hash every shadow-generated remote packet across round transitions,
/// and snapshots `round_result()` while the round is still in the Ended
/// phase (before `load_replay_round` clears it).
fn compute_fingerprint(
    local_rom: &[u8],
    remote_rom: &[u8],
    replay: &tango_pvp::replay::Replay,
    local_hooks: &'static (dyn tango_pvp::hooks::Hooks + Send + Sync),
    remote_hooks: &'static (dyn tango_pvp::hooks::Hooks + Send + Sync),
) -> anyhow::Result<Fingerprint> {
    let mut core = mgba::core::Core::new_gba("tango-test")?;
    core.enable_video_buffer();
    core.as_mut().load_rom(mgba::vfile::VFile::from_vec(local_rom.to_vec()))?;
    core.as_mut()
        .load_save(mgba::vfile::VFile::from_vec(replay.local_sram_dump()?))?;
    // Pin the cart RTC before reset so games that surface RTC into RAM
    // (e.g. exe45) produce a byte-stable fingerprint.
    let replay_time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(replay.metadata.ts);
    core.set_rtc_fixed(replay_time);
    core.as_mut().reset();
    local_hooks.patch(core.as_mut());

    let total_replay_ticks: u32 = replay.rounds.iter().map(|r| r.len() as u32).sum();
    let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);

    let shadow = tango_pvp::shadow::Shadow::new_for_replay(remote_rom, &replay, remote_hooks)?;
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

    // Determinism collection. Two independent things per loop iter:
    //   1. Hash any newly-appended `output_pairs[i].remote.packet` bytes.
    //      output_pairs resets to [] across `load_replay_round`, so a
    //      len drop = round transition (also signalled by round_idx).
    //   2. Track the latest `round_result()` we've seen for the current
    //      round and commit it when round_idx advances (clean transition)
    //      or when the replay terminates (last round).
    //
    // Polling `is_round_ended()` directly is fragile: the per-game trap
    // order varies, and on multi-round replays the Ended->InProgress
    // round_result-clearing window may close before we next sample. The
    // round_idx-change signal is monotonic and unambiguous.
    let mut packet_hasher = Sha256::new();
    let mut hashed_count: usize = 0;
    let mut rounds: Vec<RoundFingerprint> = vec![];
    let mut current_round_result: Option<tango_pvp::stepper::RoundResult> = None;
    let mut last_seen_round_idx: u32 = 0;
    let mut frames_after_drain: u32 = 0;
    // Snapshot WRAM+IWRAM at the deterministic moment all inputs are
    // drained, not at loop exit. The exit point can fuzz by a frame
    // depending on per-game round_end_* timing; this point can't.
    let mut ram_snapshot: Option<(Vec<u8>, Vec<u8>)> = None;
    // 10s of game time should be enough for any end-of-round animation
    // to fire round_end_set_*. Past this we give up and record whatever
    // we have (which may be None for genuinely incomplete replays).
    const MAX_DRAIN_FRAMES: u32 = 600;

    loop {
        let (total_left, abs_tick, round_idx, ended, result) = {
            let inner = stepper_state.lock_inner();
            let pairs = inner.output_pairs();
            if pairs.len() < hashed_count {
                hashed_count = 0;
            }
            for p in &pairs[hashed_count..] {
                packet_hasher.update(&p.remote.packet);
            }
            hashed_count = pairs.len();
            (
                inner.total_input_pairs_left(),
                inner.absolute_tick(),
                inner.current_round_index(),
                inner.is_round_ended(),
                inner.round_result(),
            )
        };

        // Round transition: commit the just-finished round's latest seen result.
        if round_idx != last_seen_round_idx {
            if let Some(rr) = current_round_result.take() {
                rounds.push(RoundFingerprint {
                    index: last_seen_round_idx,
                    tick: rr.tick,
                    outcome: rr.outcome,
                });
            }
            last_seen_round_idx = round_idx;
        }

        // Track latest seen result for the current round. set_round_result
        // may overwrite earlier values within the same round; we record
        // whatever was last set before the round ends.
        if let Some(rr) = result {
            current_round_result = Some(rr);
        }

        if total_left == 0 && abs_tick > 0 {
            if ram_snapshot.is_none() {
                let s = core.as_mut().save_state()?;
                ram_snapshot = Some((s.wram().to_vec(), s.iwram().to_vec()));
            }
            // Last round's inputs are drained. Keep running so the game
            // can fire round_end_set_* before we exit; stop as soon as
            // we have both `ended` and a result, or after the budget.
            let have_result = current_round_result.is_some();
            if (ended && have_result) || frames_after_drain >= MAX_DRAIN_FRAMES {
                if let Some(rr) = current_round_result.take() {
                    rounds.push(RoundFingerprint {
                        index: last_seen_round_idx,
                        tick: rr.tick,
                        outcome: rr.outcome,
                    });
                }
                break;
            }
            frames_after_drain += 1;
        }

        core.as_mut().run_frame();
    }

    let (wram, iwram) = ram_snapshot
        .ok_or_else(|| anyhow::anyhow!("replay exited before consuming any inputs"))?;
    let mut ram_hasher = Sha256::new();
    ram_hasher.update(&wram);
    ram_hasher.update(&iwram);

    let mut shadow_packets_sha256 = [0u8; 32];
    shadow_packets_sha256.copy_from_slice(&packet_hasher.finalize());
    let mut final_ram_sha256 = [0u8; 32];
    final_ram_sha256.copy_from_slice(&ram_hasher.finalize());

    Ok(Fingerprint {
        shadow_packets_sha256,
        final_ram_sha256,
        rounds,
    })
}

enum CaseResult {
    Ok,
    Skip(String),
    Fail(anyhow::Error),
}

fn run_one(path: &Path, roms: &HashMap<(&'static str, u8), Vec<u8>>, bless: bool) -> CaseResult {
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

    let computed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compute_fingerprint(local_rom, remote_rom, &replay, local_hooks, remote_hooks)
    }));
    let computed = match computed {
        Ok(Ok(fp)) => fp,
        Ok(Err(e)) => return CaseResult::Fail(e),
        Err(panic) => {
            let msg = if let Some(s) = panic.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = panic.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            return CaseResult::Fail(anyhow::anyhow!("panic during playback: {}", msg));
        }
    };

    let expected_path = path.with_extension("tangoreplay.expected");

    if bless {
        if let Err(e) = std::fs::write(&expected_path, computed.to_text()) {
            return CaseResult::Fail(anyhow::anyhow!("write expected {}: {}", expected_path.display(), e));
        }
        return CaseResult::Ok;
    }

    let expected_text = match std::fs::read_to_string(&expected_path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return CaseResult::Fail(anyhow::anyhow!(
                "missing expected file {} (run with TANGO_GOLDEN_BLESS=1 to create)",
                expected_path.display()
            ));
        }
        Err(e) => return CaseResult::Fail(e.into()),
    };
    let expected = match Fingerprint::from_text(&expected_text) {
        Ok(fp) => fp,
        Err(e) => {
            return CaseResult::Fail(anyhow::anyhow!(
                "parse expected {}: {}",
                expected_path.display(),
                e
            ));
        }
    };

    if computed != expected {
        return CaseResult::Fail(diff_fingerprints(&expected, &computed));
    }
    CaseResult::Ok
}

fn diff_fingerprints(expected: &Fingerprint, actual: &Fingerprint) -> anyhow::Error {
    let mut diffs = vec![];
    if expected.shadow_packets_sha256 != actual.shadow_packets_sha256 {
        diffs.push(format!(
            "shadow_packets_sha256: expected {} got {}",
            hex_encode(&expected.shadow_packets_sha256),
            hex_encode(&actual.shadow_packets_sha256),
        ));
    }
    if expected.final_ram_sha256 != actual.final_ram_sha256 {
        diffs.push(format!(
            "final_ram_sha256: expected {} got {}",
            hex_encode(&expected.final_ram_sha256),
            hex_encode(&actual.final_ram_sha256),
        ));
    }
    if expected.rounds != actual.rounds {
        diffs.push(format!(
            "rounds:\n  expected: {:?}\n  actual:   {:?}",
            expected.rounds, actual.rounds,
        ));
    }
    anyhow::anyhow!("fingerprint mismatch (re-bless with TANGO_GOLDEN_BLESS=1 if intentional):\n  {}", diffs.join("\n  "))
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

    let bless = bless_mode();

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
        match run_one(replay_path, &roms, bless) {
            CaseResult::Ok => {
                eprintln!("{}   {}", if bless { "bless" } else { "ok " }, name);
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
        "\nsummary: {} {}, {} failed, {} skipped (of {} total)",
        passed,
        if bless { "blessed" } else { "ok" },
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
