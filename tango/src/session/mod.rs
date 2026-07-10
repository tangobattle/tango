//! Live emulator-session machinery: state struct, per-session
//! Message + update + view + subscription. Owned by App as
//! `session: session::State` and routed via `Message::Session(_)`.
//!
//! The Play / Replays tabs are responsible for STARTING sessions
//! (they construct an ActiveSession via [`build_playback`] /
//! [`spawn_singleplayer`] and stuff it into `state.active`); this
//! module handles everything that happens after.

pub mod pvp;
pub mod replay;
pub mod singleplayer;
pub mod view;

use crate::anim;
use crate::app::Scanners;
use crate::audio;
use crate::config;
use crate::game;
use crate::i18n::t;
use crate::patch;
use crate::save_view;
use crate::selection;
use crate::style::{self, TEXT_BODY, TEXT_CAPTION};
use crate::video::framebuffer::Effect;
use crate::widgets;
use iced::widget::canvas::{self, Canvas, Frame, LineCap, Path, Stroke};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, container, stack, text};
use iced::{mouse, Alignment, Color, Element, Fill, Length, Point, Rectangle, Renderer, Theme};
use lucide_icons::Icon;
use tango_pvp::battle::{suggest_frame_delay, MAX_FRAME_DELAY, MIN_FRAME_DELAY};
use unic_langid::LanguageIdentifier;

/// Create the mgba core every session boots from: a GBA core with audio-sync
/// on, its video buffer enabled, and `rom` loaded. Callers then load the save
/// (which differs per session — RW file vs in-memory SRAM dump) and install
/// their own traps.
pub(crate) fn new_gba_core(rom: &[u8]) -> anyhow::Result<mgba::core::Core> {
    let mut core = mgba::core::Core::new_gba(
        "tango",
        &mgba::core::Options {
            audio_sync: true,
            ..Default::default()
        },
    )?;
    core.enable_video_buffer();
    core.as_mut().load_rom(mgba::vfile::VFile::from_vec(rom.to_vec()))?;
    Ok(core)
}

/// At most one of these can be active at a time: replay playback,
/// single-player, or PvP. The variants share enough surface (vbuf,
/// close-request) that the view + tick loop wrap them uniformly.
pub enum ActiveSession {
    Replay(replay::ReplaySession),
    SinglePlayer(singleplayer::SinglePlayerSession),
    /// Boxed: `PvpSession` is ~2.5 KB, an order of magnitude bigger
    /// than the other variants.
    PvP(Box<pvp::PvpSession>),
}

impl ActiveSession {
    /// Pre-drop teardown. Only PvP has any: it cancels its token so the
    /// receive loop announces the quit to the peer instead of leaving them
    /// hanging on a reconnect window. Replay and single-player sessions
    /// close by being dropped (the mgba thread joins in Drop).
    pub fn request_close(&self) {
        match self {
            Self::PvP(s) => s.request_close(),
            Self::Replay(_) | Self::SinglePlayer(_) => {}
        }
    }

    /// True once the session has ended on its own — currently used
    /// by PvP so a peer-disconnect / comm error tears the session
    /// view down automatically instead of leaving the user staring
    /// at a frozen frame.
    pub fn is_ended(&self) -> bool {
        match self {
            Self::Replay(_) | Self::SinglePlayer(_) => false,
            Self::PvP(s) => s.is_ended(),
        }
    }

    pub fn as_replay(&self) -> Option<&replay::ReplaySession> {
        match self {
            Self::Replay(s) => Some(s),
            _ => None,
        }
    }

    /// Local-perspective Game registration for this session. Used by
    /// the session view to pull per-game chrome (background image,
    /// logo) into the emulator pane.
    pub fn local_game(&self) -> &'static crate::game::Game {
        match self {
            Self::Replay(s) => s.game(),
            Self::SinglePlayer(s) => s.game(),
            Self::PvP(s) => s.game(),
        }
    }
}

/// One per-frame snapshot of the live PvP telemetry, retained in a short ring
/// buffer ([`State::metric_history`]) so the match-settings popover can draw a
/// sparkline per metric. `round` is `None` between rounds, when no
/// skew/lead/depth reading exists; when present it is `(skew, depth, lead)`.
#[derive(Clone, Copy)]
pub struct MetricSample {
    pub tps: f32,
    pub fps_target: f32,
    pub ping_ms: u128,
    pub round: Option<(i32, u32, i32)>,
}

impl MetricSample {
    /// Read the current telemetry off a live PvP session. Called once per
    /// emulator frame from the [`Message::UpdateFramebuffer`] handler.
    fn capture(pvp: &pvp::PvpSession) -> Self {
        Self {
            tps: pvp.tps(),
            fps_target: pvp.fps_target(),
            // Raw latest ping (not the median) — the sparkline is a live
            // display, so it should track the true per-frame reading and
            // show spikes. The median feeds only the frame-delay suggestion.
            ping_ms: pvp.latency_raw().map_or(0, |d| d.as_millis()),
            round: pvp.round_stats().map(|s| (s.skew, s.depth, s.lead)),
        }
    }
}

/// How many frames of telemetry the sparklines retain (~3 s at 60 fps).
const METRIC_HISTORY_LEN: usize = 180;

