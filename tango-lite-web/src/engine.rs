//! The pump: what turns the crank on a running session.
//!
//! [`tango_session`] hands a host a `Drive` whose `tick` advances
//! exactly one emulated frame and leaves *when* entirely to the host. On
//! a desktop that is a thread sleeping to the session's fps target.
//! Here there are no threads, so it is the event loop, from three
//! sources:
//!
//! * `requestAnimationFrame`, the natural beat while the tab is visible,
//!   and the only one of the three at which repainting means anything.
//! * a worker heartbeat (see `assets/tick-worker.js`), which is what
//!   actually keeps a hidden tab alive: rAF stops dead in a background
//!   tab and main-thread timers get clamped to ~1Hz, but worker timers
//!   don't. This matters more than it sounds — a stalled simulation is
//!   not a local inconvenience, it backs the peer's input queue up until
//!   their supervisor gives the link up for dead.
//! * the audio worklet's queue report (~100Hz), which comes for free
//!   while sound is playing. It is deliberately not the only fallback:
//!   an `AudioContext` that never got a user gesture is suspended, and a
//!   suspended context renders nothing and reports nothing.
//!
//! All three call [`pump_now`], which is idempotent in the sense that
//! matters: it advances by however much wall clock has actually passed,
//! so being called twice in one millisecond does nothing the second
//! time. That is what lets three uncoordinated sources drive one loop.
//!
//! The session lives in a thread-local rather than a Dioxus signal.
//! Nothing in it is `Clone`, it is touched 60 times a second, and the UI
//! wants a handful of numbers off it at ~10Hz — so the UI polls
//! [`status`] instead of the engine pushing.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use tango_session::pvp::{PvpDriver, PvpSession};
use tango_session::singleplayer::SinglePlayerSession;
use tango_session::Session;

const SCREEN_W: usize = mgba::gba::SCREEN_WIDTH as usize;
const SCREEN_H: usize = mgba::gba::SCREEN_HEIGHT as usize;

/// The canvas the frame goes to. Looked up by id rather than through a
/// mounted-node handle, because the pump outlives any one render.
const CANVAS_ID: &str = "tango-screen";

/// Longest stretch of wall clock one pump call will try to make up. A
/// tab that was hidden for a minute must not come back and run a
/// minute of emulation; it starts from now.
const MAX_CATCHUP_MS: f64 = 250.0;

/// Ceiling on ticks per pump call, so catch-up can't monopolise the
/// main thread. At 60Hz this still allows 4x realtime, which is more
/// headroom than any of the pacing paths ask for.
const MAX_TICKS_PER_PUMP: u32 = 8;

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
    static RAF: RefCell<Option<Closure<dyn FnMut(f64)>>> = const { RefCell::new(None) };
    /// Whether an animation frame is already scheduled, so the other
    /// tick sources' pump calls don't queue a second one.
    static RAF_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The hidden-tab heartbeat. Built once and told to start and stop,
    /// rather than spawned per session: a worker costs a thread to start
    /// up, and sessions come and go.
    static TICKER: RefCell<Option<Ticker>> = const { RefCell::new(None) };
}

/// The worker whose messages keep a backgrounded session ticking.
struct Ticker {
    worker: web_sys::Worker,
    _onmessage: Closure<dyn FnMut(web_sys::MessageEvent)>,
}

impl Ticker {
    fn new() -> Option<Self> {
        let source = js_sys::Array::of1(&JsValue::from_str(include_str!("../assets/tick-worker.js")));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("text/javascript");
        // A blob URL rather than a served file, for the same reason the
        // audio worklet uses one: the build stays a flat drop of files
        // with no paths to keep in sync.
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&source, &options).ok()?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;
        let worker = web_sys::Worker::new(&url);
        let _ = web_sys::Url::revoke_object_url(&url);
        let worker = worker.ok()?;
        let onmessage = Closure::<dyn FnMut(_)>::new(move |_: web_sys::MessageEvent| pump_now());
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        Some(Self {
            worker,
            _onmessage: onmessage,
        })
    }

    fn set_running(&self, running: bool) {
        let _ = self
            .worker
            .post_message(&JsValue::from_str(if running { "start" } else { "stop" }));
    }
}

