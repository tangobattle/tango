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

/// Exact simulation inputs, shared by playback, analysis and video export.
/// Native game metadata is optional presentation, never a package requirement.
pub struct Resolved {
    pub replay: std::sync::Arc<tango_replay::Replay>,
    pub backend: tango_session::SessionBackend,
    pub roms: [Vec<u8>; 2],
    pub match_type: u8,
    pub peer_rom: Option<tango_match::PeerRom>,
    pub local_game: Option<GameRef>,
}

pub fn resolve(
    scanners: &super::Scanners,
    patches_path: &std::path::Path,
    mut replay: std::sync::Arc<tango_replay::Replay>,
) -> anyhow::Result<Resolved> {
    let local_player = replay.local_player_index as usize;
    anyhow::ensure!(local_player < 2, "replay has bad local player index {local_player}");
    if let Some(package) = crate::package::replay::resolve(scanners, &replay.metadata)? {
        let local_game = scanners
            .roms
            .read()
            .image(package.roms[local_player])
            .and_then(|image| image.native_game);
        if replay.metadata != package.metadata {
            std::sync::Arc::make_mut(&mut replay).metadata = package.metadata;
        }
        return Ok(Resolved {
            replay,
            backend: tango_session::SessionBackend::Owned(package.backend),
            roms: package.seats.each_ref().map(|seat| seat.rom().to_vec()),
            match_type: 0,
            peer_rom: None,
            local_game,
        });
    }
    let resolve_side = |side: Option<&tango_replay::metadata::Side>| -> anyhow::Result<(GameRef, Vec<u8>)> {
        let gi = side
            .and_then(|side| side.game_info.as_ref())
            .ok_or_else(|| anyhow::anyhow!("replay side has no game info"))?;
        let game = crate::library::game::find_for_replay_side(gi)?;
        let rom = scanners
            .roms
            .read()
            .get(&game)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("rom for {}/{} not scanned", gi.rom_family, gi.rom_variant))?;
        let rom = if let Some(patch) = &gi.patch {
            crate::library::patch::apply_patch(
                crate::library::storage(),
                &rom,
                game,
                patches_path,
                &patch.name,
                &patch.version.parse()?,
            )?
        } else {
            rom
        };
        Ok((game, rom))
    };
    let (p0_game, p0_rom) = resolve_side(replay.metadata.side(0))?;
    let (p1_game, p1_rom) = resolve_side(replay.metadata.side(1))?;
    let games = [p0_game, p1_game];
    let local = games[local_player];
    let peer = games[1 - local_player];
    Ok(Resolved {
        backend: tango_session::SessionBackend::Static(local.pvp),
        roms: [p0_rom, p1_rom],
        match_type: tango_library::game::replay_match_type(&replay.metadata, local)?,
        peer_rom: Some(tango_match::PeerRom {
            code: *peer.rom_code,
            revision: peer.revision,
        }),
        local_game: Some(local),
        replay,
    })
}

/// Re-simulate a replay to produce its match stats and write the sidecar.
/// A full replay simulation — seconds of CPU; spawn on a blocking worker.
/// Resolves both sides' ROMs (with recorded patches applied) the same way
/// playback does, so it fails cleanly when a ROM or patch isn't installed.
/// `on_progress` is the analysis's per-tick reporter: `(ticks done,
/// ticks total)` plus the in-flight builder for live partial previews.
pub fn compute_and_cache_match_stats(
    scanners: super::Scanners,
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
    let storage = crate::library::storage();
    let replay = std::sync::Arc::new(tango_replay::Replay::decode(storage.open(&path)?)?);
    let resolved = resolve(&scanners, &patches_path, replay)?;
    let stats = analyze_replay(resolved, on_progress, cancel)?;
    write_match_stats(&stats_path(&cache_path, &replays_path, &path), &stats)?;
    Ok(stats)
}

/// [`compute_and_cache_match_stats`]'s re-simulation: open the replay
/// on the local game's own engine ([`tango_match::Backend::open_replay`])
/// and run the seam's linear analysis pass
/// ([`tango_match::ReplaySet::analyze`]) over it — the engine
/// underneath never surfaces here. Everything in the replay is already
/// absolute player order; `local_player` only picks whose cart's chip
/// decode the stats speak.
fn analyze_replay(
    resolved: Resolved,
    on_progress: &mut dyn FnMut(u32, u32, &tango_match::analysis::StatsBuilder),
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<tango_match::analysis::MatchStats> {
    let replay = &resolved.replay;
    let local_player = replay.local_player_index as usize;
    if local_player >= 2 {
        anyhow::bail!("replay has bad local player index {local_player}");
    }
    // The replay's input stream is already absolute pair order — just
    // widen into the seam's vocabulary, touches included (the DS games).
    let inputs: std::sync::Arc<Vec<[tango_match::HostInput; 2]>> = std::sync::Arc::new(
        replay
            .inputs
            .iter()
            .map(|&row| {
                row.map(|input| tango_match::HostInput {
                    keys: input.keys as u32,
                    touch: input.touch.map(|(x, y)| (x as u16, y as u16)),
                })
            })
            .collect(),
    );
    let set = resolved.backend.open_replay(tango_match::ReplayConfig {
        record_factory: None,
        roms: resolved.roms,
        saves: replay.srams.clone(),
        inputs,
        rng_seed: replay.rng_seed,
        rtc: replay.rtc_time(),
        match_type: resolved.match_type,
        local_player,
        peer_rom: resolved.peer_rom,
        want_stats: true,
        // Nothing listens; gameplay-neutral either way (see
        // `ReplayConfig::disable_bgm`).
        disable_bgm: false,
    })?;
    set.analyze(on_progress, cancel).map_err(Into::into)
}