/// Per-session UI state. App holds `session: State`; the Play and
/// Replays tabs swap an `ActiveSession` into `active` to start a
/// session, then [`State::update`] handles the rest until [`Close`]
/// clears it.
pub struct State {
    /// Permanent iced ↔ emu-thread wake handle. Cloned into each
    /// active session at construction so its frame callback (and
    /// PvP end-detection wires) can `notify_one()` whenever a new
    /// frame lands or `is_ended` could flip. The [`subscription`]
    /// `.notified().await`s on this single Notify across the
    /// program's lifetime — no per-session re-keying needed.
    pub frame_notify: std::sync::Arc<tokio::sync::Notify>,
    /// Shared GBA framebuffer. The active session's frame callback
    /// `copy_from_slice`s mgba's video buffer into this Mutex once
    /// per emu vblank; the [`Message::UpdateFramebuffer`] handler
    /// locks it, clones the bytes, and rebuilds
    /// [`State::current_frame`]. Pre-sized to GBA dimensions and
    /// reused across sessions — saves the per-session
    /// `Arc<Mutex<Vec<u8>>>` allocation dance and lets the handler
    /// read straight off `State` without dispatching through
    /// `ActiveSession`.
    pub vbuf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    pub active: Option<ActiveSession>,
    /// PvP-only: the opponent's save-view side panel, shown when
    /// they haven't blinded their setup. Defaults to hidden; user
    /// opens it via the edge handle. The drawer slides in from the
    /// screen edge and the edge handle rides its moving inner edge.
    pub opponent_panel: anim::Overlay,
    /// PvP-only: the local player's save-view side panel. Defaults
    /// to hidden; user toggles it via the red toolbar button. Slides
    /// the same way as [`opponent_panel`](Self::opponent_panel).
    pub self_panel: anim::Overlay,
    /// Combined keyboard + gamepad held state. Updated from
    /// the input event stream; the user's Mapping resolves it
    /// into mgba joyflags each event.
    pub input_held: crate::input::HeldState,
    /// Last value of `mapping.speed_up_held(...)` so we can
    /// detect the falling/rising edge and only call set_speed
    /// when it actually flips.
    pub speed_up_engaged: bool,
    /// In-session Settings overlay. Toggled by the Settings
    /// icon in the status bar (`Message::OpenSettings`) and the
    /// "back to session" button on the overlay itself
    /// (`Message::CloseSettings`). The emulator keeps running
    /// underneath; we just swap what `App::view` renders.
    pub settings: anim::Overlay,
    /// PvP-only: the "are you sure?" modal that gates the
    /// Disconnect item in the options menu. Disconnect tears the
    /// session down mid-match (same as Close), so the confirm
    /// keeps a stray click from costing the user a real game.
    pub disconnect: anim::Overlay,
    /// PvP-only: the match-settings popover, anchored above the
    /// telemetry plate (instrument panel) and toggled by clicking it.
    /// Holds the live frame-delay control (moved here from the footer).
    /// Mutually exclusive with the options menu.
    pub match_settings: anim::Overlay,
    /// Latest GBA framebuffer (post upscale filter), presented by the
    /// [`crate::video::framebuffer`] shader widget. Refreshed in
    /// [`Message::UpdateFramebuffer`] (which the session subscription
    /// fires once per emulator vblank). `None` between sessions and
    /// before the first frame lands.
    pub current_frame: Option<crate::video::framebuffer::Frame>,
    /// Monotonic counter stamped into each [`current_frame`] so the
    /// framebuffer pipeline can skip re-uploading when the same frame
    /// is presented twice (a UI redraw with no new emu frame).
    pub frame_revision: u64,
    /// Replay-only: the opponent's screen while the PiP toggle is on,
    /// drawn as a picture-in-picture inset by the session view. `None`
    /// whenever the PiP isn't live. Rebuilt alongside
    /// [`current_frame`](Self::current_frame) each emu frame.
    pub pip_frame: Option<crate::video::framebuffer::Frame>,
    /// [`frame_revision`](Self::frame_revision)'s twin for the PiP
    /// surface (a separate GPU texture with its own upload dedupe).
    pub pip_revision: u64,
    /// Rolling window of PvP telemetry snapshots (newest at the back),
    /// sampled once per frame from the [`Message::UpdateFramebuffer`] handler
    /// and drawn as sparklines in the match-settings popover. Capped at
    /// [`METRIC_HISTORY_LEN`]; cleared whenever the active session is not a
    /// live PvP match.
    pub metric_history: std::collections::VecDeque<MetricSample>,
    /// Replay-only: scrub-bar interaction state (drag preview, the
    /// floating hover thumbnail, and the bookkeeping that ties them to
    /// the running playback session). Inert outside a replay session.
    pub scrub: replay::Scrub,
    /// Wall-clock of the last cursor movement over the session
    /// view — drives the floating controls' auto-hide. Bumped by
    /// [`Message::MouseMoved`] and on session start
    /// ([`State::wake_controls`]).
    pub last_mouse_move: std::time::Instant,
    /// Cursor is currently over the floating controls bar — pins
    /// it visible regardless of the idle timer.
    pub controls_hovered: bool,
    /// Instant the current Esc hold started, `None` while Esc is up.
    /// Armed on the first [`Message::EscPressed`] of a physical hold
    /// (key repeat re-fires the message but not the arm), cleared on
    /// [`Message::EscReleased`]. Drives hold-to-quit: the view draws
    /// the exit overlay for the whole hold, and at [`ESC_QUIT_HOLD`]
    /// the [`update`](State::update) wrapper tears the session down.
    pub esc_hold: Option<std::time::Instant>,
    /// Show/hide transition for the floating controls bar. Synced
    /// after every update: shown while the mouse moved recently,
    /// the cursor rests on the bar, any overlay is open, a scrub
    /// is in flight, or a replay is paused. Unlike the [`anim::Overlay`]
    /// fields above it has no companion bool — its target is recomputed
    /// from those inputs each update rather than toggled by a handler.
    pub controls_anim: anim::Transition,
}

