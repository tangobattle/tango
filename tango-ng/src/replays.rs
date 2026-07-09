//! Replay discovery, copied from `tango/src/replays.rs` (minus the
//! `Scanner` alias and the lazy per-replay stats, which come over with
//! playback).

pub struct ScannedReplay {
    pub path: std::path::PathBuf,
    pub metadata: tango_pvp::replay::Metadata,
}

/// Whether the replay's local-side game is registered with the app. A
/// replay with no recorded local game info can't be filtered on, so it's
/// kept; one that names a game we don't have compiled in is hidden.
fn local_game_registered(metadata: &tango_pvp::replay::Metadata) -> bool {
    match metadata.local_side.as_ref().and_then(|s| s.game_info.as_ref()) {
        None => true,
        Some(gi) => u8::try_from(gi.rom_variant)
            .ok()
            .and_then(|variant| crate::game::find_by_family_and_variant(&gi.rom_family, variant))
            .is_some(),
    }
}

/// Walks `path` and reads the metadata header from each file,
/// skipping anything that doesn't parse. Sorts results newest-first,
/// with ties broken by link code.
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
        if !local_game_registered(&metadata) {
            continue;
        }
        out.push(ScannedReplay {
            path: p.to_path_buf(),
            metadata,
        });
    }
    out.sort_by_key(|r| (std::cmp::Reverse(r.metadata.ts), r.metadata.link_code.clone()));
    out
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

/// Replay timestamps are Unix milliseconds (the negotiated match clock).
pub fn format_ts(ms: u64, fmt: &str) -> String {
    std::time::UNIX_EPOCH
        .checked_add(std::time::Duration::from_millis(ms))
        .map(|t| chrono::DateTime::<chrono::Local>::from(t).format(fmt).to_string())
        .unwrap_or_else(|| "(?)".to_string())
}
