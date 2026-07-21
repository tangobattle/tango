//! The single-threaded session host: one `Runtime` in an
//! `Rc<RefCell<_>>`, pumped from three sources that all funnel into
//! [`Runtime::pump`] —
//!
//! - **rAF** while the tab is visible: ticks + audio + present + a
//!   `FRAME_REV` signal bump for the reactive UI;
//! - **the AudioWorklet's queue reports** (~10.7ms cadence): ticks +
//!   audio only. These keep firing when the tab is hidden and rAF
//!   stops, so a netplay session holds full speed in the background;
//! - double-fires are harmless — the accumulator sees ~0 elapsed.
//!
//! Everything here runs on the JS main thread; the atomics/mutexes in
//! `SharedSession` are uncontended. The one real hazard is re-entrant
//! locking (a single thread deadlocks itself), so the pump strictly
//! sequences tick → audio → present as siblings and nothing calls back
//! into the pump from inside a `with_link` scope.

pub mod clock;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use dioxus::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::platform::audio::web::WebAudio;
use crate::platform::audio::{Binding, LateBinder, LinkStream};
use crate::platform::video::webgl::WebGlPresenter;
use crate::platform::{gamepad, input};
use crate::session::local::{LocalArgs, LocalSession};
use crate::session::{LinkAccess, SessionDescriptor, SessionEnd, SessionKind, SharedSession};

/// Bumped once per pump that changed anything the reactive UI shows
/// (new frame, session end, boot). The canvas is NOT a subscriber —
/// pixels go through WebGL imperatively; this drives the status/menu
/// components.
pub static FRAME_REV: GlobalSignal<u64> = Signal::global(|| 0);

/// Bumped on session start/swap/close — drives structural UI changes.
pub static SESSION_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);

/// Bumped whenever save data changes on disk — every SRAM write-back
/// here, and the UI's own save imports/renames/deletes. The save
/// picker re-lists the selected game's directory on each bump, so it
/// only refreshes once a write has actually landed (relisting at
/// session close raced the async write-back and missed the file).
pub static SAVES_REV: GlobalSignal<u64> = Signal::global(|| 0);

/// SRAM write-backs still in flight. A directory listing taken while
/// this is nonzero may predate the write, so it must not be treated
/// as the truth about which saves exist (the picker's stale-pick
/// pruning once ate a freshly created save this way); the drop back
/// to zero re-lists.
pub static SAVES_IN_FLIGHT: GlobalSignal<u32> = Signal::global(|| 0);

/// The session menu overlay. It lives here rather than in the UI
/// because the document keyboard listener owns the Escape toggle.
pub static MENU_OPEN: GlobalSignal<bool> = Signal::global(|| false);

/// The top-right cable/telemetry panel. Escape collapses it before it
/// opens the menu, keeping a running lobby visible in the background.
pub static PANEL_OPEN: GlobalSignal<bool> = Signal::global(|| false);

/// Binding capture: the settings screen sets this to the key being
/// rebound; the next keyboard press (document listener) or gamepad
/// button/axis edge (pump) lands in [`CAPTURED`]. Escape cancels.
pub static CAPTURE_TARGET: GlobalSignal<Option<input::MappedKey>> = Signal::global(|| None);

/// The capture flow's result, consumed (and cleared) by the settings
/// screen, which applies it to both the Config and [`Runtime::mapping`].
pub static CAPTURED: GlobalSignal<Option<(input::MappedKey, input::PhysicalInput)>> =
    Signal::global(|| None);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PumpSource {
    Raf,
    Audio,
}

/// A running session of either kind, presented uniformly to the pump
/// and the UI. The PvP variant lands with the netplay port (M4).
pub enum Session {
    Local(LocalSession),
}

impl Session {
    fn shared(&self) -> &std::sync::Arc<SharedSession> {
        match self {
            Session::Local(s) => &s.shared,
        }
    }

