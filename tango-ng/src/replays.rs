use crate::scanner;

pub struct ScannedReplay {
    pub path: std::path::PathBuf,
    pub metadata: tango_pvp::replay::Metadata,
}

pub type Scanner = scanner::Scanner<Vec<ScannedReplay>>;

/// Walks `path` and reads the metadata header from each file, skipping
/// anything that doesn't parse. Sorts results newest-first, with ties
/// broken by link code + round for natural multi-round grouping.
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

/// Pretty path relative to the replays root.
pub fn format_rel_path(replays_path: &std::path::Path, path: &std::path::Path) -> String {
    let s = path.strip_prefix(replays_path).unwrap_or(path).to_string_lossy();
    if s.is_empty() {
        "/".to_string()
    } else {
        format!("/{s}/")
    }
}
