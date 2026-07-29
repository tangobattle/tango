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
/// `None` here would mean lossless RGB H.264 + FLAC in Matroska, which
/// is a beautiful thing to make on a phone and a terrible thing to try
/// to share from one.
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

fn set(state: Option<State>) {
    STATE.with(|s| *s.borrow_mut() = state);
    crate::library::touch();
}

/// Render the recording at `path` and hand the result to a download.
pub async fn run(path: std::path::PathBuf, name: String) {
    if is_running() {
        return;
    }
    set(Some(State::Rendering { done: 0, total: 1 }));
    let canceller = Canceller::new();
    CANCELLER.with(|c| *c.borrow_mut() = Some(canceller.clone()));

    match render(&path, &name, &canceller).await {
        Ok(()) => set(None),
        Err(_) if canceller.is_cancelled() => set(None),
        Err(e) => {
            log::warn!("export {}: {e}", path.display());
            set(Some(State::Failed(e)));
        }
    }
    CANCELLER.with(|c| *c.borrow_mut() = None);
}

async fn render(path: &std::path::Path, name: &str, canceller: &Canceller) -> Result<(), String> {
    let replay = crate::library::read_replay(path)?;
    let (games, roms) = crate::playback::resolve(&replay)?;

    // The same boot the player uses, so the render reproduces the
    // recorded match rather than a similar one.
    let config = tango_backend_mgba::r#match::playback::BootConfig {
        roms: [roms[0].to_vec(), roms[1].to_vec()],
        saves: replay.srams.clone(),
        support: [games[0].pvp, games[1].pvp],
        match_type: (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
        rng_seed: replay.rng_seed,
        rtc: replay.rtc_time(),
        // The games' own audio is the point of a video.
        disable_bgm: false,
    };
    let inputs: Vec<[u32; 2]> = replay.inputs.iter().map(|&row| row.map(|i| i.keys as u32)).collect();
    if inputs.is_empty() {
        return Err("this recording has no frames".into());
    }

    // Whole replay, every round. The desktop's export form lets you
    // pick a clip and deselect rounds; on a phone the useful answer is
    // "the match".
    let clip = Clip {
        start: 0,
        end: inputs.len() as u32 - 1,
        snapshot: None,
        // `round_starts` includes the leading 0; the marks are the
        // *transitions*, so it goes.
        round_marks: replay
            .round_starts
            .iter()
            .copied()
            .filter(|&tick| tick != 0)
            .map(|tick| tick as u32)
            .collect(),
    };
    let rounds = clip.round_marks.len() + 1;
    let rounds_mask = vec![true; rounds];
    let round_titles: Vec<String> = (1..=rounds).map(|n| format!("Round {n}")).collect();

    let request = Request {
        config: &config,
        inputs: &inputs,
        local_player: replay.local_player_index as usize,
        rounds_mask: &rounds_mask,
        round_titles: &round_titles,
        clip: &clip,
        scale: SCALE,
        twosided: false,
    };

    // Booting the re-sim pair blocks; let the screen show the progress
    // bar before it does.
    yield_to_page().await;
    log::info!("export: booting the re-simulation for {} ticks", inputs.len());
    let mut render = Render::new(&request, || Ok(Cursor::new(Vec::new())), canceller).map_err(|e| e.to_string())?;

    loop {
        match render.pump(SLICE_TICKS).map_err(|e| e.to_string())? {
            Progress::Rendering { done, total } => set(Some(State::Rendering { done, total })),
            Progress::Flushing => set(Some(State::Flushing)),
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
        tango_session::platform::sleep(std::time::Duration::ZERO).await;
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