    fn descriptor(&self) -> &SessionDescriptor {
        match self {
            Session::Local(s) => &s.descriptor,
        }
    }

    fn link(&self) -> &LinkAccess {
        match self {
            Session::Local(s) => &s.link,
        }
    }

    fn tick(&mut self) -> bool {
        match self {
            Session::Local(s) => s.driver.tick(),
        }
    }

    fn kind(&self) -> SessionKind {
        self.descriptor().kind
    }
}

pub struct Runtime {
    session: Option<Session>,
    audio: Option<WebAudio>,
    audio_binder: LateBinder,
    /// RAII: unbinding returns the output to silence.
    audio_binding: Option<Binding>,
    presenter: Option<WebGlPresenter>,
    /// The active canvas's context-loss hooks, removed when the canvas
    /// detaches or is replaced (else every mount stacks another pair).
    canvas_hooks: Option<CanvasHooks>,
    presented_rev: u64,
    clock: clock::TickClock,
    pub held: input::HeldState,
    pub mapping: input::Mapping,
    /// Joyflags held by the on-screen touch overlay, OR'd into every
    /// pump alongside the mapped keyboard/gamepad state.
    pub touch_keys: u32,
    /// OPFS, once the UI has it — the SRAM write-back target.
    storage: Option<crate::storage::Storage>,
    /// The telemetry panel's sample ring, captured per netplay frame.
    metric_history: std::collections::VecDeque<crate::session::MetricSample>,
    /// Where the running cart persists, chosen at boot; survives
    /// PvP swaps (same cart), cleared on close.
    save_target: Option<SaveTarget>,
    /// Where SRAM last actually persisted, kept across close so the UI
    /// can move a "(fresh save)" pick onto the file the session created
    /// (and default a game's first save). Taken by
    /// [`Self::take_persisted_save`].
    last_persisted_save: Option<SaveTarget>,
    /// CRC of the last persisted SRAM, so the autosave skips no-ops.
    saved_crc: Option<u32>,
    last_autosave_ms: f64,
    /// The previous session's end, kept readable after teardown so the
    /// UI can say why; cleared by [`Self::dismiss_end`] or the next boot.
    last_end: Option<SessionEnd>,
    /// Gamepad state on the previous capture-scan pump — binding capture
    /// fires on edges, and the first scan only seeds this baseline so an
    /// already-held input can't bind itself.
    capture_prev: Option<HashSet<input::PhysicalInput>>,
    /// Set while the pump runs — the keyboard handlers and UI callbacks
    /// re-borrow the Runtime, and anything that could re-enter must not.
    _pumping: bool,
}

/// Where a cart's SRAM persists: a file inside the flat `saves/`
/// directory (which game a save belongs to is content-detected, like
/// the desktop scanner).
#[derive(Clone)]
pub struct SaveTarget {
    /// The file name inside `saves/`.
    pub file: String,
}

struct CanvasHooks {
    canvas: web_sys::HtmlCanvasElement,
    lost: Closure<dyn FnMut(web_sys::Event)>,
    restored: Closure<dyn FnMut(web_sys::Event)>,
}

thread_local! {
    /// The app-lifetime runtime singleton, reachable from JS callbacks.
    static RUNTIME: RefCell<Option<Rc<RefCell<Runtime>>>> = const { RefCell::new(None) };
}