/// Start or stop the heartbeat, building the worker on first use.
fn set_ticking(running: bool) {
    TICKER.with(|t| {
        let mut slot = t.borrow_mut();
        if slot.is_none() {
            if !running {
                return;
            }
            *slot = Ticker::new();
        }
        if let Some(ticker) = slot.as_ref() {
            ticker.set_running(running);
        }
    });
}

/// The concrete driver, kept typed rather than boxed as `dyn Drive`:
/// a finished PvP match has a teardown step (flush the replay tail)
/// that the shared trait deliberately doesn't carry, and it consumes
/// the driver.
enum Driver {
    SinglePlayer(tango_session::singleplayer::Driver),
    /// `None` once `finish` has consumed it.
    Pvp(Option<PvpDriver>),
}

impl Driver {
    fn tick(&mut self) -> bool {
        match self {
            Driver::SinglePlayer(d) => d.tick(),
            Driver::Pvp(Some(d)) => d.tick(),
            Driver::Pvp(None) => false,
        }
    }

    fn fps_target(&self) -> f32 {
        match self {
            Driver::SinglePlayer(d) => d.fps_target(),
            Driver::Pvp(Some(d)) => tango_session::Drive::fps_target(d),
            Driver::Pvp(None) => 60.0,
        }
    }

    /// The match is over: let the driver write its tail.
    fn finish(&mut self) {
        if let Driver::Pvp(slot) = self {
            if let Some(driver) = slot.take() {
                driver.finish();
            }
        }
    }
}

/// What kind of session is up, for the UI's chrome.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    SinglePlayer,
    Pvp,
}

/// The cheap read-out the UI polls. Everything expensive stays behind
/// [`ENGINE`].
#[derive(Clone, PartialEq, Debug)]
pub struct Status {
    pub kind: Kind,
    /// Set once the session has ended on its own — a peer quit, a
    /// comm error, a finished match.
    pub ended: bool,
    /// PvP only.
    pub latency_ms: Option<u32>,
    pub frame_delay: u32,
    /// Rounded at the source, not at the point of display: the UI mirrors
    /// this into a signal, and an f32 that wobbles in its last digits
    /// would re-render the play screen on every poll.
    pub tps: u32,
    pub reconnecting: bool,
    pub local_player_index: u8,
    pub opponent: String,
}

struct Engine {
    session: Box<dyn Session>,
    driver: Driver,
    /// The session's audio, read by [`crate::audio::Sink::pump`]. Held
    /// here so it dies with the session it belongs to.
    stream: Option<tango_session::audio::CoreStream>,
    sink: Option<Rc<RefCell<crate::audio::Sink>>>,
    kind: Kind,
    /// `performance.now()` at the last pump, and the fractional frame
    /// carried over from it.
    last_ms: f64,
    debt: f64,
    /// Reused RGBA staging for the canvas blit.
    rgba: Vec<u8>,
    /// Resolved lazily: the pump can start before the canvas has been
    /// rendered, and the canvas is replaced whenever the screen changes.
    ctx: Option<web_sys::CanvasRenderingContext2d>,
    /// Single-player only: where to write the cartridge save back, and
    /// when we last did.
    save_path: Option<std::path::PathBuf>,
    last_save_ms: f64,
}

/// How often a single-player session's savedata is written back. The
/// games write SRAM continuously; this is just how stale a browser tab
/// closing without warning can leave it.
const SAVE_INTERVAL_MS: f64 = 10_000.0;

/// Install a booted single-player session and start pumping.
pub fn start_single_player(
    session: SinglePlayerSession,
    driver: tango_session::singleplayer::Driver,
    stream: tango_session::audio::CoreStream,
    sink: Option<Rc<RefCell<crate::audio::Sink>>>,
    save_path: Option<std::path::PathBuf>,
) {
    install(
        Box::new(session),
        Driver::SinglePlayer(driver),
        stream,
        sink,
        Kind::SinglePlayer,
        save_path,
    );
}