impl Default for State {
    fn default() -> Self {
        Self {
            frame_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
            vbuf: std::sync::Arc::new(std::sync::Mutex::new(vec![
                0u8;
                // Raw BGR555 from mgba: 2 bytes/pixel. The framebuffer shader
                // expands it to RGB on the GPU (see `video::framebuffer`).
                (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 2)
                    as usize
            ])),
            active: None,
            opponent_panel: anim::Overlay::new(false),
            self_panel: anim::Overlay::new(false),
            input_held: crate::input::HeldState::default(),
            speed_up_engaged: false,
            settings: anim::Overlay::new(false),
            disconnect: anim::Overlay::new(false),
            match_settings: anim::Overlay::new(false),
            current_frame: None,
            frame_revision: 0,
            pip_frame: None,
            pip_revision: 0,
            metric_history: std::collections::VecDeque::new(),
            scrub: replay::Scrub::default(),
            last_mouse_move: std::time::Instant::now(),
            controls_hovered: false,
            esc_hold: None,
            controls_anim: anim::Transition::new(true),
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    /// True iff a session is running. Drives main.rs's view routing.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

/// Messages the session pane emits + handles. All variants are
/// inert when `state.active` is `None`.
#[derive(Debug, Clone)]
pub enum Message {
    /// Close the session and return to the previous tab.
    Close,
    /// Cursor moved anywhere over the session view. Resets the
    /// floating controls' idle timer.
    MouseMoved,
    /// Cursor entered (`true`) / left (`false`) the floating
    /// controls bar. While inside, the bar never auto-hides.
    ControlsHovered(bool),
    /// Raw input event from the keyboard or a gamepad. The
    /// handler updates the held-state set, resolves the user's
    /// Mapping into joyflags, and pushes them to the active
    /// session. Speed-up uses the same mechanism (edge-
    /// detected).
    Input(InputEvent),
    /// Toggle play/pause on a replay session. No-op for single-player.
    TogglePlay,
    /// Scrub-bar drag in progress — fires per tick change while the
    /// button is held. Pauses playback and blits the nearest prefetched
    /// snapshot's framebuffer as an instant preview; the exact seek
    /// waits for [`Message::ScrubCommit`]. Replay-only.
    ScrubPreview(u32),
    /// Scrub-bar drag released. Fires the real (asynchronous) seek to
    /// the last previewed tick and resumes playback if it was running
    /// when the drag started. Replay-only.
    ScrubCommit(u32),
    /// Cursor moved onto / along the scrub bar (`Some`) or off it
    /// (`None`) without a button held. Drives the floating keyframe
    /// thumbnail above the bar. Replay-only.
    ScrubHover(Option<replay::scrubber::HoverInfo>),
    /// Set the playback speed factor (1.0 = realtime). Replay-only.
    SetSpeed(f32),
    /// Toggle the input display overlay (the recorded pad state of
    /// both sides, drawn over playback). Replay-only. The flag lives
    /// in config — the App's wrapper flips + persists it, same as
    /// [`Message::SetFrameDelay`]; nothing to do here.
    ToggleInputDisplay,
    /// Replay-only: toggle the opponent-screen picture-in-picture (the
    /// transport bar's PiP button).
    TogglePip,
    /// PvP-only: the match-settings frame-delay slider moved. Live-sets this
    /// side's local frame delay on the running session; the App also persists it
    /// to config. No peer coordination — it's purely a local display lag.
    SetFrameDelay(u32),
    /// PvP-only: open/close the match-settings popover anchored on the
    /// telemetry plate (instrument panel). Mutually exclusive with the
    /// options menu.
    ToggleMatchSettings,
    /// User pressed Esc inside a session. Dismisses whichever overlay
    /// is on top (settings modal, disconnect confirm, match-settings
    /// popover) if any, and arms the hold-to-quit timer — a tap never
    /// tears the session down, but holding Esc for [`ESC_QUIT_HOLD`]
    /// does (with the exit overlay counting down the hold). Routed
    /// here rather than from the InputCapture so the decision sees
    /// the current overlay state.
    EscPressed,
    /// Esc came back up — disarms hold-to-quit.
    EscReleased,
    /// Redraw/quit-check heartbeat while Esc is held, from the
    /// [`subscription`] timer branch. No handler work of its own:
    /// the elapsed-hold check lives in the [`update`](State::update)
    /// wrapper, and a paused replay (or a mid-reconnect PvP pause)
    /// produces no frame wakes to run it — this keeps the overlay
    /// filling and the quit firing anyway.
    EscHoldTick,
    /// Show the "really disconnect?" modal. PvP-only; picked from
    /// the options menu's Disconnect item, which also dismisses
    /// the popover.
    OpenDisconnectConfirm,
    /// Dismiss the disconnect confirm without disconnecting (the
    /// Cancel button + the modal backdrop both fire this).
    CloseDisconnectConfirm,
    /// Show/hide the opponent's setup side panel. PvP-only.
    ToggleOpponentPanel,
    /// Show/hide the local player's save-view panel. PvP-only.
    ToggleSelfPanel,
    /// User interacted with the opponent's save-view (tab swap,
    /// folder-group toggle, hover, …). PvP-only.
    OpponentSaveViewAction(save_view::Action),
    /// Mirror of [`OpponentSaveViewAction`] for the local panel.
    SelfSaveViewAction(save_view::Action),
    /// Show the in-session Settings overlay. The emulator keeps
    /// running; only the visible body swaps. Replaces the
    /// legacy in-game pause menu.
    OpenSettings,
    /// Hide the in-session Settings overlay (the "back to
    /// session" button on the overlay's header).
    CloseSettings,
    /// One emulator frame has landed, or `is_ended` could have
    /// flipped (PvP peer-end / disconnect / grace-timeout). The
    /// handler rebuilds the framebuffer from the active
    /// session's vbuf into [`State::current_frame`] and tears
    /// the session down if it's now ended. Fired by the session
    /// subscription, which wakes on [`State::frame_notify`] —
    /// `notify_one()`'d by both the frame callback and the PvP
    /// end-detection wires.
    UpdateFramebuffer,
    /// Click-swallower for modal panel chrome — keeps presses
    /// on the panel's inert regions from falling through to the
    /// dismiss-on-press backdrop layer beneath it.
    NoOp,
}

/// Atomic input event we feed to the mapping resolver. Lives in
/// [`crate::input`] (as [`Event`](crate::input::Event)) because the
/// settings input pane's live binding highlight consumes the same
/// normalized stream.
pub use crate::input::Event as InputEvent;

/// Per-keypress playhead delta for the replay seek keybinds, in recorded
/// frames. Arrow keys jump ±5 seconds (300 frames at 60fps); comma/period
/// frame-step by ±1. `None` for any other key.
fn replay_seek_delta(physical: iced::keyboard::key::Physical) -> Option<i32> {
    use iced::keyboard::key::{Code, Physical};
    // 5 seconds at the GBA's 60fps.
    const JUMP: i32 = 300;
    match physical {
        Physical::Code(Code::ArrowLeft) => Some(-JUMP),
        Physical::Code(Code::ArrowRight) => Some(JUMP),
        Physical::Code(Code::Comma) => Some(-1),
        Physical::Code(Code::Period) => Some(1),
        _ => None,
    }
}

impl State {
    /// Apply a session message to the state. Returns the iced Task
    /// that should be scheduled (always Task::none today — kept for
    /// API parity with the other tabs).
    pub fn update(&mut self, msg: Message, mapping: &crate::input::Mapping) -> iced::Task<Message> {
        let task = self.update_inner(msg, mapping);
        // Hold-to-quit: Esc held to the threshold tears the session
        // down, same as the Close button. Checked here on every
        // message (the 60 Hz frame wakes, plus the dedicated
        // EscHoldTick stream when the emulator is paused) instead of
        // in a handler, so it doesn't care which message crossed the
        // finish line.
        if self.esc_hold.is_some_and(|t| t.elapsed() >= ESC_QUIT_HOLD) {
            if self.active.is_some() {
                self.close_session();
            } else {
                // The session went away mid-hold with the release
                // swallowed by the view unmount — disarm so the tick
                // subscription doesn't run forever.
                self.esc_hold = None;
            }
        }
        // Mirror each overlay's bool into its transition in one
        // place — handlers above flip them freely and the
        // animations follow, including the multi-flip paths (Esc
        // peeling, mutual-exclusion closes).
        let now = iced::time::Instant::now();
        self.settings.sync(now);
        self.disconnect.sync(now);
        self.match_settings.sync(now);
        self.self_panel.sync(now);
        self.opponent_panel.sync(now);
        // Floating controls auto-hide. The per-frame
        // UpdateFramebuffer messages re-run this, so the idle
        // timer expires without needing its own timer source; a
        // paused replay (no frames) pins the bar visible anyway.
        let replay_paused = self
            .active
            .as_ref()
            .and_then(ActiveSession::as_replay)
            .is_some_and(|r| r.is_paused());
        // The telemetry panel (match_settings) deliberately
        // doesn't count: it lives in the permanently-visible
        // top-right indicator, independent of the HUD controls,
        // so leaving the graph open shouldn't pin the chips up.
        let overlay_open = self.settings.shown() || self.disconnect.shown();
        let show_controls = self.controls_hovered
            || overlay_open
            || replay_paused
            || self.scrub.preview.is_some()
            || self.last_mouse_move.elapsed() < CONTROLS_HIDE_AFTER;
        self.controls_anim.set(show_controls, now);
        task
    }

    /// Reset the floating controls' idle timer — called by the App
    /// when a session starts so the bar greets the user visible
    /// even if the mouse hasn't moved in a while. Also clears the
    /// hover pin: closing a session removes its widgets without
    /// any `on_exit` firing (the cursor is usually ON the close
    /// button), and a latched `controls_hovered` would pin the
    /// next session's chrome on screen permanently.
    pub fn wake_controls(&mut self) {
        self.last_mouse_move = std::time::Instant::now();
        self.controls_hovered = false;
        // Belt-and-braces: a hold left over from a previous session
        // (its release swallowed with the session view) must not
        // count against the new one.
        self.esc_hold = None;
    }

    /// Tear down the active session: PvP pre-drop close request, then
    /// drop-by-clearing plus the reset of every piece of per-session
    /// UI state. Shared by [`Message::Close`] (the Close button /
    /// disconnect confirm) and the Esc hold-to-quit expiry in
    /// [`update`](State::update).
    fn close_session(&mut self) {
        if let Some(s) = self.active.as_ref() {
            s.request_close();
        }
        self.active = None;
        self.current_frame = None;
        self.pip_frame = None;
        self.controls_hovered = false;
        self.disconnect.close();
        self.match_settings.close();
        self.scrub.clear();
        self.esc_hold = None;
    }

    /// Play/pause the active replay (no-op for other session kinds).
    /// Shared by the transport button's [`Message::TogglePlay`] and the
    /// spacebar keybind.
    fn toggle_replay_play(&self) {
        if let Some(s) = self.active.as_ref().and_then(ActiveSession::as_replay) {
            if s.seek_will_resume() {
                // An in-flight seek is about to resume playback, so the
                // button shows "Pause" — honor the press as one: land the
                // seek, stay paused.
                s.cancel_seek_resume();
            } else {
                // Play at end-of-replay: rewind to start and play through
                // again. Mirrors any media player — "play" on a finished
                // track restarts it. The seek is asynchronous, so resuming
                // is deferred to the chase landing — unpausing here would
                // run frames off the end before the rewind starts.
                let paused = s.is_paused();
                if paused && s.current_tick() >= s.total_ticks() {
                    s.seek_to(0, true);
                } else {
                    s.set_paused(!paused);
                }
            }
        }
    }

    fn update_inner(&mut self, msg: Message, mapping: &crate::input::Mapping) -> iced::Task<Message> {
        match msg {
            Message::Close => {
                self.close_session();
            }
            Message::Input(ev) => {
                // Replay transport keybinds: arrow keys jump ±5s, comma/period
                // step ±1 frame. A replay plays back recorded input, so these
                // keys are free to drive the seek bar; live sessions fall
                // through to the joyflag pipeline below as normal. Fires on
                // every press, key-repeat included, so holding scrubs.
                if let InputEvent::Key {
                    physical,
                    pressed: true,
                } = &ev
                {
                    // Edge: compare against the held set *before* the match
                    // below records this press, so OS key-repeat (which the
                    // seek keys want but the pause toggle doesn't) is filtered.
                    let fresh_press = !self.input_held.is_key_held(physical);
                    if let Some(s) = self.active.as_ref().and_then(ActiveSession::as_replay) {
                        if let Some(delta) = replay_seek_delta(*physical) {
                            // Chain off the in-flight seek's target so a burst
                            // of presses accumulates instead of all snapping to
                            // the same spot.
                            let base = s.pending_seek_target().unwrap_or_else(|| s.current_tick());
                            let target = base.saturating_add_signed(delta).min(s.total_ticks());
                            // Preserve the logical play state across the seek
                            // (the thread is paused for the chase either way).
                            let playing = !s.is_paused() || s.seek_will_resume();
                            s.seek_to(target, playing);
                        } else if fresh_press
                            && matches!(
                                physical,
                                iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Space)
                            )
                        {
                            self.toggle_replay_play();
                        }
                    }
                }
                self.input_held.apply(&ev);
                let joyflags = mapping.to_mgba_keys(&self.input_held);
                match self.active.as_ref() {
                    Some(ActiveSession::SinglePlayer(s)) => s.set_joyflags(joyflags),
                    Some(ActiveSession::PvP(s)) => s.set_joyflags(joyflags),
                    _ => {}
                }
                // Speed-up: only fire set_speed on the rising or
                // falling edge so we don't spam mgba's audio
                // sync target with no-op writes.
                let now_engaged = mapping.speed_up_held(&self.input_held);
                if now_engaged != self.speed_up_engaged {
                    self.speed_up_engaged = now_engaged;
                    let factor = if now_engaged { 4.0 } else { 1.0 };
                    match self.active.as_ref() {
                        Some(ActiveSession::SinglePlayer(s)) => s.set_speed(factor),
                        Some(ActiveSession::Replay(s)) => s.set_speed(factor),
                        // PvP runs at fixed EXPECTED_FPS.
                        Some(ActiveSession::PvP(_)) | None => {}
                    }
                }
            }
            Message::TogglePlay => self.toggle_replay_play(),
            Message::ScrubPreview(target) => {
                if let Some(s) = self.active.as_ref().and_then(ActiveSession::as_replay) {
                    self.scrub.drag(target, s);
                }
                // The drag blits its keyframes to the main screen —
                // the floating hover thumbnail is redundant under it.
                self.scrub.hover = None;
            }
            Message::ScrubCommit(target) => {
                if let Some(s) = self.active.as_ref().and_then(ActiveSession::as_replay) {
                    s.seek_to(target, self.scrub.resume);
                }
                self.scrub.end_drag();
            }
            Message::ScrubHover(hover) => {
                self.scrub.hover = hover;
                if let Some(s) = self.active.as_ref().and_then(ActiveSession::as_replay) {
                    self.scrub.refresh_thumb(s);
                }
            }
            Message::SetSpeed(factor) => {
                match self.active.as_ref() {
                    Some(ActiveSession::Replay(s)) => s.set_speed(factor),
                    Some(ActiveSession::SinglePlayer(s)) => s.set_speed(factor),
                    Some(ActiveSession::PvP(_)) => {
                        // PvP runs at fixed EXPECTED_FPS so both sides
                        // stay in sync — no speed control.
                    }
                    None => {}
                }
            }
            Message::ToggleInputDisplay => {
                // Config-owned flag; the App wrapper flips + persists it
                // before this dispatch. The view reads it from config.
            }
            Message::TogglePip => {
                if let Some(s) = self.active.as_ref().and_then(ActiveSession::as_replay) {
                    s.toggle_pip();
                }
            }
            Message::SetFrameDelay(d) => {
                // Purely local frame delay — apply straight to the running
                // PvP session. Config persistence happens in the App's
                // `Message::Session` handler (it owns config).
                if let Some(ActiveSession::PvP(s)) = self.active.as_ref() {
                    s.set_frame_delay(d);
                }
            }
            Message::ToggleMatchSettings => {
                // PvP-only: applied by the signal indicator.
                if let Some(ActiveSession::PvP(_)) = self.active.as_ref() {
                    self.match_settings.toggle();
                }
            }
            Message::EscPressed => {
                // Arm hold-to-quit on the first press of a physical
                // hold only — OS key repeat re-fires EscPressed, and
                // re-arming would push the deadline out forever.
                if self.esc_hold.is_none() {
                    self.esc_hold = Some(std::time::Instant::now());
                }
                // Peel overlays off top-down: the settings modal, then
                // the disconnect confirm, then the match-settings
                // popover. A tap stops there — tearing the session
                // down takes an explicit button action or the full
                // [`ESC_QUIT_HOLD`] hold.
                if self.settings.shown() {
                    self.settings.close();
                } else if self.disconnect.shown() {
                    self.disconnect.close();
                } else if self.match_settings.shown() {
                    self.match_settings.close();
                }
            }
            Message::EscReleased => {
                self.esc_hold = None;
            }
            Message::EscHoldTick => {
                // Nothing here — the hold check lives in `update`'s
                // wrapper so every message runs it; this variant only
                // exists to generate message traffic while held.
            }
            Message::OpenDisconnectConfirm => {
                self.disconnect.open();
            }
            Message::CloseDisconnectConfirm => {
                self.disconnect.close();
            }
            Message::MouseMoved => {
                self.last_mouse_move = std::time::Instant::now();
            }
            Message::ControlsHovered(h) => {
                self.controls_hovered = h;
            }
            Message::NoOp => {}
            Message::ToggleOpponentPanel => {
                self.opponent_panel.toggle();
            }
            Message::ToggleSelfPanel => {
                self.self_panel.toggle();
            }
            Message::OpponentSaveViewAction(action) => {
                if let Some(ActiveSession::PvP(s)) = self.active.as_mut() {
                    let sv_task = s.opponent_save_view.fold(&action);
                    return sv_task.map(Message::OpponentSaveViewAction);
                }
            }
            Message::SelfSaveViewAction(action) => {
                if let Some(ActiveSession::PvP(s)) = self.active.as_mut() {
                    let sv_task = s.local_save_view.fold(&action);
                    return sv_task.map(Message::SelfSaveViewAction);
                }
            }
            Message::OpenSettings => {
                self.settings.open();
            }
            Message::CloseSettings => {
                self.settings.close();
            }
            Message::UpdateFramebuffer => {
                // Telemetry snapshot for the popover sparklines, captured while
                // the session is borrowed below and pushed afterward. `None`
                // (no live PvP match) clears the history so a fresh match — or
                // a return to SP/replay — starts the charts clean.
                let mut sample = None;
                if let Some(session) = self.active.as_ref() {
                    // PvP self-closes when the per-game match-end
                    // hook + peer-end handshake (or grace timeout)
                    // are both satisfied. The end-detection paths
                    // each call `notify_one()` so this branch fires
                    // even after the emu thread has paused.
                    if session.is_ended() {
                        self.close_session();
                    } else {
                        // Upload the native frame as-is; the selected effect
                        // magnifies it on the GPU at draw time.
                        let pixels = self.vbuf.lock().unwrap().clone();
                        self.frame_revision = self.frame_revision.wrapping_add(1);
                        self.current_frame = Some(crate::video::framebuffer::Frame {
                            pixels: std::sync::Arc::new(pixels),
                            width: replay::SCREEN_WIDTH,
                            height: replay::SCREEN_HEIGHT,
                            revision: self.frame_revision,
                            // Neutral placeholder — the view picks the live
                            // effect from config at draw time (see
                            // `framebuffer_view`), so the producer doesn't need
                            // to know the current filter.
                            effect: &crate::video::effects::PASSTHROUGH,
                        });
                        if let ActiveSession::PvP(pvp) = session {
                            sample = Some(MetricSample::capture(pvp));
                        }
                        // Replay PiP: the opponent's screen while the bar
                        // toggle is on.
                        self.pip_frame = session.as_replay().and_then(|r| r.pip_pixels()).map(|pixels| {
                            self.pip_revision = self.pip_revision.wrapping_add(1);
                            crate::video::framebuffer::Frame {
                                pixels: std::sync::Arc::new(pixels),
                                width: replay::SCREEN_WIDTH,
                                height: replay::SCREEN_HEIGHT,
                                revision: self.pip_revision,
                                // The PiP draws at a small fixed size; no
                                // upscale filter, just the plain surface.
                                effect: &crate::video::effects::PASSTHROUGH,
                            }
                        });
                    }
                }
                match sample {
                    Some(s) => {
                        self.metric_history.push_back(s);
                        while self.metric_history.len() > METRIC_HISTORY_LEN {
                            self.metric_history.pop_front();
                        }
                    }
                    None => self.metric_history.clear(),
                }
            }
        }
        iced::Task::none()
    }
}

/// Per-emulator-frame wake stream. Yields
/// [`Message::UpdateFramebuffer`] each time someone fires
/// `notify_one()` on [`State::frame_notify`] — the per-frame
/// callback for new vbuf data, and the PvP end-detection wires
/// (peer-end packet, peer disconnect, grace timeout) for
/// state-transition checks. Always-on across the program's
/// lifetime; parks silently with no active session because
/// nothing fires the notify. Keyboard input still flows through
/// [`crate::input_capture`] — see that module's docs for why the
/// subscription path is too laggy for joypad state.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    let frames = iced::Subscription::run_with(
        FrameTag {
            notify: state.frame_notify.clone(),
        },
        build_frame_stream,
    );
    // The scrub bar's prefetch-progress fill is only repainted on redraw,
    // and a paused (or mid-seek) replay fires no `frame_notify` — so the bar
    // would sit frozen while the background prefetcher races ahead. Tick a
    // redraw at ~20 Hz for the duration of the prefetch so it fills live.
    // Playback already redraws at 60 Hz from the frame callback, hence the
    // `is_paused` gate, and the whole thing switches off once prefetch lands.
    let prefetching = state
        .active
        .as_ref()
        .and_then(ActiveSession::as_replay)
        .is_some_and(|r| r.is_paused() && r.prefetch_progress() < r.total_ticks());
    let mut subs = vec![frames];
    if prefetching {
        subs.push(iced::time::every(std::time::Duration::from_millis(50)).map(|_| Message::UpdateFramebuffer));
    }
    // While Esc is held, tick ~30 Hz so the exit overlay's progress
    // bar fills (and the quit fires) even when the emulator isn't
    // producing frame wakes — a paused replay or a mid-reconnect
    // PvP pause. Live sessions redraw at 60 Hz regardless; the tick
    // is only ever load-bearing on the paused paths, and it stops
    // the moment the key comes back up.
    if state.esc_hold.is_some() {
        subs.push(iced::time::every(std::time::Duration::from_millis(33)).map(|_| Message::EscHoldTick));
    }
    iced::Subscription::batch(subs)
}