impl Runtime {
    /// Create the singleton and install its callback sources (rAF loop,
    /// document keyboard listeners). Idempotent per page load.
    pub fn install() -> Rc<RefCell<Runtime>> {
        if let Some(existing) = RUNTIME.with(|r| r.borrow().clone()) {
            return existing;
        }
        let runtime = Rc::new(RefCell::new(Runtime {
            session: None,
            audio: None,
            audio_binder: LateBinder::new(),
            audio_binding: None,
            presenter: None,
            canvas_hooks: None,
            presented_rev: 0,
            clock: clock::TickClock::new(),
            held: input::HeldState::default(),
            mapping: input::Mapping::default(),
            touch_keys: 0,
            storage: None,
            metric_history: std::collections::VecDeque::with_capacity(
                crate::session::HISTORY_LEN,
            ),
            save_target: None,
            last_persisted_save: None,
            saved_crc: None,
            last_autosave_ms: 0.0,
            last_end: None,
            capture_prev: None,
            _pumping: false,
        }));
        RUNTIME.with(|r| *r.borrow_mut() = Some(runtime.clone()));
        install_raf(Rc::downgrade(&runtime));
        install_keyboard(Rc::downgrade(&runtime));
        install_beforeunload(Rc::downgrade(&runtime));
        install_focus_release(Rc::downgrade(&runtime));
        crate::platform::wakelock::install();
        runtime
    }

    pub fn set_storage(&mut self, storage: crate::storage::Storage) {
        self.storage = Some(storage);
    }

    /// Attach (or replace) the presenter for the session canvas, and
    /// arm context-loss recovery: `webglcontextlost` must be
    /// preventDefault'ed for the browser to restore the context, and
    /// `webglcontextrestored` rebuilds the pipeline on the same canvas.
    pub fn attach_canvas(&mut self, canvas: &web_sys::HtmlCanvasElement) {
        self.drop_canvas_hooks();
        match WebGlPresenter::new(canvas) {
            Ok(p) => {
                self.presenter = Some(p);
                // Force a re-upload on the next pump.
                self.presented_rev = 0;
            }
            Err(e) => log::error!("webgl presenter: {e}"),
        }

        let lost: Closure<dyn FnMut(web_sys::Event)> = Closure::new(|e: web_sys::Event| {
            log::warn!("webgl context lost");
            e.prevent_default();
        });
        let restored: Closure<dyn FnMut(web_sys::Event)> = {
            let canvas = canvas.clone();
            Closure::new(move |_| {
                log::warn!("webgl context restored; rebuilding the presenter");
                if let Some(runtime) = RUNTIME.with(|r| r.borrow().clone()) {
                    if let Ok(mut rt) = runtime.try_borrow_mut() {
                        rt.attach_canvas(&canvas);
                    }
                }
            })
        };
        let _ = canvas
            .add_event_listener_with_callback("webglcontextlost", lost.as_ref().unchecked_ref());
        let _ = canvas.add_event_listener_with_callback(
            "webglcontextrestored",
            restored.as_ref().unchecked_ref(),
        );
        self.canvas_hooks = Some(CanvasHooks {
            canvas: canvas.clone(),
            lost,
            restored,
        });
    }

    fn drop_canvas_hooks(&mut self) {
        if let Some(hooks) = self.canvas_hooks.take() {
            let _ = hooks.canvas.remove_event_listener_with_callback(
                "webglcontextlost",
                hooks.lost.as_ref().unchecked_ref(),
            );
            let _ = hooks.canvas.remove_event_listener_with_callback(
                "webglcontextrestored",
                hooks.restored.as_ref().unchecked_ref(),
            );
        }
    }

    pub fn detach_canvas(&mut self) {
        self.drop_canvas_hooks();
        self.presenter = None;
    }

    /// Boot a fresh solo session for a detected game. The caller must
    /// have ensured the audio sink exists (user-gesture requirement).
    /// `save_file` is the name (inside the flat `saves/` directory) the
    /// cart persists back into (write-back on quit + a 60s autosave);
    /// `None` disables persistence.
    pub fn start_local(
        &mut self,
        game: crate::library::GameRef,
        rom: Vec<u8>,
        save: Option<Vec<u8>>,
        save_file: Option<String>,
    ) -> anyhow::Result<()> {
        self.close_session();
        let rtc = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_millis(js_sys::Date::now() as u64);
        self.saved_crc = save.as_deref().map(crc32fast::hash);
        let session = crate::session::local::start(LocalArgs {
            game,
            rom,
            save,
            rtc,
        })?;
        self.save_target = save_file.map(|file| SaveTarget { file });
        self.last_persisted_save = None;
        self.last_autosave_ms = performance_now();
        self.adopt_session(Session::Local(session));
        Ok(())
    }

