//! The replay index: what replays exist and what their headers say.
//!
//! Re-simulating a replay to produce match stats is deliberately *not*
//! here — that needs an emulator core and the analysis engine, which is
//! the session layer's business. This module only reads headers.

use crate::scanner;
use crate::storage::{Listing, Storage};

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
    /// One tick per recorded input pair. Convert at 60 FPS for
    /// wall-clock duration.
    pub tick_count: u32,
    /// Number of rounds the recorded match got through — 2-3 for a
    /// finished best-of-3, whatever made it to disk for incompletes.
    ///
    /// `None` until somebody has re-simulated the recording: a replay is
    /// inputs, and only the games' telemetry knows what those inputs
    /// added up to. [`compute_stats`] never fills this in — a host that
    /// caches match analyses (tango does, as `.stats` sidecars) can,
    /// from the analysis it already has.
    pub round_count: Option<u32>,
    /// Whether the recorded stream ended with `END_OF_REPLAY`.
    pub is_complete: bool,
}

pub type Scanner = scanner::Scanner<Vec<ScannedReplay>>;

/// Whether the replay's local-side game resolves for re-simulation
/// ([`crate::game::find_for_replay_side`]). A replay with no recorded
/// local game info can't be filtered on, so it's kept; one that names a
/// game we don't have compiled in — or whose family has bumped its
/// replay version since it was recorded — is hidden.
fn local_game_playable(side: Option<&tango_replay::metadata::Side>) -> bool {
    match side.and_then(|s| s.game_info.as_ref()) {
        None => true,
        Some(gi) => crate::game::find_for_replay_side(gi).is_ok(),
    }
}

/// Reads the metadata header out of each file in `listing`, skipping
/// anything that doesn't parse. Goes through [`Storage::open`] rather
/// than reading whole files — a replay runs to megabytes and only its
/// header is wanted here. The heavier per-replay stats (length, round
/// count, completion) are intentionally NOT computed either; see
/// [`compute_stats`] for the lazy follow-up. Sorts results newest-first,
/// with ties broken by link code.
pub fn scan_replays(storage: &dyn Storage, listing: &Listing) -> Vec<ScannedReplay> {
    let mut out = Vec::new();
    for entry in listing.entries() {
        let mut f = match storage.open(&entry.path) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("{}: {e}", entry.path.display());
                continue;
            }
        };
        let (local_player_index, metadata) = match tango_replay::read_metadata(&mut f) {
            Ok((_version, lpi, m)) => (lpi, m),
            // Debug, not warn: a library carried across a schema bump is
            // full of recordings this build can't read, and that's not
            // news every launch. Logged at all because a silent skip is
            // what made a pathological one invisible.
            Err(e) => {
                log::debug!("replay scan: {}: {e}", entry.path.display());
                continue;
            }
        };
        // Hide replays whose game isn't registered (its
        // `gamesupport-<game>` feature is disabled / its crate isn't
        // compiled in) or whose family has bumped its replay version
        // past the recording's — there's no way to view or export them.
        if !local_game_playable(metadata.side(local_player_index)) {
            continue;
        }
        out.push(ScannedReplay {
            path: entry.path.clone(),
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
pub fn compute_stats(storage: &dyn Storage, path: &std::path::Path) -> std::io::Result<ReplayStats> {
    let replay = tango_replay::Replay::decode(storage.open(path)?)?;
    Ok(ReplayStats {
        tick_count: replay.inputs.len() as u32,
        // Not a header fact — see [`ReplayStats::round_count`].
        round_count: None,
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
