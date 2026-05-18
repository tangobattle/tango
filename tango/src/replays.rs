use crate::scanner;

pub struct ScannedReplay {
    pub path: std::path::PathBuf,
    pub metadata: tango_pvp::replay::Metadata,
    /// Heavy stats — tick count, round count, completion flag.
    /// Filled in lazily by a background worker. Completion can't
    /// be cheaply peeked from the last byte either: a truncated
    /// recording whose tail happens to be `0x00` (legal as the
    /// low byte of joyflags inside a 2-byte tag) would look
    /// complete, so we always walk the input stream to be sure.
    pub stats: Option<ReplayStats>,
}

/// Output of [`compute_stats`]. Cheap to copy.
#[derive(Clone, Copy, Debug)]
pub struct ReplayStats {
    /// Sum of `rounds[i].len()` from the decoded replay — one
    /// tick per recorded input pair. Convert at 60 FPS for
    /// wall-clock duration.
    pub tick_count: u32,
    /// Number of rounds the recorded match got through. 2-3 for
    /// a finished best-of-3; whatever made it to disk for
    /// incompletes.
    pub round_count: u32,
    /// Whether the recorded stream ended with `END_OF_REPLAY`.
    pub is_complete: bool,
}

pub type Scanner = scanner::Scanner<Vec<ScannedReplay>>;

/// Walks `path` and reads the metadata header from each file,
/// skipping anything that doesn't parse. The heavier per-replay
/// stats (length, round count, completion) are intentionally NOT
/// computed here — see [`compute_stats`] for the lazy follow-up.
/// Sorts results newest-first, with ties broken by link code.
pub fn scan_replays(path: &std::path::Path) -> Vec<ScannedReplay> {
    let mut out = Vec::new();
    if std::fs::metadata(path).is_err() {
        return out;
    }
    for entry in walkdir::WalkDir::new(path) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!("replay scan: {e:?}");
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let mut f = match std::fs::File::open(p) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("{}: {e}", p.display());
                continue;
            }
        };
        let metadata = match tango_pvp::replay::read_metadata(&mut f) {
            Ok(m) => m,
            Err(_) => continue,
        };
        out.push(ScannedReplay {
            path: p.to_path_buf(),
            metadata,
            stats: None,
        });
    }
    out.sort_by_key(|r| {
        (
            std::cmp::Reverse(r.metadata.ts),
            r.metadata.link_code.clone(),
        )
    });
    out
}

/// Heavy stats computation for a single replay — full decode
/// (metadata, both WRAM zstd frames, every input tick). Spawn
/// this on a worker thread, never from the UI path.
pub fn compute_stats(path: &std::path::Path) -> std::io::Result<ReplayStats> {
    let f = std::fs::File::open(path)?;
    let replay = tango_pvp::replay::Replay::decode(f)?;
    Ok(ReplayStats {
        tick_count: replay.rounds.iter().map(|r| r.len() as u32).sum(),
        round_count: replay.rounds.len() as u32,
        is_complete: replay.is_complete,
    })
}

/// Pretty path relative to the replays root.
pub fn format_rel_path(replays_path: &std::path::Path, path: &std::path::Path) -> String {
    let s = path.strip_prefix(replays_path).unwrap_or(path).to_string_lossy();
    if s.is_empty() {
        "/".to_string()
    } else {
        format!("/{s}/")
    }
}