/// Stable subscription identity. The hash is a constant string so
/// iced keeps the same stream alive across view rebuilds; the
/// `notify` payload carries the actual wake handle through to
/// [`build_frame_stream`].
struct FrameTag {
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl std::hash::Hash for FrameTag {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        "session-frame".hash(h);
    }
}

fn build_frame_stream(tag: &FrameTag) -> impl futures::Stream<Item = Message> {
    let notify = tag.notify.clone();
    futures::stream::unfold(notify, |notify| async move {
        notify.notified().await;
        Some((Message::UpdateFramebuffer, notify))
    })
}

/// Optional iced texture handle for a Game's background art. Pulls
/// the TGA out of the appropriate BNLC volume's shared `exe.dat` and
/// caches the decoded iced `Handle` per game. `None` whenever Steam
/// / BNLC / the target entry can't be read — caller drops the
/// background widget instead of degrading to a placeholder.
fn background_handle(game: &'static crate::game::Game) -> Option<iced::widget::image::Handle> {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static CACHE: LazyLock<std::sync::Mutex<HashMap<usize, Option<iced::widget::image::Handle>>>> =
        LazyLock::new(Default::default);
    let key = game as *const _ as usize;
    if let Some(cached) = CACHE.lock().unwrap().get(&key).cloned() {
        return cached;
    }
    let bg = game.background;
    let path = format!("exe/data/bg/{}", bg.tga);
    let handle = crate::bnlc::get(bg.volume)
        .and_then(|b| b.read_shared_file(&path))
        .and_then(|bytes| {
            // TGA has no magic prefix, so the image crate's
            // auto-detect refuses to guess it. Pass the format
            // explicitly — every shared-archive background is TGA.
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga)
                .inspect_err(|e| log::warn!("bnlc bg {:?}/{}: decode: {e}", bg.volume, bg.tga))
                .ok()
        })
        .map(|img| {
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw())
        });
    CACHE.lock().unwrap().insert(key, handle.clone());
    handle
}

