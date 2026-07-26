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
//! They are not equals. **While rAF is running it is the only clock**;
//! the other two check how long it has been since the last animation
//! frame and do nothing unless it has gone quiet. Letting all three tick
//! is what the first version did, and it stutters: the frame you see is
//! whichever one the sim happened to reach when rAF sampled it, so ticks
//! landing a few milliseconds either side of the frame boundary show up
//! as motion that jitters even though the average rate is exactly right.
//! One clock per displayed frame is what makes it smooth.
//!
//! Painting is likewise rAF's alone, and only when a tick has actually
//! produced something new — there is no point uploading a frame the
//! display will never show, and at 285 combined calls a second that
//! upload was most of the main thread.
//!
//! The session lives in a thread-local rather than a Dioxus signal.
//! Nothing in it is `Clone`, it is touched 60 times a second, and the UI
//! wants a handful of numbers off it at ~10Hz — so the UI polls
//! [`status`] instead of the engine pushing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use tango_session::pvp::{PvpDriver, PvpSession};
use tango_session::replay::ReplaySession;
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

/// How long rAF has to have been silent before the fallback sources
/// take over the clock. Comfortably longer than a frame at any refresh
/// rate, so they stay out of the way while it is running, and short
/// enough that a tab going to the background barely breaks stride.
///
/// Deliberately not `document.hidden`: rAF also stops for a fully
/// occluded window, which reports itself visible. Asking "has a frame
/// happened lately" covers every case without naming any of them.
const FALLBACK_AFTER_MS: f64 = 50.0;

/// A replay's keyframe pass runs one tick at a time, against whatever
/// is left of the frame after the picture is on screen.
///
/// Two versions of this were wrong before this one. A flat slice per
/// frame is wrong because each of these ticks is real emulation on a
/// *second* pair, so a slice of sixteen is sixteen extra frames of work
/// behind every frame shown. A fixed deadline with a multi-tick slice
/// is wrong on a slow device, where a single slice can cost more than
/// the entire budget and the overshoot is exactly the stutter it was
/// supposed to prevent.
///
/// So: one tick at a time, and never start one the remaining budget
/// can't pay for — which needs an estimate of what a tick costs *here*,
/// measured rather than assumed, because that is the number that
/// differs by an order of magnitude between a laptop and a phone.
const PREFETCH_SLICE: u32 = 1;

/// Ceiling on the share of a frame the pass may take, and the fraction
/// of the frame's own interval it may take when that is shorter. A
/// device that can barely hold playback has nothing spare and does
/// nothing — which is the right answer: a stuttering picture is worse
/// than a scrub bar that fills slowly. Pausing gives the whole budget
/// back, since a paused frame does no emulation of its own.
const PREFETCH_UNTIL_MS: f64 = 9.0;
const PREFETCH_FRAME_SHARE: f64 = 0.5;

/// Smoothing on the measured per-tick cost. Slow enough not to chase a
/// single unlucky tick, fast enough to notice the device throttling.
const PREFETCH_COST_DECAY: f64 = 0.8;

/// What a prefetch tick is assumed to cost before one has been timed.
/// Deliberately not zero: a first slice that runs unconditionally is
/// exactly the overshoot this is here to avoid.
const PREFETCH_COST_GUESS_MS: f64 = 1.0;

/// How long a closed session is kept alive so its quit announcement can
/// reach the peer. Comfortably past the `Goodbye` send's own one-second
/// cap, and invisible either way — the UI has already moved on.
const GOODBYE_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

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
    /// When the last animation frame ran. The fallback sources read it
    /// to decide whether rAF still owns the clock (see
    /// [`FALLBACK_AFTER_MS`]).
    static LAST_FRAME_MS: std::cell::Cell<f64> = const { std::cell::Cell::new(f64::NEG_INFINITY) };
    /// The hidden-tab heartbeat. Built once and told to start and stop,
    /// rather than spawned per session: a worker costs a thread to start
    /// up, and sessions come and go.
    static TICKER: RefCell<Option<Ticker>> = const { RefCell::new(None) };
    /// A session that ended by itself, waiting to be collected by the
    /// UI. See [`take_ended`].
    static ENDED: Cell<Option<Kind>> = const { Cell::new(None) };
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
    /// Playback's three loops — drive, seek chase, prefetch — folded
    /// into one, which is the shape `tango_session::replay` offers a
    /// host that has one thread of control rather than three.
    Replay(tango_session::replay::Driver),
}

