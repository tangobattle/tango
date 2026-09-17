//! The match-stats sidecar cache: where a replay's cooked
//! [`tango_match::analysis::MatchStats`] are stored through [`crate::Storage`].
//! Hosts persist live-match completion and replay analysis results here;
//! readers can inspect stats without re-simulating the recording.

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
    storage: &dyn crate::Storage,
    cache_path: &std::path::Path,
    replays_path: &std::path::Path,
    replay_path: &std::path::Path,
) -> Option<tango_match::analysis::MatchStats> {
    let f = storage.open(&stats_path(cache_path, replays_path, replay_path)).ok()?;
    tango_match::analysis::MatchStats::read(std::io::BufReader::new(f)).ok()
}

/// Write `stats` to a replay's cache slot, creating the mirrored
/// directory as needed.
pub fn write_match_stats(
    storage: &dyn crate::Storage,
    stats_file: &std::path::Path,
    stats: &tango_match::analysis::MatchStats,
) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    stats.write(&mut bytes)?;
    crate::storage::write_atomic(storage, stats_file, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_support::Memory, Storage};
    use std::path::Path;
    #[test]
    fn stats_roundtrip_and_invalid_cache_use_only_the_storage_adapter() {
        let store = Memory::default();
        let (cache, root, replay) = (
            Path::new("/cache"),
            Path::new("/replays"),
            Path::new("/replays/nested/match.tangoreplay"),
        );
        let path = stats_path(cache, root, replay);
        assert!(load_match_stats(&store, cache, root, replay).is_none());
        write_match_stats(&store, &path, &tango_match::analysis::MatchStats::default()).unwrap();
        assert!(load_match_stats(&store, cache, root, replay).is_some());
        assert_eq!(store.0.lock().unwrap().len(), 1, "the atomic write leaves no temporary");
        store.write(&path, b"invalid cache").unwrap();
        assert!(load_match_stats(&store, cache, root, replay).is_none());
    }
}
