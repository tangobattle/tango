//! Rendering a recording to a video file.
//!
//! [`tango_replay_renderer`] re-simulates the match and hands frames
//! and audio to [`encoder_facade`], which on this target is the browser's
//! own **WebCodecs** encoders feeding the facade's pure-Rust muxers. So
//! there is no ffmpeg here and nothing to install: the same pipeline
//! the desktop exports through runs in the tab.
//!
//! The one thing this host has to get right is *not blocking*. A
//! WebCodecs encoder returns its packets through callbacks, and those
//! callbacks are event-loop tasks — a loop that pumped the render
//! without yielding would starve the very encoders it is feeding and
//! deadlock against the renderer's own queue-depth guard. So the loop
//! below pumps a slice and then yields, every time, which is exactly
//! the shape the renderer's docs ask a browser for.
//!
//! The container is written to a `Cursor<Vec<u8>>` — the muxers seek
//! backwards to fill in what only the end of the stream knows — and the
//! finished bytes go straight to a download.

use std::cell::RefCell;
use std::io::Cursor;

use wasm_bindgen::prelude::*;

use tango_replay_renderer::{Canceller, Clip, Progress, Render, Request};

/// Ticks per pump slice. Small because each one is followed by a yield
/// and the encoders need those turns; large enough that the yield isn't
/// most of the wall clock.
const SLICE_TICKS: usize = 8;

/// Nearest-neighbour upscale. 2x is 480x320 — a sane size to send
/// someone, and a whole multiple so the pixels stay square and sharp.
/// `None` here would mean uncompressed RGB24 + PCM in Matroska, which is
/// a faithful archival file and a terrible thing to try to share from a
/// phone.
const SCALE: Option<usize> = Some(2);

/// How far along, for the replays screen.
#[derive(Clone, PartialEq, Debug)]
pub enum State {
    Rendering {
        done: usize,
        total: usize,
    },
    /// Frames are all in; the encoders are draining.
    Flushing,
    Failed(String),
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static CANCELLER: RefCell<Option<Canceller>> = const { RefCell::new(None) };
}

pub fn state() -> Option<State> {
    STATE.with(|s| s.borrow().clone())
}

pub fn is_running() -> bool {
    matches!(state(), Some(State::Rendering { .. } | State::Flushing))
}

/// Stop an export where it is. The render reports cancellation as an
/// error, which the loop below tells apart by asking the canceller.
pub fn cancel() {
    CANCELLER.with(|c| {
        if let Some(canceller) = c.borrow().as_ref() {
            canceller.kill();
        }
    });
}

/// Dismiss a failure notice.
pub fn clear() {
    if !is_running() {
        STATE.with(|s| *s.borrow_mut() = None);
    }
}

fn set(library: &crate::library::Handle, state: Option<State>) {
    STATE.with(|s| *s.borrow_mut() = state);
    crate::library::touch(library);
}

/// Render the recording at `path` and hand the result to a download.
pub async fn run(library: crate::library::Handle, path: std::path::PathBuf, name: String) {
    if is_running() {
        return;
    }
    set(&library, Some(State::Rendering { done: 0, total: 1 }));
    let canceller = Canceller::new();
    CANCELLER.with(|c| *c.borrow_mut() = Some(canceller.clone()));

    match render(&library, &path, &name, &canceller).await {
        Ok(()) => set(&library, None),
        Err(_) if canceller.is_cancelled() => set(&library, None),
        Err(e) => {
            log::warn!("export {}: {e}", path.display());
            set(&library, Some(State::Failed(e)));
        }
    }
    CANCELLER.with(|c| *c.borrow_mut() = None);
}

async fn render(
    library: &crate::library::Handle,
    path: &std::path::Path,
    name: &str,
    canceller: &Canceller,
) -> Result<(), String> {
    let (replay, resolved) = crate::library::open_replay(library, path)?;
    // The same boot the player uses, so the render reproduces the
    // recorded match rather than a similar one — with the games' own
    // audio, which is the point of a video.
    let engine =
        tango_session::replay::EngineReplay::new(resolved.games, resolved.roms, &replay).map_err(|e| e.to_string())?;
    let total_ticks = engine.total_ticks();

    // Whole replay, one chapter. The desktop's export form lets you pick
    // a clip and deselect rounds; on a phone the useful answer is "the
    // match" — and chaptering it would mean re-simulating the recording
    // first just to find out where the rounds are, since a recording
    // doesn't say.
    let clip = Clip {
        start: 0,
        end: total_ticks,
        snapshot: None,
        round_marks: vec![],
    };
    let rounds_mask = vec![true];
    let round_titles = vec!["Round 1".to_string()];

    let request = Request {
        backend: engine.backend,
        config: engine.config,
        rounds_mask: &rounds_mask,
        round_titles: &round_titles,
        clip: &clip,
        scale: SCALE,
        twosided: false,
        swap_sides: false,
    };

    // Booting the re-sim pair blocks; let the screen show the progress
    // bar before it does.
    yield_to_page().await;
    log::info!("export: booting the re-simulation for {total_ticks} ticks");
    let mut render = Render::new(request, || Ok(Cursor::new(Vec::new())), canceller).map_err(|e| e.to_string())?;

    loop {
        match render.pump(SLICE_TICKS).map_err(|e| e.to_string())? {
            Progress::Rendering { done, total } => set(&library, Some(State::Rendering { done, total })),
            Progress::Flushing => set(&library, Some(State::Flushing)),
            Progress::Done(writer) => {
                let extension = match tango_replay_renderer::container(SCALE.is_none()) {
                    encoder_facade::Container::Mp4 => "mp4",
                    encoder_facade::Container::Matroska => "mkv",
                };
                let bytes = writer.into_inner();
                log::info!("export: {} wrote {} bytes", name, bytes.len());
                crate::ui::download(&bytes, &format!("{name}.{extension}"));
                return Ok(());
            }
        }
        // The yield is load-bearing: the encoders run their callbacks
        // here, and without it the render outruns them and stalls
        // against the renderer's queue guard forever.
        yield_to_page().await;
    }
}

/// Give the event loop a turn. This is where the WebCodecs encoders run
/// their packet callbacks, so it is the difference between a render
/// that finishes and one that wedges.
///
/// A `MessageChannel` round trip rather than `setTimeout(0)`, because a
/// background tab clamps timers to roughly one a second — which turns a
/// render that yields a few hundred times into a render that takes a
/// few hundred seconds. Message tasks aren't clamped, so an export
/// keeps its pace whether or not the tab is the one being looked at.
async fn yield_to_page() {
    let Ok(channel) = web_sys::MessageChannel::new() else {
        // No channel to bounce off: fall back to the timer, slow but
        // never wrong.
        tango_platform::sleep(std::time::Duration::ZERO).await;
        return;
    };
    let (tx, rx) = futures::channel::oneshot::channel::<()>();
    let tx = std::rc::Rc::new(RefCell::new(Some(tx)));
    let on_message = Closure::once(move |_: web_sys::MessageEvent| {
        if let Some(tx) = tx.borrow_mut().take() {
            let _ = tx.send(());
        }
    });
    let port = channel.port1();
    port.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    port.start();
    let _ = channel.port2().post_message(&JsValue::NULL);
    let _ = rx.await;
    port.set_onmessage(None);
    drop(on_message);
}
