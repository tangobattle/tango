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

use tango_library::rom::GameRef;

/// Both sides' games and the exact ROMs they were played on.
///
/// Absolute player order throughout — the file's one
/// perspective-dependent byte is `local_player_index`, and it is
/// deliberately not consulted here. Shared with the exporter, which
/// re-simulates the same match through a different pipeline and so
/// needs the identical pair.
pub fn resolve(replay: &tango_replay::Replay) -> Result<([GameRef; 2], [Arc<Vec<u8>>; 2]), String> {
    let resolved = crate::library::with(|library| {
        tango_library::replays::resolve_roms(
            &library.files,
            &library.roms,
            &library.config.patches_path(),
            &replay.metadata,
        )
    })
    .ok_or_else(|| "library not open".to_owned())?
    .map_err(|e| e.to_string())?;
    Ok((resolved.games, resolved.roms.map(Arc::new)))
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
        // Both seats always share one engine, so either seat's rate is
        // the session's.
        games[0].pvp.tps().to_f32().unwrap(),
        crate::audio::sample_rate(),
        // No picture-in-picture: the inset is a second screen's worth
        // of pixels on a display that hasn't room for the first.
        false,
        // No stats prefetch — there is no results screen here to feed,
        // and the pass is a whole second simulation of the match. That
        // also means no round boundaries: this frontend caches no
        // analyses, so there is nothing to hand in and no pass to
        // discover them.
        None,
        vec![],
    )
    .map_err(|e| e.to_string())?;

    // Priming happens on the first ticks, so let the screen change
    // before the main thread goes away for a moment.
    tango_session::platform::sleep(std::time::Duration::from_millis(32)).await;
    crate::engine::start_replay(session, workers.into_driver(), stream, sink);
    Ok(())
}