    /// Where SRAM last persisted, taken once. Outlives the session so
    /// the UI can see it after close.
    pub fn take_persisted_save(&mut self) -> Option<SaveTarget> {
        self.last_persisted_save.take()
    }

    /// Persist SRAM into the chosen file in the game's save directory
    /// (fire-and-forget; OPFS writes are async and small). No-op when
    /// unchanged since the last write.
    fn persist_sram(&mut self, bytes: Option<Vec<u8>>) {
        let (Some(bytes), Some(target), Some(storage)) = (bytes, &self.save_target, &self.storage)
        else {
            return;
        };
        self.last_persisted_save = Some(target.clone());
        let crc = crc32fast::hash(&bytes);
        if self.saved_crc == Some(crc) {
            return;
        }
        self.saved_crc = Some(crc);
        let target = target.clone();
        let storage = storage.clone();
        *SAVES_IN_FLIGHT.write() += 1;
        wasm_bindgen_futures::spawn_local(async move {
            let dir = storage.saves().clone();
            if let Err(e) = crate::storage::write(&dir, &target.file, &bytes).await {
                log::error!("couldn't write back {}: {e}", target.file);
            } else {
                log::info!("saved {} ({} bytes)", target.file, bytes.len());
            }
            // Either way the folder's truth may have moved — let the
            // picker re-list it (a failed write prunes the phantom).
            *SAVES_IN_FLIGHT.write() -= 1;
            *SAVES_REV.write() += 1;
        });
    }

    /// Export the running cart's SRAM (the presented/local side).
    fn export_sram(&self) -> Option<Vec<u8>> {
        let session = self.session.as_ref()?;
        let player = session.descriptor().local_player;
        session.link().with_link(|link| link.export_save(player))?
    }

    /// Swap in a freshly booted session: bind its audio, prime the
    /// sink, reset the cadence, and tell the UI.
    fn adopt_session(&mut self, session: Session) {
        self.metric_history.clear();
        let sample_rate = self.audio.as_ref().map(|a| a.sample_rate()).unwrap_or(48_000);
        self.audio_binder.set_sample_rate(sample_rate);
        self.audio_binding = self
            .audio_binder
            .bind(Some(Box::new(LinkStream::new(
                session.link().clone(),
                session.shared().clone(),
                sample_rate,
            ))))
            .ok();
        self.session = Some(session);
        if let Some(audio) = &mut self.audio {
            // A fixed silence cushion under the sink's sawtooth; see
            // WebAudio::prime.
            audio.prime(2048);
        }
        self.clock.reset();
        self.last_end = None;
        // Present nothing until the new session publishes its first
        // frame — the canvas keeps the outgoing session's image across
        // PvP swaps instead of flashing the blank vbuf.
        self.presented_rev = 0;
        crate::platform::wakelock::set_active(true);
        *SESSION_EPOCH.write() += 1;
    }

    /// The console's reset button: reboot the solo machine in place.
    /// SRAM is the cart's own memory and survives untouched (the OPFS
    /// autosave keeps covering it); a wireless machine's adapter
    /// power-cycles with the console, exactly as on hardware. PvP
    /// has no reset — one side rebooting isn't an input the link could
    /// replay — so this is a no-op there.
    pub fn reset_session(&mut self) {
        let Some(session) = &self.session else { return };
        if !matches!(session, Session::Local(_)) {
            return;
        }
        session.link().with_link(|link| {
            for i in 0..link.num_players() {
                link.core_mut(i).reset();
            }
            // Reinstate the deepened audio buffer and drop anything the
            // outgoing machine had queued, same as at boot.
            crate::session::prepare_audio_buffers(link);
        });
    }

