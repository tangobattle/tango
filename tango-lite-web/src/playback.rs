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
    let sides = [replay.metadata.p1_side.as_ref(), replay.metadata.p2_side.as_ref()];
    let mut games: Vec<GameRef> = Vec::new();
    let mut roms: Vec<Arc<Vec<u8>>> = Vec::new();
    for (index, side) in sides.iter().enumerate() {
        let info = side
            .and_then(|s| s.game_info.as_ref())
            .ok_or_else(|| format!("the recording doesn't say what player {} was playing", index + 1))?;
        let variant = info.rom_variant as u8;
        let game = game::find_by_family_and_variant(&info.rom_family, variant)
            .ok_or_else(|| format!("unsupported game {} v{variant}", info.rom_family))?;
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
        let rom = crate::library::patched_rom(game, patch.as_ref())
            .map_err(|e| format!("player {}: {e}", index + 1))?;
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
    let (session, workers, stream) = tango_session::replay::ReplaySession::new(
        games,
        roms,
        replay,
        crate::audio::sample_rate(),
        // No picture-in-picture: the inset is a second screen's worth
        // of pixels on a display that hasn't room for the first.
        false,
        // No stats prefetch — there is no results screen here to feed,
        // and the pass is a whole second simulation of the match.
        None,
    )
    .map_err(|e| e.to_string())?;

    // Priming happens on the first ticks, so let the screen change
    // before the main thread goes away for a moment.
    tango_session::platform::sleep(std::time::Duration::from_millis(32)).await;
    crate::engine::start_replay(session, workers.into_driver(), stream, sink);
    Ok(())
}