/// How long the cursor has to sit still before the floating
/// controls slide away.
const CONTROLS_HIDE_AFTER: std::time::Duration = std::time::Duration::from_millis(2500);

/// How long Esc must be held down to quit the active session. The
/// countdown chip appears the moment the hold arms — no grace
/// period; it's a compact HUD chip, not a dim, so an Esc tap just
/// flashes it as feedback that the key registered.
const ESC_QUIT_HOLD: std::time::Duration = std::time::Duration::from_secs(3);

/// Expand an mgba-native BGR555 framebuffer (one little-endian `u16`
/// per pixel — see [`State`]'s `vbuf`) to an RGBA8 image handle for
/// the hover thumbnail, via dataview's shared conversion — the same
/// table that renders ROM sprites/palettes and replay video exports,
/// and the CPU twin of the GPU decode in `video/effects/common.wgsl`.
/// At 240×160 it's cheap, and it only runs when the hovered keyframe
/// changes.
fn thumbnail_handle(framebuffer: &[u8]) -> iced::widget::image::Handle {
    let mut rgba = vec![0u8; framebuffer.len() * 2];
    tango_dataview::rom::bgr555_to_rgba8(framebuffer, &mut rgba);
    iced::widget::image::Handle::from_rgba(replay::SCREEN_WIDTH, replay::SCREEN_HEIGHT, rgba)
}