    pub fn close_session(&mut self) {
        // Write the cart's SRAM back before the machine goes away.
        let sram = self.export_sram();
        self.persist_sram(sram);
        self.save_target = None;
        self.saved_crc = None;
        self.audio_binding = None;
        *MENU_OPEN.write() = false;
        crate::platform::wakelock::set_active(false);
        if self.session.take().is_some() {
            *SESSION_EPOCH.write() += 1;
        }
    }

    /// Why the last session ended. Outlives the session (which is torn
    /// down on the pump that saw it end) so the UI can show an end
    /// overlay until [`Self::dismiss_end`].
    pub fn last_end(&self) -> Option<SessionEnd> {
        self.last_end.clone()
    }

    pub fn dismiss_end(&mut self) {
        if self.last_end.take().is_some() {
            *SESSION_EPOCH.write() += 1;
        }
    }

    pub fn descriptor(&self) -> Option<&SessionDescriptor> {
        self.session.as_ref().map(|s| s.descriptor())
    }

    /// The telemetry panel's sample ring (PvP only; cleared on swap).
    #[allow(dead_code)] // the PvP telemetry panel reads this (M4)
    pub fn metric_history(&self) -> &std::collections::VecDeque<crate::session::MetricSample> {
        &self.metric_history
    }

    /// Install the audio sink (built asynchronously from a user
    /// gesture; see `WebAudio::create`).
    pub fn set_audio(&mut self, audio: WebAudio) {
        self.audio_binder.set_sample_rate(audio.sample_rate());
        self.audio = Some(audio);
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    pub fn shared(&self) -> Option<&std::sync::Arc<SharedSession>> {
        self.session.as_ref().map(|s| s.shared())
    }

    pub fn set_volume(&self, v: f32) {
        self.audio_binder.set_volume(v);
    }

    /// Route one document-level key event. Returns true when the code
    /// is bound (the listener then preventDefaults it).
    pub fn key_event(&mut self, code: &str, pressed: bool) -> bool {
        self.held.set_key(code, pressed);
        self.mapping.binds_code(code)
    }

    /// The single pump every source funnels into.
    pub fn pump(&mut self, source: PumpSource) {
        let now_ms = performance_now();

        // Binding capture: gamepad button/axis edges become the pending
        // binding (keyboard capture lives in the document listener).
        if CAPTURE_TARGET.peek().is_some() {
            let mut snap = input::HeldState::default();
            gamepad::poll_into(&mut snap);
            let active: HashSet<input::PhysicalInput> = input::gamepad_candidates()
                .into_iter()
                .filter(|p| snap.is_active(p))
                .collect();
            if let Some(prev) = &self.capture_prev {
                if let Some(physical) = active.iter().find(|p| !prev.contains(*p)).cloned() {
                    if let Some(key) = CAPTURE_TARGET.write().take() {
                        *CAPTURED.write() = Some((key, physical));
                    }
                }
            }
            self.capture_prev = Some(active);
        } else if self.capture_prev.is_some() {
            self.capture_prev = None;
        }

        // Input: gamepad snapshot + the held keyboard state → joyflags.
        if let Some(session) = &self.session {
            gamepad::poll_into(&mut self.held);
            let joyflags = self.mapping.to_mgba_keys(&self.held) | self.touch_keys;
            session
                .shared()
                .joyflags
                .store(joyflags, std::sync::atomic::Ordering::Relaxed);
            // Hold-to-fast-forward — local sessions only; netplay pace
            // belongs to the throttler.
            if session.kind() == SessionKind::Local {
                let speed = if self.mapping.speed_up_held(&self.held) {
                    300
                } else {
                    100
                };
                session
                    .shared()
                    .speed
                    .store(speed, std::sync::atomic::Ordering::Relaxed);
            }
        }

        // Ticks.
        let mut changed = false;
        if let Some(session) = &mut self.session {
            let shared = session.shared().clone();
            let paused = shared.paused.load(std::sync::atomic::Ordering::Acquire);
            // A hidden tab keeps ticking only for netplay (the cable
            // must hold while alt-tabbed). A hidden solo session idles
            // instead of burning a core in the background forever.
            let idle_hidden = session.kind() == SessionKind::Local && document_hidden();
            if paused || idle_hidden {
                shared.set_fps_target(0.0);
                self.clock.reset();
            } else {
                if shared.take_pace_reset() {
                    self.clock.reset();
                }
                let fps = f32::from_bits(
                    shared
                        .fps_target
                        .load(std::sync::atomic::Ordering::Relaxed),
                )
                .max(crate::session::EXPECTED_FPS * 0.25); // fresh boots start at 0
                for _ in 0..self.clock.due(now_ms, fps) {
                    if !session.tick() {
                        // Ended: tear down, keeping the reason readable
                        // until the UI dismisses it.
                        let end = shared
                            .end
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or(SessionEnd::LocalQuit);
                        log::info!("session ended: {end:?}");
                        self.last_end = Some(end);
                        self.close_session();
                        changed = true;
                        break;
                    }
                    changed = true;
                }
            }
        }

        // Audio: top the sink up from whatever is bound (silence when
        // no session). Strictly after ticks — LinkStream locks the link.
        if let (Some(audio), true) = (&mut self.audio, self.session.is_some()) {
            audio.resume_if_suspended();
            audio.pump(&mut self.audio_binder);
        }

        // Solo autosave: SRAM back to OPFS every minute when it changed
        // (a tab kill then loses at most this window). PvP never
        // autosaves mid-session — the frontier's SRAM is speculative
        // under rollback; it persists at end instead.
        if self.save_target.is_some()
            && self.session.as_ref().map(|s| s.kind()) == Some(SessionKind::Local)
            && now_ms - self.last_autosave_ms > 60_000.0
        {
            self.last_autosave_ms = now_ms;
            let sram = self.export_sram();
            self.persist_sram(sram);
        }

        // Telemetry: one sample per changed pump while the cable is in
        // (the engine's stats are already per-frame; a batch shares its
        // newest reading, matching the native per-frame-notify capture).
        if changed {
            if let Some(session) = &self.session {
                if session.kind() == SessionKind::Pvp {
                    let sample =
                        crate::session::MetricSample::capture(&session.shared().stats.lock().unwrap());
                    if self.metric_history.len() == crate::session::HISTORY_LEN {
                        self.metric_history.pop_front();
                    }
                    self.metric_history.push_back(sample);
                }
            }
        }

        // Debug probes: the simulated frontier and the newest advance's
        // peak sio slices (the lockstep-livelock early-warning), readable
        // from devtools / automation even while the tab is hidden and the
        // UI is frozen — and by the watchdog's heartbeat records.
        if changed {
            if let Some(session) = &self.session {
                let (frontier, slices) = {
                    let stats = session.shared().stats.lock().unwrap();
                    (stats.frontier, stats.slices_peak)
                };
                let _ = js_sys::Reflect::set(
                    &js_sys::global(),
                    &"tangoWebFrontier".into(),
                    &(frontier as f64).into(),
                );
                let _ = js_sys::Reflect::set(
                    &js_sys::global(),
                    &"tangoWebSlices".into(),
                    &(slices as f64).into(),
                );
            }
        }

        // Debug probe: wasm linear-memory pages (64KiB each). Linear
        // memory only ever grows, so a steady climb here is the
        // telltale of a leak long before the tab notices.
        let _ = js_sys::Reflect::set(
            &js_sys::global(),
            &"tangoWebWasmPages".into(),
            &(core::arch::wasm32::memory_size::<0>() as f64).into(),
        );
        // Debug probe: whether fast-forward reads as held — a stuck
        // modifier here once pinned background tabs at 3× CPU.
        let _ = js_sys::Reflect::set(
            &js_sys::global(),
            &"tangoWebSpeedUp".into(),
            &self.mapping.speed_up_held(&self.held).into(),
        );
        // Debug probe: the running session's kind, so the watchdog's
        // heartbeat records say what a wedge interrupted.
        let _ = js_sys::Reflect::set(
            &js_sys::global(),
            &"tangoWebSession".into(),
            &match self.session.as_ref().map(|s| s.kind()) {
                Some(SessionKind::Pvp) => "pvp",
                Some(SessionKind::Local) => "local",
                None => "none",
            }
            .into(),
        );

        // Present + UI signal: only on the visible-path source.
        if source == PumpSource::Raf {
            if let (Some(presenter), Some(session)) = (&mut self.presenter, &self.session) {
                let rev = session
                    .shared()
                    .vbuf_rev
                    .load(std::sync::atomic::Ordering::Acquire);
                // rev 0 = nothing published yet (a freshly swapped-in
                // session): hold the previous image rather than flash
                // its still-blank buffer.
                if rev != self.presented_rev && rev != 0 {
                    self.presented_rev = rev;
                    let vbuf = session.shared().vbuf.lock().unwrap();
                    presenter.present(&vbuf);
                }
            }
            if changed {
                *FRAME_REV.write() += 1;
            }
        }
    }
}

fn performance_now() -> f64 {
    web_sys::window().unwrap().performance().unwrap().now()
}

/// Covers both backgrounded tabs and fully-occluded windows (Chrome
/// reports "hidden" for either; rAF is already stopped in both).
fn document_hidden() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .map(|d| d.hidden())
        .unwrap_or(false)
}

