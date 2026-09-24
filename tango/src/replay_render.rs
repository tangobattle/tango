//! Native wiring for [`tango_replay_renderer`]: all this adds is the
//! file to write into, a thread to run on, and a stream of what the
//! render reports. The re-simulation, the clip/round selection and the
//! chapter bookkeeping live in the renderer crate; which encoder runs is
//! [`encoder_facade`]'s to decide, and on a desktop that means ffmpeg
//! subprocesses.
//!
//! ffmpeg is only an *encoder* there: each stream comes back as a
//! fragmented MP4 that carries nothing but itself, and the container the
//! render writes is assembled in Rust from all of them. That means the
//! bundled ffmpeg has to be built with the MP4 muxer —
//! `--enable-muxer=mp4` — and a render that finds one without it says
//! so before it starts.

pub use tango_replay_renderer::{container, Canceller, Clip, Error, Request};

/// Everything one render needs, owned so it can move onto the render
/// thread. The app prepares it: the resolved engine replay, the section
/// mask and localized chapter titles, and the clip (the whole recording,
/// for a whole-replay export).
pub struct Job {
    pub engine: tango_session::replay::EngineReplay,
    pub rounds_mask: Vec<bool>,
    pub round_titles: Vec<String>,
    pub clip: Clip,
    /// `None` for raw output (RGB24 + PCM, no upscale); otherwise a
    /// lossy render at that nearest-neighbor upscale.
    pub scale: Option<usize>,
    pub twosided: bool,
    pub swap_sides: bool,
}

/// Why a spawned render wrote no file.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// The render itself failed, or was cancelled through its
    /// [`Canceller`].
    #[error(transparent)]
    Render(#[from] Error),
    #[error("spawn replay-export thread: {0}")]
    Spawn(std::io::Error),
    /// The render thread went away without reporting (it panicked).
    #[error("export task ended without result")]
    NoResult,
}

/// What a running render reports.
pub enum Event {
    /// `(completed, total)` ticks rendered.
    Progress { completed: usize, total: usize },
    /// The render ended: the written file, or why it didn't get written
    /// (including a cancel through the [`Canceller`]). Always the last
    /// event.
    Finished(Result<std::path::PathBuf, RenderError>),
}

/// Start `job` rendering to `output_path` on a dedicated thread and
/// stream its progress, ending with [`Event::Finished`]. The caller keeps
/// a clone of `canceller` to stop it.
pub fn spawn(
    job: Job,
    output_path: std::path::PathBuf,
    canceller: Canceller,
) -> impl futures::Stream<Item = Event> + Send + 'static {
    use futures::StreamExt;

    let (progress_tx, progress_rx) = futures::channel::mpsc::unbounded::<(usize, usize)>();
    let done: std::sync::Arc<std::sync::Mutex<Option<Result<std::path::PathBuf, RenderError>>>> = Default::default();
    let done_thread = done.clone();
    // The export is fully synchronous (std::process ffmpeg subprocesses,
    // no async), so it lives entirely outside the iced/tokio worker pool
    // — no shared-runtime starvation regardless of how tight the export
    // inner loop runs.
    let spawned = std::thread::Builder::new()
        .name("replay-export".to_string())
        .spawn(move || {
            // Clone the sender into the callback. The original
            // `progress_tx` stays alive on the thread until *after*
            // `done_thread` is set; otherwise the channel closes the
            // moment the callback (and thus the moved sender) is dropped,
            // and the stream below reads `done` while it's still unset.
            let cb_tx = progress_tx.clone();
            let cb = move |current: usize, total: usize| {
                let _ = cb_tx.unbounded_send((current, total));
            };
            let Job {
                engine,
                rounds_mask,
                round_titles,
                clip,
                scale,
                twosided,
                swap_sides,
            } = job;
            let request = Request {
                backend: engine.backend,
                config: engine.config,
                rounds_mask: &rounds_mask,
                round_titles: &round_titles,
                clip: &clip,
                scale,
                twosided,
                swap_sides,
            };
            let result = render(request, &output_path, &canceller, cb)
                .map(|()| output_path)
                .map_err(RenderError::Render);
            *done_thread.lock().unwrap() = Some(result);
            // Closing the channel here is what tells the stream to read
            // `done` — which is now safely set above.
            drop(progress_tx);
        });
    if let Err(e) = spawned {
        *done.lock().unwrap() = Some(Err(RenderError::Spawn(e)));
    }
    progress_rx
        .map(|(completed, total)| Event::Progress { completed, total })
        .chain(futures::stream::once(async move {
            Event::Finished(done.lock().unwrap().take().unwrap_or(Err(RenderError::NoResult)))
        }))
}

/// Render `request` to `output_path`, reporting `(completed, total)`
/// ticks through `progress_callback`. Fully synchronous; [`spawn`] runs
/// it on a thread of its own.
fn render(
    request: Request<'_>,
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
