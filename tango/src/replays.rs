use crate::scanner;

pub struct ScannedReplay {
    pub path: std::path::PathBuf,
    pub metadata: tango_pvp::replay::Metadata,
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
        // Hide replays whose game isn't registered (its
        // `gamesupport-<game>` feature is disabled / its crate isn't
        // compiled in) — there's no way to view or export them.
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

/// Where a replay's cached match stats live: `<name>.tangoreplay.stats`
/// next to the replay. Written at match teardown for live matches and by
/// [`compute_and_cache_match_stats`] for everything else.
pub fn stats_path(replay_path: &std::path::Path) -> std::path::PathBuf {
    let mut s = replay_path.as_os_str().to_owned();
    s.push(".stats");
    std::path::PathBuf::from(s)
}

/// The cached match stats for a replay, if a readable sidecar of the
/// current format version is on disk. Any failure (missing, malformed,
/// stale version) is just `None` — the caller recomputes.
pub fn load_match_stats(replay_path: &std::path::Path) -> Option<tango_pvp::analysis::MatchStats> {
    let f = std::fs::File::open(stats_path(replay_path)).ok()?;
    tango_pvp::analysis::MatchStats::read(std::io::BufReader::new(f)).ok()
}

/// Re-simulate a replay to produce its match stats and write the sidecar.
/// A full replay simulation — seconds of CPU; spawn on a blocking worker.
/// Resolves both sides' ROMs (with recorded patches applied) the same way
/// playback does, so it fails cleanly when a ROM or patch isn't installed.
pub fn compute_and_cache_match_stats(
    scanners: crate::app::Scanners,
    patches_path: std::path::PathBuf,
    path: std::path::PathBuf,
) -> anyhow::Result<tango_pvp::analysis::MatchStats> {
    let f = std::fs::File::open(&path)?;
    let replay = tango_pvp::replay::Replay::decode(f)?;

    let resolve = |side: Option<&tango_pvp::replay::metadata::Side>| -> anyhow::Result<(
        &'static (dyn tango_pvp::hooks::Hooks + Send + Sync),
        Vec<u8>,
    )> {
        let gi = side
            .and_then(|s| s.game_info.as_ref())
            .ok_or_else(|| anyhow::anyhow!("replay side has no game info"))?;
        let variant = u8::try_from(gi.rom_variant)
            .map_err(|_| anyhow::anyhow!("variant {} out of range", gi.rom_variant))?;
        let entry = crate::game::find_by_family_and_variant(&gi.rom_family, variant)
            .ok_or_else(|| anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, gi.rom_variant))?;
        let rom = scanners
            .roms
            .read()
            .get(&entry)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("rom for {}/{} not scanned", gi.rom_family, gi.rom_variant))?;
        let rom = if let Some(patch_info) = gi.patch.as_ref() {
            let v = semver::Version::parse(&patch_info.version)?;
            crate::patch::apply_patch_from_disk(&rom, entry, &patches_path, &patch_info.name, &v)?
        } else {
            rom
        };
        Ok((entry.hooks, rom))
    };
    let (local_hooks, local_rom) = resolve(replay.metadata.local_side.as_ref())?;
    let (remote_hooks, remote_rom) = resolve(replay.metadata.remote_side.as_ref())?;

    let stats = tango_pvp::analysis::analyze(&replay, &local_rom, &remote_rom, local_hooks, remote_hooks)?;
    let f = std::fs::File::create(stats_path(&path))?;
    stats.write(std::io::BufWriter::new(f))?;
    Ok(stats)
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