/// Decode a `.tangoreplay`, resolve both sides' ROM (+ optional
/// patch) from the scanners, and spin up a playback session bound to
/// the shared audio binder. Ready to drop straight into the app's
/// `session` slot.
pub fn build_playback(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    frame_notify: std::sync::Arc<tokio::sync::Notify>,
    vbuf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    path: &std::path::Path,
) -> anyhow::Result<replay::ReplaySession> {
    let f = std::fs::File::open(path)?;
    let replay = std::sync::Arc::new(tango_pvp::replay::Replay::decode(f)?);
    let patches_path = config.patches_path();
    let resolve_rom = |side: Option<&tango_pvp::replay::metadata::Side>| -> anyhow::Result<(
        &'static game::Game,
        std::sync::Arc<Vec<u8>>,
    )> {
        let gi = side
            .and_then(|s| s.game_info.as_ref())
            .ok_or_else(|| anyhow::anyhow!("replay side has no game info"))?;
        let variant = u8::try_from(gi.rom_variant)
            .map_err(|_| anyhow::anyhow!("variant {} out of range", gi.rom_variant))?;
        let entry = crate::game::find_by_family_and_variant(&gi.rom_family, variant)
            .ok_or_else(|| anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, gi.rom_variant))?;
        let g = game::from_gamedb_entry(entry).ok_or_else(|| {
            anyhow::anyhow!("no impl for {}/{}", gi.rom_family, gi.rom_variant)
        })?;
        let rom = scanners
            .roms
            .read()
            .get(&entry)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("rom for {}/{} not scanned", gi.rom_family, gi.rom_variant))?;
        let rom = if let Some(patch_info) = gi.patch.as_ref() {
            let v = semver::Version::parse(&patch_info.version)?;
            patch::apply_patch_from_disk(&rom, entry, &patches_path, &patch_info.name, &v)?
        } else {
            rom
        };
        Ok((g, std::sync::Arc::new(rom)))
    };

    let (local_game, local_rom) = resolve_rom(replay.metadata.local_side.as_ref())?;
    let (remote_game, remote_rom) = resolve_rom(replay.metadata.remote_side.as_ref())?;
    replay::ReplaySession::new(
        local_game,
        local_rom,
        remote_game,
        remote_rom,
        replay,
        audio_binder,
        frame_notify,
        vbuf,
    )
}

