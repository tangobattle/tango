//! Opening a recording.
//!
//! A replay is re-simulated, not decoded: the file carries the seed,
//! both SRAMs and the input pairs, and playback boots the same pair of
//! cores the match ran on and feeds them the recorded stream. So
//! watching one needs exactly what playing one needed — both sides'
//! ROMs, with both sides' patches applied — and the honest failure when
//! a ROM is missing is to say which one.
//!
//! [`tango_session::replay`] hands back three loops (drive, seek chase,
//! prefetch). A desktop gives each a thread; `Workers::into_driver`
//! folds them into the single [`Drive`](tango_session::Drive) this host
//! pumps, slicing the seek chase and the prefetch pass so neither can
//! monopolise the frame.

use num_traits::ToPrimitive;
use std::sync::Arc;

use tango_library::game;
use tango_library::rom::GameRef;

/// Both sides' games and the exact ROMs they were played on.
///
/// Absolute player order throughout — the file's one
/// perspective-dependent byte is `local_player_index`, and it is
/// deliberately not consulted here. Shared with the exporter, which
/// re-simulates the same match through a different pipeline and so
/// needs the identical pair.
pub fn resolve(replay: &tango_replay::Replay) -> Result<([GameRef; 2], [Arc<Vec<u8>>; 2]), String> {
    replay.metadata.require_native().map_err(|error| error.to_string())?;
    let sides = [replay.metadata.p1_side.as_ref(), replay.metadata.p2_side.as_ref()];
    let mut games: Vec<GameRef> = Vec::new();
    let mut roms: Vec<Arc<Vec<u8>>> = Vec::new();
    for (index, side) in sides.iter().enumerate() {
        let info = side
            .and_then(|s| s.game_info.as_ref())
            .ok_or_else(|| format!("the recording doesn't say what player {} was playing", index + 1))?;
        // Also rejects a replay whose family has bumped its replay
        // version since the recording — a different ROM isn't the only
        // thing that re-simulates to a different match; changed engine
        // support does too.
        let game = game::find_for_replay_side(info).map_err(|e| e.to_string())?;
        // The patch has to be the one that was played, not the newest:
        // a patch changes the ROM, and a different ROM re-simulates to
        // a different match.
        let patch = info
            .patch
            .as_ref()
            .map(|p| {
                p.version
                    .parse::<semver::Version>()
                    .map(|version| (p.name.clone(), version))
                    .map_err(|_| format!("{} has an unreadable version ({})", p.name, p.version))
            })
            .transpose()?;
        let rom =
            crate::library::patched_rom(game, patch.as_ref()).map_err(|e| format!("player {}: {e}", index + 1))?;
        games.push(game);
        roms.push(Arc::new(rom));
    }
    Ok(([games[0], games[1]], [roms[0].clone(), roms[1].clone()]))
}

/// Boot the recording at `path` and hand it to the pump.
pub async fn open(path: std::path::PathBuf) -> Result<(), String> {
    let replay = Arc::new(crate::library::read_replay(&path)?);
    let (games, roms) = resolve(&replay)?;

    let sink = crate::audio::sink().await;
    let local_player = replay.local_player_index as usize;
    let local = *games
        .get(local_player)
        .ok_or_else(|| "bad local player index".to_string())?;
    let peer = games[1 - local_player];
    let (session, workers, stream) =
        tango_session::replay::ReplaySession::new(tango_session::replay::ReplaySessionArgs {
            backend: tango_session::SessionBackend::Static(local.pvp),
            match_type: tango_library::game::replay_match_type(&replay.metadata, local).map_err(|e| e.to_string())?,
            peer_rom: Some(tango_match::PeerRom {
                code: *peer.rom_code,
                revision: peer.revision,
            }),
            roms,
            replay,
            expected_fps: local.pvp.tps().to_f32().unwrap(),
            sample_rate: crate::audio::sample_rate(),
            // Keep one screen and omit statistics on the small browser canvas.
            show_pip: false,
            stats_job: None,
            round_boundaries: vec![],
            record_factory: None,
        })
        .map_err(|e| e.to_string())?;

    // Priming happens on the first ticks, so let the screen change
    // before the main thread goes away for a moment.
    tango_session::platform::sleep(std::time::Duration::from_millis(32)).await;
    crate::engine::start_replay(session, workers.into_driver(), stream, sink);
    Ok(())
}