/// The worklet's queue-report hook: pump with the Audio source. Wired
/// by `WebAudio::create` via this free function so the closure only
/// holds a weak handle.
pub fn pump_from_audio_report() {
    if let Some(runtime) = RUNTIME.with(|r| r.borrow().clone()) {
        if let Ok(mut rt) = runtime.try_borrow_mut() {
            rt.pump(PumpSource::Audio);
        }
    }
}

fn install_raf(runtime: Weak<RefCell<Runtime>>) {
    let handle: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let handle2 = handle.clone();
    *handle.borrow_mut() = Some(Closure::new(move |_now: f64| {
        if let Some(rt) = runtime.upgrade() {
            rt.borrow_mut().pump(PumpSource::Raf);
            request_animation_frame(handle2.borrow().as_ref().unwrap());
        }
    }));
    request_animation_frame(handle.borrow().as_ref().unwrap());
}

fn request_animation_frame(closure: &Closure<dyn FnMut(f64)>) {
    web_sys::window()
        .unwrap()
        .request_animation_frame(closure.as_ref().unchecked_ref())
        .expect("requestAnimationFrame");
}

/// Warn before the tab closes over a live session: OPFS writes are
/// async and can't complete during unload, so "use Quit to save" — the
/// autosave interval bounds the loss either way.
fn install_beforeunload(runtime: Weak<RefCell<Runtime>>) {
    let window = web_sys::window().unwrap();
    let closure: Closure<dyn FnMut(web_sys::BeforeUnloadEvent)> =
        Closure::new(move |e: web_sys::BeforeUnloadEvent| {
            let Some(rt) = runtime.upgrade() else { return };
            let Ok(rt) = rt.try_borrow() else { return };
            if rt.shared().is_some() {
                e.prevent_default();
                // Legacy engines want a non-empty returnValue.
                e.set_return_value("A game is running.");
            }
        });
    window
        .add_event_listener_with_callback("beforeunload", closure.as_ref().unchecked_ref())
        .expect("addEventListener");
    closure.forget();
}

