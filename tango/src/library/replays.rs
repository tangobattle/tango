//! Replays: the library crate's index, plus the re-simulation that
//! turns one into match stats.
//!
//! The index half ([`tango_library::replays`], re-exported below) only
//! reads headers and is frontend-agnostic. The half in this file boots
//! an emulator core per side and replays every input tick through the
//! analysis engine, so it needs the ROM library, patch application, and
//! `tango_match` — none of which belong in the library crate.

pub use tango_library::replays::*;

// The stats-sidecar cache format + paths live in the session crate
// (the live match and the playback prefetcher both write it there);
// re-exported so app callers keep one replays-module surface. Written
// at match teardown for live matches and by
// [`compute_and_cache_match_stats`] for everything else.
pub use tango_session::stats::{load_match_stats, stats_path, write_match_stats};

use crate::library::rom::GameRef;

/// Re-simulate a replay to produce its match stats and write the sidecar.
/// A full replay simulation — seconds of CPU; spawn on a blocking worker.
/// Resolves both sides' ROMs (with recorded patches applied) the same way
/// playback does, so it fails cleanly when a ROM or patch isn't installed.
/// `on_progress` is the analysis's per-tick reporter: `(ticks done,
/// ticks total)` plus the in-flight builder for live partial previews.
pub fn compute_and_cache_match_stats(
    scanners: crate::app::Scanners,
    patches_path: std::path::PathBuf,
    cache_path: std::path::PathBuf,
    replays_path: std::path::PathBuf,
    path: std::path::PathBuf,
    on_progress: &mut dyn FnMut(u32, u32, &tango_backend_mgba::analysis::StatsBuilder),
    // Flipping this aborts the simulation mid-pass with a "cancelled"
    // error and nothing cached — used when a playback session's
    // prefetcher takes over the same analysis.
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<tango_backend_mgba::analysis::MatchStats> {
    let storage = crate::library::storage();
    let replay = tango_replay::Replay::decode(storage.open(&path)?)?;

    let resolve = |side: Option<&tango_replay::metadata::Side>| -> anyhow::Result<(GameRef, Vec<u8>)> {
        let gi = side
            .and_then(|s| s.game_info.as_ref())
            .ok_or_else(|| anyhow::anyhow!("replay side has no game info"))?;
        let variant =
            u8::try_from(gi.rom_variant).map_err(|_| anyhow::anyhow!("variant {} out of range", gi.rom_variant))?;
        let entry = crate::library::game::find_by_family_and_variant(&gi.rom_family, variant)
            .ok_or_else(|| anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, gi.rom_variant))?;
        let rom = scanners
            .roms
            .read()
            .get(&entry)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("rom for {}/{} not scanned", gi.rom_family, gi.rom_variant))?;
        let rom = if let Some(patch_info) = gi.patch.as_ref() {
            let v = semver::Version::parse(&patch_info.version)?;
            crate::library::patch::apply_patch(storage, &rom, entry, &patches_path, &patch_info.name, &v)?
        } else {
            rom
        };
        Ok((entry, rom))
    };
    let (p1_game, p1_rom) = resolve(replay.metadata.side(0))?;
    let (p2_game, p2_rom) = resolve(replay.metadata.side(1))?;

    let stats = analyze_replay(&replay, [p1_game, p2_game], [p1_rom, p2_rom], on_progress, cancel)?;
    write_match_stats(&stats_path(&cache_path, &replays_path, &path), &stats)?;
    Ok(stats)
}

/// [`compute_and_cache_match_stats`]'s SIO-engine arm: linearly
/// re-simulate through [`tango_backend_mgba::analysis::analyze`]. Everything in
/// the replay is already absolute player order; `local_player` only
/// picks whose cart's chip decode the stats speak.
fn analyze_replay(
    replay: &tango_replay::Replay,
    games: [GameRef; 2],
    roms: [Vec<u8>; 2],
    on_progress: &mut dyn FnMut(u32, u32, &tango_backend_mgba::analysis::StatsBuilder),
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<tango_backend_mgba::analysis::MatchStats> {
    let local_player = replay.local_player_index as usize;
    let inputs: Vec<[u32; 2]> = replay.inputs.iter().map(|&row| row.map(|i| i.keys as u32)).collect();
    // Offline re-analysis re-simulates on the mgba engine directly, so
    // it needs both seats' support rather than a session.
    let seat = |g: &'static tango_gamesupport::Game| {
        tango_library::game::mgba_support(g.rom_code, g.revision)
            .ok_or_else(|| anyhow::anyhow!("replay analysis supports mgba games only"))
    };
    let support: [&dyn tango_backend_mgba::GameSupport; 2] = [seat(games[0])?, seat(games[1])?];
    tango_backend_mgba::analysis::analyze(
        tango_backend_mgba::analysis::AnalyzeConfig {
            roms: roms.clone(),
            saves: replay.srams.clone(),
            support,
            match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            rng_seed: replay.rng_seed,
            rtc: replay.rtc_time(),
            local_player,
            inputs: &inputs,
            usage: support[local_player].usage_fold(&roms[local_player]),
        },
        on_progress,
        cancel,
    )
    .map_err(Into::into)
}