/// Install a booted, primed live match and start pumping.
pub fn start_pvp(
    session: PvpSession,
    driver: PvpDriver,
    stream: tango_session::audio::CoreStream,
    sink: Option<Rc<RefCell<crate::audio::Sink>>>,
) {
    install(
        Box::new(session),
        Driver::Pvp(Some(driver)),
        stream,
        sink,
        Kind::Pvp,
        None,
    );
}

fn install(
    session: Box<dyn Session>,
    driver: Driver,
    stream: tango_session::audio::CoreStream,
    sink: Option<Rc<RefCell<crate::audio::Sink>>>,
    kind: Kind,
    save_path: Option<std::path::PathBuf>,
) {
    stop();
    if let Some(sink) = &sink {
        let mut sink = sink.borrow_mut();
        sink.resume_if_suspended();
        // The previous session's tail must not play under this one.
        sink.flush();
        sink.prime();
    }
    let now = now_ms();
    ENGINE.with(|e| {
        *e.borrow_mut() = Some(Engine {
            session,
            driver,
            stream: Some(stream),
            sink,
            kind,
            last_ms: now,
            debt: 0.0,
            rgba: vec![0u8; SCREEN_W * SCREEN_H * 4],
            ctx: None,
            save_path,
            last_save_ms: now,
        })
    });
    schedule_frame();
    set_ticking(true);
}

/// Tear the running session down. Idempotent.
pub fn stop() {
    set_ticking(false);
    let engine = ENGINE.with(|e| e.borrow_mut().take());
    let Some(mut engine) = engine else { return };
    flush_save(&mut engine);
    engine.session.request_close();
    engine.driver.finish();
    if let Some(sink) = &engine.sink {
        sink.borrow_mut().flush();
    }
}

pub fn is_running() -> bool {
    ENGINE.with(|e| e.borrow().is_some())
}

/// The UI's read-out. `None` when nothing is running.
pub fn status() -> Option<Status> {
    ENGINE.with(|e| {
        let engine = e.borrow();
        let engine = engine.as_ref()?;
        let pvp = engine.session.downcast_ref::<PvpSession>();
        Some(Status {
            kind: engine.kind,
            ended: engine.session.is_ended(),
            latency_ms: pvp.and_then(|p| p.latency()).map(|d| d.as_millis() as u32),
            frame_delay: pvp.map(|p| p.frame_delay()).unwrap_or(0),
            tps: pvp.map(|p| p.tps().round() as u32).unwrap_or(0),
            reconnecting: pvp.map(|p| p.is_reconnecting()).unwrap_or(false),
            local_player_index: pvp.map(|p| p.local_player_index()).unwrap_or(0),
            opponent: pvp.map(|p| p.remote_nickname.clone()).unwrap_or_default(),
        })
    })
}

/// Live-set the local frame delay from the in-match slider. Purely
/// local — the peer is neither told nor asked.
pub fn set_frame_delay(frame_delay: u32) {
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow().as_ref() {
            if let Some(pvp) = engine.session.downcast_ref::<PvpSession>() {
                pvp.set_frame_delay(frame_delay);
            }
        }
    });
}

/// Advance the session by however much wall clock has passed, then
/// repaint and top the audio ring up. Safe to call from anywhere and as
/// often as you like — the work it does is proportional to elapsed time,
/// not to the number of calls.
pub fn pump_now() {
    let now = now_ms();
    let over = ENGINE.with(|e| {
        let mut slot = e.borrow_mut();
        let Some(engine) = slot.as_mut() else {
            return false;
        };
        if !engine.step(now) {
            // Take it out from under the borrow so teardown — which
            // drops the session, and with it the cancellation token the
            // network tasks watch — runs with the cell free.
            return true;
        }
        false
    });
    if over {
        stop();
    }
}