/// Release held keyboard keys whenever focus or visibility leaves the
/// tab: the matching keyup fires wherever the user went, so anything
/// still held here would stay "down" forever — most damagingly a held
/// fast-forward, which would keep a background tab at 3× CPU.
fn install_focus_release(runtime: Weak<RefCell<Runtime>>) {
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let release = move || {
        if let Some(rt) = runtime.upgrade() {
            if let Ok(mut rt) = rt.try_borrow_mut() {
                rt.held.release_keys();
            }
        }
    };
    {
        let release = release.clone();
        let closure: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |_| release());
        window
            .add_event_listener_with_callback("blur", closure.as_ref().unchecked_ref())
            .expect("addEventListener");
        closure.forget();
    }
    {
        let closure: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |_| {
            if document_hidden() {
                release();
            }
        });
        document
            .add_event_listener_with_callback("visibilitychange", closure.as_ref().unchecked_ref())
            .expect("addEventListener");
        closure.forget();
    }
}

fn install_keyboard(runtime: Weak<RefCell<Runtime>>) {
    let document = web_sys::window().unwrap().document().unwrap();
    for (event, pressed) in [("keydown", true), ("keyup", false)] {
        let runtime = runtime.clone();
        let doc = document.clone();
        let closure: Closure<dyn FnMut(web_sys::KeyboardEvent)> =
            Closure::new(move |e: web_sys::KeyboardEvent| {
                let code = e.code();
                // Binding capture: the next key press becomes the binding
                // (Escape cancels); either way, neither the game nor the
                // browser sees it.
                if pressed && CAPTURE_TARGET.peek().is_some() {
                    if let Some(key) = CAPTURE_TARGET.write().take() {
                        if code != "Escape" {
                            *CAPTURED.write() =
                                Some((key, input::PhysicalInput::Key(code.as_str().into())));
                        }
                    }
                    e.prevent_default();
                    return;
                }
                let Some(rt) = runtime.upgrade() else { return };
                let Ok(mut rt) = rt.try_borrow_mut() else { return };
                // The event's modifier flags are ground truth for keys
                // whose keyup can vanish without a blur (macOS system
                // shortcuts like ⌘⇧4 ride on modifiers and swallow
                // their releases): an event arriving without a flag
                // proves that modifier is up, however its keyup was
                // lost.
                for (down, sides) in [
                    (e.shift_key(), ["ShiftLeft", "ShiftRight"]),
                    (e.ctrl_key(), ["ControlLeft", "ControlRight"]),
                    (e.alt_key(), ["AltLeft", "AltRight"]),
                    (e.meta_key(), ["MetaLeft", "MetaRight"]),
                ] {
                    if !down {
                        for side in sides {
                            rt.key_event(side, false);
                        }
                    }
                }
                // Releases always land, whatever gate consumed the
                // press: held state must never outlive the physical
                // key, or a stale fast-forward runs the next solo
                // machine at 3×. Everything below is a press-side
                // concern.
                if !pressed {
                    rt.key_event(&code, false);
                    return;
                }
                // Keys belong to the page unless something here consumes
                // them: a live session (game input, the Escape overlays)
                // or its menu. Anywhere else — pickers, settings, lobby —
                // native behavior (buttons, forms, focus) wins.
                if rt.shared().is_none() && !*MENU_OPEN.peek() {
                    return;
                }
                // Escape drives the overlays, never the game: it
                // collapses the cable panel first, then toggles the menu.
                if code == "Escape" {
                    if rt.shared().is_some() {
                        if *PANEL_OPEN.peek() && !*MENU_OPEN.peek() {
                            *PANEL_OPEN.write() = false;
                        } else {
                            let open = *MENU_OPEN.peek();
                            *MENU_OPEN.write() = !open;
                        }
                        e.prevent_default();
                    }
                    return;
                }
                // A focused control owns the keyboard: while the user is
                // in the session's UI (the room-code input, a menu
                // button), bound keys must not double as game input.
                let item_focused = doc
                    .active_element()
                    .is_some_and(|el| !matches!(el.tag_name().as_str(), "BODY" | "HTML"));
                if item_focused {
                    return;
                }
                if rt.key_event(&code, true) {
                    // Bound key: don't let arrows/space scroll the page.
                    e.prevent_default();
                }
            });
        document
            .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
            .expect("addEventListener");
        // App-lifetime listeners: leak deliberately.
        closure.forget();
    }
}
