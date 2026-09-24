//! Replays: the library crate's index, plus the desktop's cached
//! re-simulation that turns one into match stats.
//!
//! The index half ([`tango_library::replays`], re-exported below) only
//! reads headers. The analysis itself is the portable
//! [`tango_session::replay::analyze`]; this file resolves the ROMs from
//! the desktop's scanners and writes the result to the stats cache.

pub use tango_library::replays::*;

pub use tango_library::stats::stats_path;
pub fn load_match_stats(
    cache: &std::path::Path,
    root: &std::path::Path,
    replay: &std::path::Path,
) -> Option<tango_match::analysis::MatchStats> {
    tango_library::stats::load_match_stats(super::storage(), cache, root, replay)
}
pub fn write_match_stats(path: &std::path::Path, stats: &tango_match::analysis::MatchStats) -> std::io::Result<()> {
    tango_library::stats::write_match_stats(super::storage(), path, stats)
}

/// Re-simulate a replay to produce its match stats and write the sidecar.
/// A full replay simulation — seconds of CPU; spawn on a blocking worker.
/// Resolves both sides' ROMs (with recorded patches applied) the same way
/// playback does, so it fails cleanly when a ROM or patch isn't installed.
/// `on_progress` is the analysis's per-tick reporter: `(ticks done,
/// ticks total)` plus the in-flight builder for live partial previews.
pub fn compute_and_cache_match_stats(
    scanners: super::Catalog,
    patches_path: std::path::PathBuf,
    cache_path: std::path::PathBuf,
    replays_path: std::path::PathBuf,
    path: std::path::PathBuf,
    on_progress: &mut dyn FnMut(u32, u32, &tango_match::analysis::StatsBuilder),
    // Flipping this aborts the simulation mid-pass with a "cancelled"
    // error and nothing cached — used when a playback session's
    // prefetcher takes over the same analysis.
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<tango_match::analysis::MatchStats> {
    let (replay, resolved) = open(crate::library::storage(), &scanners.roms, &patches_path, &path)?;
    let stats = tango_session::replay::analyze(resolved.games, resolved.roms, &replay, on_progress, cancel)?;
    write_match_stats(&stats_path(&cache_path, &replays_path, &path), &stats)?;
    Ok(stats)
}