impl Engine {
    /// One pump. `false` once the session is over.
    fn step(&mut self, now: f64) -> bool {
        let elapsed = (now - self.last_ms).clamp(0.0, MAX_CATCHUP_MS);
        self.last_ms = now;

        // The session publishes what rate it wants to run at — 60 for
        // single-player, whatever the throttler shaved it to in a match
        // — and the audio stream stretches to match, so following it
        // here is what keeps the two in step.
        let fps = self.driver.fps_target().max(1.0) as f64;
        self.debt += elapsed * fps / 1000.0;

        self.session.set_joyflags(crate::input::joyflags());

        let mut budget = MAX_TICKS_PER_PUMP;
        while self.debt >= 1.0 && budget > 0 {
            self.debt -= 1.0;
            budget -= 1;
            if !self.driver.tick() {
                return false;
            }
        }
        // Whatever we couldn't afford this time is not owed forever;
        // carrying it would turn one long stall into a permanent
        // fast-forward.
        if self.debt > 1.0 {
            self.debt = 1.0;
        }

        self.paint();

        if let (Some(sink), Some(stream)) = (self.sink.as_ref(), self.stream.as_mut()) {
            sink.borrow_mut().pump(stream);
        }

        if now - self.last_save_ms > SAVE_INTERVAL_MS {
            self.last_save_ms = now;
            flush_save(self);
        }
        true
    }

    fn paint(&mut self) {
        if self.ctx.is_none() {
            self.ctx = canvas_context();
        }
        let Some(ctx) = self.ctx.as_ref() else { return };
        let frame = self.session.frame();
        if frame.len() != SCREEN_W * SCREEN_H * 2 {
            return;
        }
        // mgba hands out its native BGR555; the shared LUT conversion is
        // the same one the desktop's replay renderer uses.
        tango_dataview::rom::bgr555_to_rgba8(&frame, &mut self.rgba);
        let Ok(image) = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(&self.rgba),
            SCREEN_W as u32,
            SCREEN_H as u32,
        ) else {
            return;
        };
        let _ = ctx.put_image_data(&image, 0.0, 0.0);
    }
}

/// Write a single-player session's cartridge savedata back to the file
/// it was loaded from. A match never does this: PvP runs entirely off
/// the committed in-memory image, and writing it back would let a match
/// edit your save.
///
/// The write is refused unless the bytes parse as a save for this game.
/// That is not defensive tidiness — before the cartridge has written its
/// SRAM even once, `export_save` hands back the flash image as it powers
/// on, which is 32KB of `0xff`. Persisting that overwrites a real save
/// with a blank one, and booting a game and backing out of it before the
/// title screen is a completely ordinary thing to do.
fn flush_save(engine: &mut Engine) {
    let Some(path) = engine.save_path.clone() else { return };
    let game = engine.session.local_game();
    let Some(session) = engine.session.downcast_ref::<SinglePlayerSession>() else {
        return;
    };
    let Some(bytes) = session.export_save() else { return };
    if game.parse_save(&bytes).is_err() {
        log::debug!("not persisting {}: the cart hasn't written a save yet", path.display());
        return;
    }
    crate::library::write_save(&path, &bytes);
}

fn canvas_context() -> Option<web_sys::CanvasRenderingContext2d> {
    let canvas: web_sys::HtmlCanvasElement = web_sys::window()?
        .document()?
        .get_element_by_id(CANVAS_ID)?
        .dyn_into()
        .ok()?;
    canvas.set_width(SCREEN_W as u32);
    canvas.set_height(SCREEN_H as u32);
    canvas.get_context("2d").ok()??.dyn_into().ok()
}

/// Drop the cached canvas handle. Called when the screen the canvas
/// lives on is torn down and rebuilt, so the next paint re-resolves
/// instead of drawing into a detached element.
pub fn invalidate_canvas() {
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow_mut().as_mut() {
            engine.ctx = None;
        }
    });
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

/// Ask for the next animation frame, installing the callback on first
/// use. The chain stops when the session does, and `install` restarts it.
fn schedule_frame() {
    if RAF_PENDING.with(|p| p.get()) {
        return;
    }
    RAF.with(|r| {
        let mut slot = r.borrow_mut();
        if slot.is_none() {
            *slot = Some(Closure::new(move |_: f64| {
                RAF_PENDING.with(|p| p.set(false));
                pump_now();
                if is_running() {
                    schedule_frame();
                }
            }));
        }
        if let (Some(window), Some(callback)) = (web_sys::window(), slot.as_ref()) {
            if window
                .request_animation_frame(callback.as_ref().unchecked_ref())
                .is_ok()
            {
                RAF_PENDING.with(|p| p.set(true));
            }
        }
    });
}
