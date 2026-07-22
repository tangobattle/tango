//! The match-stats sidecar cache: where a replay's cooked
//! [`tango_match::analysis::MatchStats`] live on disk. Written at match
//! teardown by the live PvP session and by the replay prefetcher's
//! analysis pass; read back by anything that wants a replay's stats
//! without re-simulating it.

/// Where a replay's cached match stats live: the replay's path relative
/// to the replays root, mirrored under `<data>/cache/replay-stats/` with
/// `.stats` appended — NOT a sidecar next to the replay, so the replays
/// folder stays clean and writing stats doesn't churn the rescan
/// fingerprint.
pub fn stats_path(
    cache_path: &std::path::Path,
    replays_path: &std::path::Path,
    replay_path: &std::path::Path,
) -> std::path::PathBuf {
    // A replay outside the replays root shouldn't happen (the scanner is
    // the only source of replay paths) — keyed degraded by file name.
    let rel = replay_path
        .strip_prefix(replays_path)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| {
            replay_path
                .file_name()
                .map(std::path::PathBuf::from)
                .unwrap_or_default()
        });
    let mut s = cache_path.join("replay-stats").join(rel).into_os_string();
    s.push(".stats");
    std::path::PathBuf::from(s)
}

/// The cached match stats for a replay, if a readable cache entry of the
/// current format version is on disk. Any failure (missing, malformed,
/// stale version) is just `None` — the caller recomputes.
pub fn load_match_stats(
    cache_path: &std::path::Path,
    replays_path: &std::path::Path,
    replay_path: &std::path::Path,
) -> Option<tango_match::analysis::MatchStats> {
    let f = std::fs::File::open(stats_path(cache_path, replays_path, replay_path)).ok()?;
    tango_match::analysis::MatchStats::read(std::io::BufReader::new(f)).ok()
}

/// Write `stats` to a replay's cache slot, creating the mirrored
/// directory as needed.
pub fn write_match_stats(stats_file: &std::path::Path, stats: &tango_match::analysis::MatchStats) -> anyhow::Result<()> {
    if let Some(parent) = stats_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let f = std::fs::File::create(stats_file)?;
    stats.write(std::io::BufWriter::new(f))
}