/// Build the live PvP session from the netplay handoff data
/// plus the local selection + scanners. Async because
/// PvpSession::new awaits the lobby loop's receiver handoff,
/// and because remote-side rom resolution might apply a patch.
pub async fn spawn_pvp(
    scanners: Scanners,
    config: config::Config,
    audio_binder: audio::LateBinder,
    frame_notify: std::sync::Arc<tokio::sync::Notify>,
    vbuf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    local_game: crate::rom::GameRef,
    local_patch: Option<(String, semver::Version)>,
    pre_match: crate::netplay::PreMatchData,
) -> anyhow::Result<pvp::PvpSession> {
    let local_game_impl =
        game::from_gamedb_entry(local_game).ok_or_else(|| anyhow::anyhow!("no impl for local game"))?;
    let local_rom_raw = scanners
        .roms
        .read()
        .get(&local_game)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("local rom not scanned"))?;
    let local_rom_bytes = if let Some((name, version)) = local_patch.as_ref() {
        patch::apply_patch_from_disk(&local_rom_raw, local_game, &config.patches_path(), name, version)?
    } else {
        local_rom_raw
    };

    // Remote-side game + rom. Falls back to the local game if
    // the remote's GameInfo is missing, but a Compatible verdict
    // would have caught that.
    let remote_gi = pre_match
        .remote_settings
        .game_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("remote settings missing game info"))?;
    let remote_game =
        crate::game::find_by_family_and_variant(&remote_gi.family_and_variant.0, remote_gi.family_and_variant.1)
            .ok_or_else(|| anyhow::anyhow!("unknown remote rom"))?;
    let remote_game_impl =
        game::from_gamedb_entry(remote_game).ok_or_else(|| anyhow::anyhow!("no impl for remote game"))?;
    let remote_rom_raw = scanners
        .roms
        .read()
        .get(&remote_game)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("remote rom not scanned"))?;
    let remote_rom_bytes = if let Some(p) = remote_gi.patch.as_ref() {
        patch::apply_patch_from_disk(
            &remote_rom_raw,
            remote_game,
            &config.patches_path(),
            &p.name,
            &p.version,
        )?
    } else {
        remote_rom_raw
    };

    // Build the opponent's Loaded only if they didn't blind their
    // setup — otherwise we don't have visibility into their save.
    // Loaded parses chip/navi/navicust assets from the rom + wram,
    // so the session pane can render them with the same widgets we
    // use for the local side.
    let opponent_loaded = if !pre_match.remote_settings.blind_setup {
        let remote_save = remote_game
            .parse_save(&pre_match.remote_save_data)
            .map_err(|e| anyhow::anyhow!("parse remote save: {e:?}"))?;
        // `remote_rom_bytes` is already the patched image we run in the
        // session, so resolve the matching `rom_overrides` + charset and
        // hand both straight to `from_patched_rom` — no second BPS apply.
        let applied_patch = remote_gi.patch.as_ref().and_then(|p| {
            let patches = scanners.patches.read();
            let version_meta = patches.get(&p.name)?.versions.get(&p.version).cloned()?;
            Some(crate::selection::AppliedPatch {
                name: p.name.clone(),
                version: p.version.clone(),
                version_meta,
            })
        });
        Some(crate::selection::Loaded::from_patched_rom(
            remote_game,
            remote_rom_bytes.clone(),
            std::path::PathBuf::new(),
            remote_save,
            applied_patch,
        ))
    } else {
        None
    };

    // Build the local-side Loaded so the in-session "my setup"
    // toggle can render the same save-view we use for the
    // opponent panel.
    let local_loaded = {
        let local_save = local_game
            .parse_save(&pre_match.local_save_data)
            .map_err(|e| anyhow::anyhow!("parse local save: {e:?}"))?;
        // Same as the opponent side: `local_rom_bytes` is already
        // patched, so layer the overrides on via `from_patched_rom`
        // instead of re-applying the BPS patch.
        let applied_patch = local_patch.as_ref().and_then(|(name, version)| {
            let patches = scanners.patches.read();
            let version_meta = patches.get(name)?.versions.get(version).cloned()?;
            Some(crate::selection::AppliedPatch {
                name: name.clone(),
                version: version.clone(),
                version_meta,
            })
        });
        Some(crate::selection::Loaded::from_patched_rom(
            local_game,
            local_rom_bytes.clone(),
            std::path::PathBuf::new(),
            local_save,
            applied_patch,
        ))
    };

    pvp::PvpSession::new(
        local_game_impl,
        std::sync::Arc::new(local_rom_bytes),
        remote_game_impl,
        std::sync::Arc::new(remote_rom_bytes),
        pre_match,
        // Presentation delay is purely local — read straight from config (clamped
        // to the supported range), not negotiated with the peer.
        config
            .frame_delay
            .clamp(tango_pvp::battle::MIN_FRAME_DELAY, tango_pvp::battle::MAX_FRAME_DELAY),
        config.disable_bgm_in_pvp,
        &config.replays_path(),
        &audio_binder,
        opponent_loaded,
        local_loaded,
        frame_notify,
        vbuf,
    )
    .await
}

