use crate::library::scanner;

pub struct ScannedReplay {
    pub path: std::path::PathBuf,
    /// The recorder's player slot — picks the "you" side out of the
    /// metadata's player-ordered sides.
    pub local_player_index: u8,
    pub metadata: tango_replay::Metadata,
}

impl ScannedReplay {
    pub fn local_side(&self) -> Option<&tango_replay::metadata::Side> {
        self.metadata.side(self.local_player_index)
    }

    pub fn remote_side(&self) -> Option<&tango_replay::metadata::Side> {
        self.metadata.side(1 - self.local_player_index)
    }
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
fn local_game_registered(side: Option<&tango_replay::metadata::Side>) -> bool {
    match side.and_then(|s| s.game_info.as_ref()) {
        None => true,
        Some(gi) => u8::try_from(gi.rom_variant)
            .ok()
            .and_then(|variant| crate::library::game::find_by_family_and_variant(&gi.rom_family, variant))
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
        let (local_player_index, metadata) = match tango_replay::read_metadata(&mut f) {
            Ok((_version, lpi, m)) => (lpi, m),
            Err(_) => continue,
        };
        // Hide replays whose game isn't registered (its
        // `gamesupport-<game>` feature is disabled / its crate isn't
        // compiled in) — there's no way to view or export them.
        if !local_game_registered(metadata.side(local_player_index)) {
            continue;
        }
        out.push(ScannedReplay {
            path: p.to_path_buf(),
            local_player_index,
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
    let replay = tango_replay::Replay::decode(f)?;
    Ok(ReplayStats {
        tick_count: replay.inputs.len() as u32,
        round_count: replay.round_starts.len() as u32,
        is_complete: replay.is_complete,
    })
}

// The stats-sidecar cache format + paths live in the session crate
// (the live match and the playback prefetcher both write it there);
// re-exported so app callers keep one replays-module surface. Written
// at match teardown for live matches and by
// [`compute_and_cache_match_stats`] for everything else.
pub use tango_session::stats_cache::{load_match_stats, stats_path, write_match_stats};

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
    on_progress: &mut dyn FnMut(u32, u32, &tango_match::analysis::StatsBuilder),
    // Flipping this aborts the simulation mid-pass with a "cancelled"
    // error and nothing cached — used when a playback session's
    // prefetcher takes over the same analysis.
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<tango_match::analysis::MatchStats> {
    let f = std::fs::File::open(&path)?;
    let replay = tango_replay::Replay::decode(f)?;

    let resolve =
        |side: Option<&tango_replay::metadata::Side>| -> anyhow::Result<(crate::library::rom::GameRef, Vec<u8>)> {
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
                crate::library::patch::apply_patch_from_disk(&rom, entry, &patches_path, &patch_info.name, &v)?
            } else {
                rom
            };
            Ok((entry, rom))
        };
    let (p1_game, p1_rom) = resolve(replay.metadata.side(0))?;
    let (p2_game, p2_rom) = resolve(replay.metadata.side(1))?;

    let stats = analyze_replay(
        &replay,
        [p1_game, p2_game],
        [p1_rom, p2_rom],
        on_progress,
        cancel,
    )?;
    write_match_stats(&stats_path(&cache_path, &replays_path, &path), &stats)?;
    Ok(stats)
}

/// [`compute_and_cache_match_stats`]'s SIO-engine arm: linearly
/// re-simulate through [`tango_match::analysis::analyze`]. Everything in
/// the replay is already absolute player order; `local_player` only
/// picks whose chip semantics the stats speak.
fn analyze_replay(
    replay: &tango_replay::Replay,
    games: [crate::library::rom::GameRef; 2],
    roms: [Vec<u8>; 2],
    on_progress: &mut dyn FnMut(u32, u32, &tango_match::analysis::StatsBuilder),
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<tango_match::analysis::MatchStats> {
    let local_player = replay.local_player_index as usize;
    let inputs: Vec<[u32; 2]> = replay.inputs.iter().map(|&[p1, p2]| [p1 as u32, p2 as u32]).collect();
    tango_match::analysis::analyze(
        tango_match::analysis::AnalyzeConfig {
            roms: roms.clone(),
            saves: replay.srams.clone(),
            support: [games[0].pvp, games[1].pvp],
            match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
            rng_seed: replay.rng_seed,
            rtc: replay.rtc_time(),
            local_player,
            inputs: &inputs,
            chip_semantics: games[local_player].pvp.chip_semantics(&roms[local_player]),
            counts_buster: games[local_player].pvp.counts_buster(&roms[local_player]),
        },
        on_progress,
        cancel,
    )
    .map_err(Into::into)
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
