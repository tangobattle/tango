//! Native wiring for [`tango_replay_renderer`]: all this adds is the
//! file to write into and a thread to run on. The re-simulation, the
//! clip/round selection and the chapter bookkeeping live in the renderer
//! crate; which encoder runs is [`encoder_facade`]'s to decide, and on a
//! desktop that means ffmpeg subprocesses.
//!
//! ffmpeg is only an *encoder* there: each stream comes back as a
//! fragmented MP4 that carries nothing but itself, and the container the
//! render writes is assembled in Rust from all of them. That means the
//! bundled ffmpeg has to be built with the MP4 muxer —
//! `--enable-muxer=mp4` — and a render that finds one without it says
//! so before it starts.

pub use tango_replay_renderer::{container, Canceller, Clip, Error, Request};

/// Render `request` to `output_path`, reporting `(completed, total)`
/// ticks through `progress_callback`. Fully
/// synchronous; the app runs it on a dedicated thread ([`crate::app`]'s
/// `spawn_replay_render`) while the replays tab's inline panel
/// ([`crate::tabs::replays`]) owns the [`Canceller`] and renders the
/// progress.
pub fn render(
    request: &Request<'_>,
    output_path: &std::path::Path,
    canceller: &Canceller,
    progress_callback: impl Fn(usize, usize),
) -> Result<(), Error> {
    // The finished file comes back at the end; the caller only wanted
    // it written.
    tango_replay_renderer::render(
        request,
        || {
            // Opened for reading as well: a faststart MP4 relocates its
            // index, which moves the media that follows it.
            Ok(std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(output_path)?)
        },
        canceller,
        progress_callback,
    )
    .map(|_file| ())
}