/// Boot the supplied selection in single-player mode. Caller must
/// already have a complete (game + rom + save) Loaded — there's no
/// fallback for missing pieces, so the Play button is responsible for
/// gating.
pub fn spawn_singleplayer(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    frame_notify: std::sync::Arc<tokio::sync::Notify>,
    vbuf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    loaded: &selection::Loaded,
) -> anyhow::Result<singleplayer::SinglePlayerSession> {
    let game = game::from_gamedb_entry(loaded.game)
        .ok_or_else(|| anyhow::anyhow!("no game impl for {:?}", loaded.game.family_and_variant()))?;
    // Loaded stashes the *parsed* ROM (assets), not the raw bytes —
    // grab them back from the scanner and re-apply the patch if any so
    // the emulator sees the same image it would in the legacy app.
    let raw = scanners
        .roms
        .read()
        .get(&loaded.game)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("rom not in scanner cache"))?;
    let rom_bytes = if let Some(p) = loaded.patch.as_ref() {
        patch::apply_patch_from_disk(&raw, loaded.game, &config.patches_path(), &p.name, &p.version)?
    } else {
        raw
    };
    singleplayer::SinglePlayerSession::new(
        game,
        std::sync::Arc::new(rom_bytes),
        &loaded.save_path,
        audio_binder,
        frame_notify,
        vbuf,
    )
}

/// Convert a tick count (60 Hz GBA frames) into `m:ss` for the scrub
/// bar's wallclock labels.
pub fn format_tick(tick: u32) -> String {
    let total_s = tick / 60;
    let m = total_s / 60;
    let s = total_s % 60;
    format!("{m}:{s:02}")
}
