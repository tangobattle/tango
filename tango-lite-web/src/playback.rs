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

/// Boot the recording at `path` and hand it to the pump.
pub async fn open(host: &crate::host::Host, path: std::path::PathBuf) -> Result<(), String> {
    let (replay, resolved) = crate::library::open_replay(&host.library, &path)?;

    let sink = host.engine.audio_sink().await;
    let (session, workers, stream) = tango_session::replay::ReplaySession::new(
        resolved.games,
        resolved.roms,
        Arc::new(replay),
        crate::audio::sample_rate(),
        // No picture-in-picture: the inset is a second screen's worth
        // of pixels on a display that hasn't room for the first.
        false,
        // No stats prefetch — there is no results screen here to feed,
        // and the pass is a whole second simulation of the match. That
        // also means no round boundaries: this frontend caches no
        // analyses, so there is nothing to hand in and no pass to
        // discover them.
        false,
        vec![],
    )
    .map_err(|e| e.to_string())?;

    // Priming happens on the first ticks, so let the screen change
    // before the main thread goes away for a moment.
    tango_platform::sleep(std::time::Duration::from_millis(32)).await;
    crate::engine::start_replay(&host.engine, session, workers.into_driver(), stream, sink);
    Ok(())
}
