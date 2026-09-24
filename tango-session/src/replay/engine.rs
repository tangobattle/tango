//! A recording mapped onto its game's engine: the one place a
//! [`tango_replay::Replay`]'s fields become a
//! [`tango_match::ReplayConfig`], so playback, analysis and export all
//! re-simulate the identical match.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Why a recording can't be handed to its game's engine.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The perspective byte names neither seat.
    #[error("replay has bad local player index {0}")]
    BadLocalPlayerIndex(u8),
    #[error("replay has no inputs")]
    EmptyReplay,
}

/// A recording ready to open: the engine that runs it and the
/// configuration it opens with. The engine is the local seat's own
/// backend — which emulator that is stays the game's business.
pub struct EngineReplay {
    pub backend: &'static (dyn tango_match::Backend + Send + Sync),
    /// Collects no statistics and keeps the BGM on; a consumer that
    /// wants otherwise flips those two fields.
    pub config: tango_match::ReplayConfig,
}

impl EngineReplay {
    /// `games` and `roms` in absolute player order, as
    /// `tango_library::replays::resolve_roms` hands them out; the
    /// recording's local-player index only picks the perspective.
    pub fn new(
        games: [tango_gamesupport::GameRef; 2],
        roms: [Vec<u8>; 2],
        replay: &tango_replay::Replay,
    ) -> Result<Self, ConfigError> {
        let local_player = replay.local_player_index as usize;
        if local_player >= 2 {
            return Err(ConfigError::BadLocalPlayerIndex(replay.local_player_index));
        }
        if replay.inputs.is_empty() {
            return Err(ConfigError::EmptyReplay);
        }
        // The replay's input stream is already absolute pair order
        // (core 0 runs player 0's game) — just widen into the seam's
        // vocabulary, touches included (the DS games).
        let inputs = replay
            .inputs
            .iter()
            .map(|&row| {
                row.map(|input| tango_match::HostInput {
                    keys: input.keys as u32,
                    touch: input.touch.map(|(x, y)| (x as u16, y as u16)),
                })
            })
            .collect();
        Ok(Self {
            backend: games[local_player].pvp,
            config: tango_match::ReplayConfig {
                roms,
                saves: replay.srams.clone(),
                inputs: Arc::new(inputs),
                rng_seed: replay.rng_seed,
                rtc: replay.rtc_time(),
                match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
                local_player,
                peer_rom: tango_match::PeerRom {
                    code: *games[1 - local_player].rom_code,
                    revision: games[1 - local_player].revision,
                },
                want_stats: false,
                // Gameplay-neutral either way (see
                // `ReplayConfig::disable_bgm`); only a render has a
                // listener who might want it off.
                disable_bgm: false,
            },
        })
    }

    /// How many ticks the recording holds — never zero.
    pub fn total_ticks(&self) -> u32 {
        self.config.inputs.len() as u32
    }
}

/// Re-simulate a recording headlessly for its match statistics: open it
/// on the local game's engine and run the seam's linear analysis pass
/// ([`tango_match::ReplaySet::analyze`]) over it. Seconds of CPU; a host
/// with threads runs it on a worker. `on_progress` reports `(ticks done,
/// ticks total)` plus the in-flight builder for live partial previews;
/// flipping `cancel` aborts with nothing partial.
pub fn analyze(
    games: [tango_gamesupport::GameRef; 2],
    roms: [Vec<u8>; 2],
    replay: &tango_replay::Replay,
    on_progress: &mut dyn FnMut(u32, u32, &tango_match::analysis::StatsBuilder),
    cancel: &AtomicBool,
) -> Result<tango_match::analysis::MatchStats, crate::Error> {
    let mut engine = EngineReplay::new(games, roms, replay)?;
    engine.config.want_stats = true;
    let set = engine.backend.open_replay(engine.config)?;
    Ok(set.analyze(on_progress, cancel)?)
}