impl Driver {
    fn tick(&mut self) -> bool {
        match self {
            Driver::SinglePlayer(d) => d.tick(),
            Driver::Pvp(Some(d)) => d.tick(),
            Driver::Pvp(None) => false,
            Driver::Replay(d) => tango_session::Drive::tick(d),
        }
    }

    fn fps_target(&self) -> f32 {
        match self {
            Driver::SinglePlayer(d) => d.fps_target(),
            Driver::Pvp(Some(d)) => tango_session::Drive::fps_target(d),
            Driver::Pvp(None) => 60.0,
            Driver::Replay(d) => tango_session::Drive::fps_target(d),
        }
    }

    /// Spend a slice on a replay's prefetch pass. `false` once there is
    /// nothing left to prefetch (or this isn't a replay at all).
    ///
    /// Deliberately the host's call, and it isn't optional in practice:
    /// the pass races a second pair through the stream laying down
    /// keyframes, and without them a backward seek has nothing to
    /// restore from and has to re-simulate its way there. Skipping it
    /// entirely makes the scrub bar look broken.
    fn prefetch(&mut self, budget: u32) -> bool {
        match self {
            Driver::Replay(driver) => driver.prefetch_step(budget),
            _ => false,
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
    Replay,
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
    /// Replay only: `(playhead, total)` in ticks.
    pub playhead: Option<(u32, u32)>,
    /// Replay only: how far the keyframe pass has got, in ticks. What
    /// the scrub bar shades — seeking inside it lands immediately,
    /// past it has to re-simulate.
    pub prefetched: u32,
    pub paused: bool,
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
    /// The JS-side twin of `rgba` and the `ImageData` aliasing it, built
    /// once per canvas. See [`Engine::paint`].
    surface: Option<(js_sys::Uint8ClampedArray, web_sys::ImageData)>,
    /// A tick has produced a frame that hasn't been drawn yet. Cleared
    /// by the paint, so a display refresh with nothing new behind it
    /// costs nothing.
    fresh: bool,
    /// Measured cost of one replay prefetch tick on this device, so a
    /// frame can tell whether it can afford another one. See
    /// [`PREFETCH_SLICE`].
    prefetch_cost_ms: f64,
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
    // A player holding still through a custom screen is idle as far as
    // the OS is concerned, and a phone that locks mid-match stalls the
    // simulation — which the peer experiences as a dead link.
    crate::wakelock::hold();
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
            surface: None,
            fresh: false,
            prefetch_cost_ms: PREFETCH_COST_GUESS_MS,
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
    crate::wakelock::release();
    let engine = ENGINE.with(|e| e.borrow_mut().take());
    let Some(mut engine) = engine else { return };
    flush_save(&mut engine);
    // For a match this is the quit announcement: it cancels the token
    // the supervisor watches, and the supervisor puts a `Goodbye` on the
    // control channel on its way out. Without it the peer sees a bare
    // channel close, which is indistinguishable from its own reconnect
    // dropping a transport — so it spends its give-up window waiting for
    // someone who already left.
    engine.session.request_close();
    engine.driver.finish();
    if let Some(sink) = &engine.sink {
        sink.borrow_mut().flush();
    }
    // And that announcement is asynchronous, so the session has to
    // outlive this turn. Dropping it here would take the transport down
    // with it before the supervisor ever got scheduled, and the goodbye
    // would never reach the wire.
    wasm_bindgen_futures::spawn_local(async move {
        tango_session::platform::sleep(GOODBYE_GRACE).await;
        drop(engine);
    });
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
        let replay = engine.session.downcast_ref::<ReplaySession>();
        Some(Status {
            playhead: replay.map(|r| (r.current_tick(), r.total_ticks())),
            prefetched: replay.map(|r| r.prefetch_progress()).unwrap_or(0),
            paused: replay.is_some_and(|r| r.is_paused()),
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

/// Install a replay and start playing it back.
pub fn start_replay(
    session: ReplaySession,
    driver: tango_session::replay::Driver,
    stream: tango_session::audio::CoreStream,
    sink: Option<Rc<RefCell<crate::audio::Sink>>>,
) {
    install(
        Box::new(session),
        Driver::Replay(driver),
        stream,
        sink,
        Kind::Replay,
        None,
    );
}

/// Replay transport: pause, resume, and jump.
pub fn set_paused(paused: bool) {
    with_replay(|replay| replay.set_paused(paused));
}

pub fn seek_to(tick: u32) {
    // `resume_after` follows what the transport was doing, so scrubbing
    // a paused replay leaves it paused and scrubbing a playing one
    // picks straight back up.
    with_replay(|replay| {
        let resume = !replay.is_paused();
        replay.seek_to(tick, resume);
    });
}

fn with_replay(f: impl FnOnce(&ReplaySession)) {
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow().as_ref() {
            if let Some(replay) = engine.session.downcast_ref::<ReplaySession>() {
                f(replay);
            }
        }
    });
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

/// Advance the session from one of the fallback tick sources — the
/// worker heartbeat or the audio queue report.
///
/// Does nothing while animation frames are still arriving. rAF is a
/// better clock than either of these (it is *the* clock the picture is
/// sampled on), and having several of them drive the same loop is what
/// makes motion jitter, so they only take over once it has gone quiet.
pub fn pump_now() {
    if now_ms() - LAST_FRAME_MS.with(|t| t.get()) < FALLBACK_AFTER_MS {
        return;
    }
    // If rAF has stopped but the page is still on screen, this is the
    // only thing left that can draw — and a frozen picture over a
    // running simulation is worse than the cost of painting. A hidden
    // page gets no paint at all, which is the usual case here and the
    // whole reason painting isn't done on every tick.
    pump(page_visible());
}

fn page_visible() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .map(|document| document.visibility_state() == web_sys::VisibilityState::Visible)
        .unwrap_or(false)
}

/// Advance the session by however much wall clock has passed, top the
/// audio ring up, and — on the frame path, and only if a tick actually
/// produced one — put the new picture up.
fn pump(on_frame: bool) {
    let now = now_ms();
    let over = ENGINE.with(|e| {
        let mut slot = e.borrow_mut();
        let Some(engine) = slot.as_mut() else {
            return false;
        };
        // Take it out from under the borrow if it's over: teardown drops
        // the session, and with it the cancellation token the network
        // tasks watch, so it wants the cell free.
        !engine.step(now, on_frame)
    });
    if over {
        // Record what ended *before* tearing it down, so the UI can
        // find out afterwards. Watching `status().ended` instead does
        // not work: the moment a session is over the pump stops it, and
        // `status()` is `None` from that instant — a poll fast enough
        // to catch the flag in between is not something to rely on.
        let kind = ENGINE.with(|e| e.borrow().as_ref().map(|engine| engine.kind));
        ENDED.with(|c| c.set(kind));
        stop();
    }
}

/// The kind of session that just ended on its own, if one did. Consumed
/// by the read, so it fires exactly one navigation.
///
/// Only set when a session ends *itself* — a match finishing, a peer
/// leaving, a dead link. A user quitting already knows where they are
/// going and clears this on the way out.
pub fn take_ended() -> Option<Kind> {
    ENDED.with(|c| c.take())
}

impl Engine {
    /// One pump. `false` once the session is over.
    fn step(&mut self, now: f64, on_frame: bool) -> bool {
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
            self.fresh = true;
        }
        // Whatever we couldn't afford this time is not owed forever;
        // carrying it would turn one long stall into a permanent
        // fast-forward.
        if self.debt > 1.0 {
            self.debt = 1.0;
        }

        // Only from an animation frame, and only if there is something
        // new to show: a repaint the display won't sample is pure cost,
        // and it is a large one — a screen's worth of conversion and a
        // 150KB upload into JS.
        if on_frame && self.fresh {
            self.fresh = false;
            self.paint();
        }

        // Whatever the frame has left over goes to a replay's keyframe
        // pass — measured from the top of this frame's work, so it is
        // leftover time and not extra time. Only on the frame path: the
        // fallback exists to keep a session alive, not to get work done.
        if on_frame {
            // Against this frame's own interval, so a display running
            // at 120Hz doesn't hand out a 60Hz frame's worth of slack.
            let interval = elapsed.clamp(8.0, 33.0);
            let deadline = now + PREFETCH_UNTIL_MS.min(interval * PREFETCH_FRAME_SHARE);
            loop {
                let started = now_ms();
                if started + self.prefetch_cost_ms > deadline {
                    break;
                }
                if !self.driver.prefetch(PREFETCH_SLICE) {
                    break;
                }
                let cost = now_ms() - started;
                self.prefetch_cost_ms =
                    self.prefetch_cost_ms * PREFETCH_COST_DECAY + cost * (1.0 - PREFETCH_COST_DECAY);
            }
        }

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
            self.surface = None;
        }
        let Some(ctx) = self.ctx.as_ref() else { return };
        let frame = self.session.frame();
        if frame.len() != SCREEN_W * SCREEN_H * 2 {
            return;
        }
        // mgba hands out its native BGR555; the shared LUT conversion is
        // the same one the desktop's replay renderer uses.
        tango_dataview::rom::bgr555_to_rgba8(&frame, &mut self.rgba);

        // One JS-side buffer for the session's life, with an ImageData
        // view onto it. `new ImageData(array, …)` aliases the array it
        // is given rather than copying, so writing through the handle
        // updates what `put_image_data` will draw — which turns a
        // per-frame allocation of a 150KB typed array (and the garbage
        // that came with it) into a plain copy.
        if self.surface.is_none() {
            let array = js_sys::Uint8ClampedArray::new_with_length((SCREEN_W * SCREEN_H * 4) as u32);
            let Ok(image) = web_sys::ImageData::new_with_js_u8_clamped_array_and_sh(
                &array,
                SCREEN_W as u32,
                SCREEN_H as u32,
            ) else {
                return;
            };
            self.surface = Some((array, image));
        }
        let Some((array, image)) = self.surface.as_ref() else { return };
        array.copy_from(&self.rgba);
        let _ = ctx.put_image_data(image, 0.0, 0.0);
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
    // A GBA frame is fully opaque, and saying so lets the compositor
    // skip blending the canvas against what is behind it — which it
    // does on every one of these, upscaled.
    let options = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&options, &"alpha".into(), &JsValue::FALSE);
    canvas
        .get_context_with_context_options("2d", &options)
        .ok()??
        .dyn_into()
        .ok()
}

/// Drop the cached canvas handle. Called when the screen the canvas
/// lives on is torn down and rebuilt, so the next paint re-resolves
/// instead of drawing into a detached element.
pub fn invalidate_canvas() {
    ENGINE.with(|e| {
        if let Some(engine) = e.borrow_mut().as_mut() {
            engine.ctx = None;
            // The ImageData belongs to the context it was drawn
            // through; a new canvas gets a new one.
            engine.surface = None;
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
                // Stamped before the pump, not after: this is what the
                // fallback sources measure against, and it should say
                // "a frame is happening", not "a frame finished".
                LAST_FRAME_MS.with(|t| t.set(now_ms()));
                pump(true);
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
