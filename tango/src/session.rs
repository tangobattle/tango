//! Live emulator-session machinery: state struct, per-session
//! Message + update + view + subscription. Owned by App as
//! `session: session::State` and routed via `Message::Session(_)`.
//!
//! The Play / Replays tabs are responsible for STARTING sessions
//! (they construct an ActiveSession via [`build_playback`] /
//! [`spawn_singleplayer`] and stuff it into `state.active`); this
//! module handles everything that happens after.

use crate::app::{Scanners, TEXT_BODY, TEXT_CAPTION};
use crate::audio;
use crate::config;
use crate::game;
use crate::i18n::t;
use crate::patch;
use crate::pvp_session;
use crate::replay_session;
use crate::save_view;
use crate::scrubber;
use crate::selection;
use crate::singleplayer_session;
use crate::video::framebuffer::Effect;
use crate::widgets;
use iced::widget::canvas::{self, Canvas, Frame, LineCap, Path, Stroke};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{container, stack, text};
use iced::{mouse, Alignment, Color, Element, Fill, Length, Point, Rectangle, Renderer, Theme};
use lucide_icons::Icon;
use sweeten::widget::{button, column, mouse_area, row};
use unic_langid::LanguageIdentifier;

/// At most one of these can be active at a time: replay playback, or
/// single-player. The two variants share enough surface (vbuf,
/// close-request) that the view + tick loop wrap them uniformly.
pub enum ActiveSession {
    Replay(replay_session::ReplaySession),
    SinglePlayer(singleplayer_session::SinglePlayerSession),
    PvP(pvp_session::PvpSession),
}

impl ActiveSession {
    pub fn request_close(&self) {
        match self {
            Self::Replay(s) => s.request_close(),
            Self::SinglePlayer(s) => s.request_close(),
            Self::PvP(s) => s.request_close(),
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

    pub fn as_replay(&self) -> Option<&replay_session::ReplaySession> {
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

/// Per-session UI state. App holds `session: State`; the Play and
/// Replays tabs swap an `ActiveSession` into `active` to start a
/// session, then [`State::update`] handles the rest until [`Close`]
/// clears it.
/// One per-frame snapshot of the live PvP telemetry, retained in a short ring
/// buffer ([`State::metric_history`]) so the match-settings popover can draw a
/// sparkline per metric. `round` is `None` between rounds, when no skew/depth
/// reading exists.
#[derive(Clone, Copy)]
pub struct MetricSample {
    pub tps: f32,
    pub fps_target: f32,
    pub ping_ms: u128,
    pub round: Option<(i32, u32)>,
}

impl MetricSample {
    /// Read the current telemetry off a live PvP session. Called once per
    /// emulator frame from the [`Message::UpdateFramebuffer`] handler.
    fn capture(pvp: &pvp_session::PvpSession) -> Self {
        Self {
            tps: pvp.tps(),
            fps_target: pvp.fps_target(),
            ping_ms: pvp.latency().map_or(0, |d| d.as_millis()),
            round: pvp.round_stats().map(|s| (s.skew, s.depth)),
        }
    }
}

/// How many frames of telemetry the sparklines retain (~3 s at 60 fps).
const METRIC_HISTORY_LEN: usize = 180;

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
    /// PvP-only: shows the opponent's save view in a side panel
    /// when they enabled reveal-setup. Defaults to visible when
    /// the panel is available; user can hide it via the toggle
    /// button in the header.
    pub show_opponent_panel: bool,
    /// PvP-only: shows the local player's save view in a side
    /// panel. Defaults to hidden; user toggles via the
    /// red toolbar button.
    pub show_self_panel: bool,
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
    pub show_settings: bool,
    /// Ellipsis-anchored "more options" popover. The trigger lives
    /// in every session type's controls strip; the contents vary
    /// (replay gets the speed picker + Close; SP gets just
    /// Settings + Close; PvP swaps Close for the red Disconnect
    /// item). Closes when any item is picked, the session is
    /// closed, or the trigger is toggled again.
    pub show_options_menu: bool,
    /// PvP-only: the "are you sure?" modal that gates the
    /// Disconnect item in the options menu. Disconnect tears the
    /// session down mid-match (same as Close), so the confirm
    /// keeps a stray click from costing the user a real game.
    pub show_disconnect_confirm: bool,
    /// PvP-only: the match-settings popover, anchored above the
    /// telemetry plate (instrument panel) and toggled by clicking it.
    /// Holds the live frame-delay control (moved here from the footer).
    /// Mutually exclusive with the options menu.
    pub show_match_settings: bool,
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
    /// Rolling window of PvP telemetry snapshots (newest at the back),
    /// sampled once per frame from the [`Message::UpdateFramebuffer`] handler
    /// and drawn as sparklines in the match-settings popover. Capped at
    /// [`METRIC_HISTORY_LEN`]; cleared whenever the active session is not a
    /// live PvP match.
    pub metric_history: std::collections::VecDeque<MetricSample>,
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
            show_opponent_panel: false,
            show_self_panel: false,
            input_held: crate::input::HeldState::default(),
            speed_up_engaged: false,
            show_settings: false,
            show_options_menu: false,
            show_disconnect_confirm: false,
            show_match_settings: false,
            current_frame: None,
            frame_revision: 0,
            metric_history: std::collections::VecDeque::new(),
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
    /// Raw input event from the keyboard or a gamepad. The
    /// handler updates the held-state set, resolves the user's
    /// Mapping into joyflags, and pushes them to the active
    /// session. Speed-up uses the same mechanism (edge-
    /// detected).
    Input(InputEvent),
    /// Toggle play/pause on a replay session. No-op for single-player.
    TogglePlay,
    /// Drag the scrub bar — fires on every value change. Replay-only.
    Seek(u32),
    /// Set the playback speed factor (1.0 = realtime). Replay-only.
    SetSpeed(f32),
    /// PvP-only: the match-settings frame-delay slider moved. Live-sets this
    /// side's local frame delay on the running session; the App also persists it
    /// to config. No peer coordination — it's purely a local display lag.
    SetFrameDelay(u32),
    /// Open/close the ellipsis-anchored options popover.
    ToggleOptionsMenu,
    /// PvP-only: open/close the match-settings popover anchored on the
    /// telemetry plate (instrument panel). Mutually exclusive with the
    /// options menu.
    ToggleMatchSettings,
    /// User pressed Esc inside a session. Closes whichever overlay is on
    /// top (settings modal, disconnect confirm, match-settings popover,
    /// options popover) if any, otherwise opens the options popover. Routed
    /// here rather than from the InputCapture so the decision sees the
    /// current overlay state.
    EscPressed,
    /// Show the "really disconnect?" modal. PvP-only; picked from
    /// the options menu's Disconnect item, which also dismisses
    /// the popover.
    OpenDisconnectConfirm,
    /// Dismiss the disconnect confirm without disconnecting (the
    /// Cancel button + the modal backdrop both fire this).
    CloseDisconnectConfirm,
    /// Show/hide the opponent's reveal-setup side panel. PvP-only.
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

/// Atomic input event we feed to the mapping resolver. Carries
/// the raw key/button/axis info so the session layer can drive
/// both joyflags and the speed-up edge detector.
#[derive(Debug, Clone)]
pub enum InputEvent {
    Key {
        physical: iced::keyboard::key::Physical,
        pressed: bool,
    },
    Button {
        button: crate::input::GamepadButton,
        pressed: bool,
    },
    Axis {
        axis: crate::input::GamepadAxis,
        value: f32,
    },
    /// Controller dropped — clear all gamepad state so
    /// disconnected buttons don't read as still-held.
    GamepadDisconnected,
}

impl State {
    /// Apply a session message to the state. Returns the iced Task
    /// that should be scheduled (always Task::none today — kept for
    /// API parity with the other tabs).
    pub fn update(&mut self, msg: Message, mapping: &crate::input::Mapping, video_filter: &str) -> iced::Task<Message> {
        match msg {
            Message::Close => {
                if let Some(s) = self.active.as_ref() {
                    s.request_close();
                }
                self.active = None;
                self.current_frame = None;
                self.show_options_menu = false;
                self.show_disconnect_confirm = false;
                self.show_match_settings = false;
            }
            Message::Input(ev) => {
                match ev {
                    InputEvent::Key { physical, pressed } => self.input_held.set_key(physical, pressed),
                    InputEvent::Button { button, pressed } => self.input_held.set_button(button, pressed),
                    InputEvent::Axis { axis, value } => self.input_held.set_axis(axis, value),
                    InputEvent::GamepadDisconnected => self.input_held.clear_gamepad(),
                }
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
            Message::TogglePlay => {
                if let Some(s) = self.active.as_ref().and_then(ActiveSession::as_replay) {
                    // Play at end-of-replay: rewind to start and
                    // play through again. Mirrors the behaviour you
                    // get on any media player — "play" on a finished
                    // track restarts it.
                    let paused = s.is_paused();
                    if paused && s.current_tick() >= s.total_ticks() {
                        s.seek_to(0);
                    }
                    s.set_paused(!paused);
                }
            }
            Message::Seek(target) => {
                if let Some(s) = self.active.as_ref().and_then(ActiveSession::as_replay) {
                    s.seek_to(target);
                }
            }
            Message::SetSpeed(factor) => {
                self.show_options_menu = false;
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
            Message::SetFrameDelay(d) => {
                // Purely local frame delay — apply straight to the running
                // PvP session. Config persistence happens in the App's
                // `Message::Session` handler (it owns config).
                if let Some(ActiveSession::PvP(s)) = self.active.as_ref() {
                    s.set_frame_delay(d);
                }
            }
            Message::ToggleOptionsMenu => {
                self.show_options_menu = !self.show_options_menu;
                // The two popovers share the bottom-right corner; never
                // show both at once.
                self.show_match_settings = false;
            }
            Message::ToggleMatchSettings => {
                // PvP-only: applied by the view's plate button. Toggle the
                // popover and close the options menu so they don't overlap.
                if let Some(ActiveSession::PvP(_)) = self.active.as_ref() {
                    self.show_match_settings = !self.show_match_settings;
                    self.show_options_menu = false;
                }
            }
            Message::EscPressed => {
                // Peel overlays off top-down: the modal first, then the two
                // bottom-right popovers, else fall through to toggling options.
                if self.show_settings {
                    self.show_settings = false;
                } else if self.show_disconnect_confirm {
                    self.show_disconnect_confirm = false;
                } else if self.show_match_settings {
                    self.show_match_settings = false;
                } else {
                    self.show_options_menu = !self.show_options_menu;
                }
            }
            Message::OpenDisconnectConfirm => {
                self.show_options_menu = false;
                self.show_disconnect_confirm = true;
            }
            Message::CloseDisconnectConfirm => {
                self.show_disconnect_confirm = false;
            }
            Message::NoOp => {}
            Message::ToggleOpponentPanel => {
                self.show_opponent_panel = !self.show_opponent_panel;
            }
            Message::ToggleSelfPanel => {
                self.show_self_panel = !self.show_self_panel;
            }
            Message::OpponentSaveViewAction(action) => {
                if let Some(ActiveSession::PvP(s)) = self.active.as_mut() {
                    let sv_task = s.opponent_save_view.apply(&action);
                    return sv_task.map(Message::OpponentSaveViewAction);
                }
            }
            Message::SelfSaveViewAction(action) => {
                if let Some(ActiveSession::PvP(s)) = self.active.as_mut() {
                    let sv_task = s.local_save_view.apply(&action);
                    return sv_task.map(Message::SelfSaveViewAction);
                }
            }
            Message::OpenSettings => {
                self.show_settings = true;
                self.show_options_menu = false;
                self.show_match_settings = false;
            }
            Message::CloseSettings => {
                self.show_settings = false;
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
                        self.active = None;
                        self.current_frame = None;
                        self.show_options_menu = false;
                        self.show_disconnect_confirm = false;
                        self.show_match_settings = false;
                    } else {
                        // Upload the native frame as-is; the selected effect
                        // magnifies it on the GPU at draw time.
                        let pixels = self.vbuf.lock().unwrap().clone();
                        self.frame_revision = self.frame_revision.wrapping_add(1);
                        self.current_frame = Some(crate::video::framebuffer::Frame {
                            pixels: std::sync::Arc::new(pixels),
                            width: replay_session::SCREEN_WIDTH,
                            height: replay_session::SCREEN_HEIGHT,
                            revision: self.frame_revision,
                            effect: crate::video::effects::effect_for(video_filter),
                        });
                        if let ActiveSession::PvP(pvp) = session {
                            sample = Some(MetricSample::capture(pvp));
                        }
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
    iced::Subscription::run_with(
        FrameTag {
            notify: state.frame_notify.clone(),
        },
        build_frame_stream,
    )
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

/// Live frame-delay control: a turtle-icon heading naming it, over the lobby's
/// frame-delay row (slider, fixed-width numeric readout, latency-driven
/// "suggest" wand). Lifting the title into the heading frees the control line so
/// the slider gets lobby-like width even in the compact panel. Frame delay is
/// purely local display lag, so dragging it mid-match takes effect on the next
/// rendered frame with no peer coordination.
fn frame_delay_control<'a>(lang: &'a LanguageIdentifier, pvp: &'a pvp_session::PvpSession) -> Element<'a, Message> {
    let fd = pvp.frame_delay();

    // Heading: turtle glyph + title, both muted — matches the metric-card
    // captions above so the control reads as part of the same panel.
    let heading = row![
        Icon::Turtle.widget().size(TEXT_BODY).style(widgets::muted_text_style),
        text(t!(lang, "settings-netplay-frame-delay"))
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Fill);

    // Slider fills the row; the value + wand take their natural sizes.
    let slider = iced::widget::slider(
        tango_pvp::battle::MIN_FRAME_DELAY..=tango_pvp::battle::MAX_FRAME_DELAY,
        fd,
        Message::SetFrameDelay,
    )
    .width(Length::Fill);

    // "Suggest" button — same legacy formula as the lobby (one-way frames + 1 -
    // 2, clamped to the slider range). Disabled until the first ping reading
    // lands (`latency()` is `Some(ZERO)` until then).
    let suggest_msg = match pvp.latency() {
        Some(latency) if !latency.is_zero() => {
            let one_way_frames = (latency.as_nanos() * 60 / 2 / std::time::Duration::from_secs(1).as_nanos()) as i32;
            let d = (one_way_frames + 1 - 2).clamp(
                tango_pvp::battle::MIN_FRAME_DELAY as i32,
                tango_pvp::battle::MAX_FRAME_DELAY as i32,
            ) as u32;
            Some(Message::SetFrameDelay(d))
        }
        _ => None,
    };
    let suggest = widgets::icon_button_maybe(
        Icon::Wand,
        t!(lang, "lobby-frame-delay-suggest"),
        suggest_msg,
        crate::app::STANDARD_PADDING,
    );

    let control = row![
        slider,
        // Live value as a fixed-width monospaced numeral so the slider's
        // position has a readable counterpart that doesn't jiggle layout.
        text(format!("{}", fd))
            .size(TEXT_BODY)
            .font(iced::Font::MONOSPACE)
            .width(Length::Fixed(18.0)),
        suggest,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Fill);

    column![heading, control]
        .spacing(3)
        .width(Length::Fixed(PANEL_W))
        .into()
}

// Panel + sparkline geometry. The cards are all `PANEL_W` wide so the metrics
// line up; the metric value reads in a fixed `VALUE_W` column on the right
// (sized to the widest readout, `NNN ms`) so every number right-aligns and
// every chart ends at the same x, with the chart filling everything to its
// left. The frame-delay control spans the same width: a turtle-icon heading
// over a lobby-style slider row.
const PANEL_W: f32 = 228.0;
const VALUE_W: f32 = 50.0;
const SPARK_H: f32 = 24.0;
// Each metric's full-height value span (sample saturates into it). Chosen to
// line up with the tone thresholds so a point's height roughly tracks its color.
const TPS_SPAN: f32 = 8.0; // fps below target = floor of the chart
const SKEW_SPAN: i32 = 8; // ± about parity; 0 sits mid-height
const DEPTH_SPAN: u32 = 8;
const PING_SPAN: u128 = 200;

/// A compact per-metric history chart for the match-settings panel. Each
/// retained sample is `(height fraction in 0..=1, tone)`, plotted left→right
/// (oldest→newest) as a thin line whose every segment and vertex is colored by
/// that sample's health tone — so the trend tells the same green/amber/red
/// story as the readout, point by point, instead of one flat color for the
/// whole line. `None` slots are gaps (e.g. skew/depth between rounds) and break
/// the line.
struct Sparkline {
    points: Vec<Option<(f32, StatTone)>>,
    /// Whether to wash the area below the trace (down to the chart floor) with a
    /// faint tint of each segment's tone. On for the one-sided metrics (tps,
    /// depth, ping); off for skew, which is bidirectional about its midline.
    fill_under: bool,
    /// Height fraction (0 = bottom, 1 = top) of a reference line to draw, or
    /// `None` for no line. Parity (mid-height) for skew, the value-0 floor for
    /// depth/ping — and `None` for tps, whose displayed floor is `target − 8`,
    /// not 0, so a "zero" line there would mislead.
    zero: Option<f32>,
}

impl Sparkline {
    fn view<'a>(self) -> Element<'a, Message> {
        // Fill the card's chart area; height is fixed so the row lays out cleanly.
        Canvas::new(self).width(Length::Fill).height(Length::Fixed(SPARK_H)).into()
    }
}

impl canvas::Program<Message> for Sparkline {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let text_color = theme.palette().text;
        let n = self.points.len();
        let w = bounds.width;
        let h = bounds.height;
        // Inset vertically so points at the extremes (yf 0 or 1) keep the line
        // width fully on-canvas instead of clipping at the edge.
        const PAD: f32 = 2.0;
        let y_at = |yf: f32| PAD + (1.0 - yf.clamp(0.0, 1.0)) * (h - 2.0 * PAD);

        // Recessed background so the chart area reads as its own inset panel.
        let bg = Path::rounded_rectangle(Point::new(0.0, 0.0), bounds.size(), 3.0.into());
        frame.fill(
            &bg,
            Color {
                a: if palette.is_dark { 0.10 } else { 0.05 },
                ..text_color
            },
        );

        // Fixed rolling window: samples sit a fixed pixel step apart with the
        // newest pinned to the right edge, so the trace scrolls in from the
        // right at full scale instead of stretching to fill while the buffer is
        // still filling up.
        let dx = w / (METRIC_HISTORY_LEN.saturating_sub(1).max(1) as f32);
        let x_at = |i: usize| w - (n.saturating_sub(1) - i) as f32 * dx;

        // Tone wash below the trace, down to the chart floor, per segment.
        if self.fill_under {
            let base = y_at(0.0);
            for i in 0..n.saturating_sub(1) {
                if let (Some((y0, _)), Some((y1, tone))) = (self.points[i], self.points[i + 1]) {
                    let (x0, x1) = (x_at(i), x_at(i + 1));
                    let area = Path::new(|p| {
                        p.move_to(Point::new(x0, y_at(y0)));
                        p.line_to(Point::new(x1, y_at(y1)));
                        p.line_to(Point::new(x1, base));
                        p.line_to(Point::new(x0, base));
                        p.close();
                    });
                    frame.fill(&area, Color { a: 0.3, ..stat_tone_color(theme, tone) });
                }
            }
        }

        // Reference line where one is meaningful (parity for skew, the value-0
        // floor for depth/ping). Drawn over the fill so it stays visible, under
        // the trace.
        if let Some(z) = self.zero {
            let zero_y = y_at(z);
            frame.stroke(
                &Path::line(Point::new(0.0, zero_y), Point::new(w, zero_y)),
                Stroke::default().with_color(Color { a: 0.22, ..text_color }).with_width(1.0),
            );
        }

        // The trace itself: one hairline segment per adjacent pair of samples,
        // each colored by the newer endpoint's tone, breaking across `None`
        // gaps. No vertices/dots — the connected segments are the whole chart.
        for i in 0..n.saturating_sub(1) {
            if let (Some((y0, _)), Some((y1, tone))) = (self.points[i], self.points[i + 1]) {
                let seg = Path::line(Point::new(x_at(i), y_at(y0)), Point::new(x_at(i + 1), y_at(y1)));
                frame.stroke(
                    &seg,
                    Stroke::default()
                        .with_color(stat_tone_color(theme, tone))
                        .with_width(1.0)
                        .with_line_cap(LineCap::Round),
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

/// One telemetry card: `icon caption` on top, `control value` below — the shape
/// shared by every metric (control = sparkline) and the frame-delay knob
/// (control = slider). Icon + caption ride muted; `control` fills the row while
/// `value` sits right-aligned in a fixed [`VALUE_W`] column, so every readout
/// lines up and every chart ends at the same x. Fixed at [`PANEL_W`] so the
/// cards align with one another.
fn telemetry_card<'a>(
    icon: Icon,
    caption: String,
    control: Element<'a, Message>,
    value: Element<'a, Message>,
) -> Element<'a, Message> {
    let caption_row = row![
        icon.widget().size(TEXT_BODY).style(widgets::muted_text_style),
        text(caption).size(TEXT_CAPTION).style(widgets::muted_text_style),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Fill);
    let value_row = row![
        control,
        container(value)
            .width(Length::Fixed(VALUE_W))
            .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Fill);
    column![caption_row, value_row]
        .spacing(3)
        .width(Length::Fixed(PANEL_W))
        .into()
}

/// A right-aligned monospace value readout, tinted by `tone` (or default text
/// when `None`, e.g. the frame-delay number).
fn value_text<'a>(s: String, tone: Option<StatTone>) -> Element<'a, Message> {
    text(s)
        .size(TEXT_BODY)
        .font(iced::Font::MONOSPACE)
        .style(move |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(tone.map_or_else(|| theme.palette().text, |t| stat_tone_color(theme, t))),
        })
        .into()
}

/// TPS readout: current rate over its live cap, stacked to stay narrow. The
/// current rate carries the health tone; the cap rides muted underneath.
fn tps_value<'a>(tps: f32, fps_target: f32, tone: StatTone) -> Element<'a, Message> {
    use iced::widget::text::LineHeight;
    column![
        text(format!("{:.2}", tps))
            .size(TEXT_BODY)
            .font(iced::Font::MONOSPACE)
            .line_height(LineHeight::Relative(1.0))
            .style(move |theme: &iced::Theme| iced::widget::text::Style {
                color: Some(stat_tone_color(theme, tone)),
            }),
        text(format!("{:.2}", fps_target))
            .size(TEXT_CAPTION)
            .font(iced::Font::MONOSPACE)
            .line_height(LineHeight::Relative(1.0))
            .style(widgets::muted_text_style),
    ]
    .spacing(2)
    .align_x(Alignment::End)
    .into()
}

/// One metric card: build its sparkline series by mapping every retained sample
/// through `point` (returning `None` for slots with no reading, which become
/// gaps), and read the current value off the newest sample via `value` (showing
/// `—` muted when there's nothing yet, e.g. skew/depth between rounds).
fn metric_card<'a>(
    icon: Icon,
    caption: String,
    fill_under: bool,
    zero: Option<f32>,
    history: &std::collections::VecDeque<MetricSample>,
    point: impl Fn(&MetricSample) -> Option<(f32, StatTone)>,
    value: impl Fn(&MetricSample) -> Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let points = history.iter().map(&point).collect();
    let value = history
        .back()
        .and_then(value)
        .unwrap_or_else(|| value_text("—".to_string(), Some(StatTone::Muted)));
    telemetry_card(icon, caption, Sparkline { points, fill_under, zero }.view(), value)
}

/// Contents of the match-settings panel: a sparkline card per live metric
/// (TPS, skew, depth, ping) stacked above the frame-delay card. Each chart
/// reads its window from `history` and its current value from the newest
/// sample.
fn match_settings_content<'a>(
    lang: &'a LanguageIdentifier,
    pvp: &'a pvp_session::PvpSession,
    history: &std::collections::VecDeque<MetricSample>,
) -> Element<'a, Message> {
    // `zero` is the reference line: parity (mid-height) for skew, the value-0
    // floor for depth/ping, and `None` for tps (its floor is `target − 8`, so a
    // "zero" line there would mislead).
    let tps_card = metric_card(
        Icon::Gauge,
        t!(lang, "session-stat-tps"),
        true,
        None,
        history,
        |s| {
            (s.fps_target > 0.0).then(|| {
                let yf = (s.tps - (s.fps_target - TPS_SPAN)) / TPS_SPAN;
                (yf.clamp(0.0, 1.0), tone_for_tps(s.tps, s.fps_target))
            })
        },
        |s| (s.fps_target > 0.0).then(|| tps_value(s.tps, s.fps_target, tone_for_tps(s.tps, s.fps_target))),
    );

    let skew_card = metric_card(
        Icon::ArrowLeftRight,
        t!(lang, "session-stat-skew"),
        false,
        Some(0.5),
        history,
        |s| {
            s.round.map(|(skew, _)| {
                let yf = (skew.clamp(-SKEW_SPAN, SKEW_SPAN) as f32 + SKEW_SPAN as f32) / (2.0 * SKEW_SPAN as f32);
                (yf, tone_for_skew(skew))
            })
        },
        |s| s.round.map(|(skew, _)| value_text(fmt_skew(skew), Some(tone_for_skew(skew)))),
    );

    let depth_card = metric_card(
        Icon::Layers2,
        t!(lang, "session-stat-depth"),
        true,
        Some(0.0),
        history,
        |s| s.round.map(|(_, depth)| (depth.min(DEPTH_SPAN) as f32 / DEPTH_SPAN as f32, tone_for_depth(depth))),
        |s| s.round.map(|(_, depth)| value_text(fmt_depth(depth), Some(tone_for_depth(depth)))),
    );

    let ping_card = metric_card(
        Icon::SignalHigh,
        t!(lang, "session-stat-ping"),
        true,
        Some(0.0),
        history,
        |s| Some((s.ping_ms.min(PING_SPAN) as f32 / PING_SPAN as f32, tone_for_ping(s.ping_ms))),
        |s| Some(value_text(fmt_ping(s.ping_ms), Some(tone_for_ping(s.ping_ms)))),
    );

    // Faint rule separating the read-only metrics from the frame-delay knob.
    let rule = container(iced::widget::Space::new().width(Fill).height(Length::Fixed(1.0))).style(
        |theme: &iced::Theme| {
            let p = theme.extended_palette();
            iced::widget::container::Style {
                background: Some(iced::Background::Color(Color {
                    a: if p.is_dark { 0.16 } else { 0.13 },
                    ..theme.palette().text
                })),
                ..Default::default()
            }
        },
    );

    column![tps_card, skew_card, depth_card, ping_card, rule, frame_delay_control(lang, pvp)]
        .spacing(8)
        .width(Length::Fixed(PANEL_W))
        .into()
}

/// Semantic tone for a PvP telemetry value. The icon always rides
/// muted; only the value picks up `Good`/`Warn`/`Bad` so color reads
/// as "this number means something is healthy / borderline / wrong"
/// rather than mere decoration.
#[derive(Clone, Copy)]
enum StatTone {
    Muted,
    Good,
    Warn,
    Bad,
}

fn stat_tone_color(theme: &iced::Theme, tone: StatTone) -> iced::Color {
    match tone {
        StatTone::Muted => widgets::muted_color(theme),
        StatTone::Good => theme.extended_palette().success.strong.color,
        // Amber lives outside iced's default palette, so hardcode a
        // tone that reads on both the dark navy and light parchment
        // HUD plates.
        StatTone::Warn => iced::Color::from_rgb(0.92, 0.67, 0.18),
        StatTone::Bad => theme.extended_palette().danger.strong.color,
    }
}

// Health tone per metric. Shared by the instrument-panel cells and the
// popover sparklines so the value readout and the chart points always agree
// on green/amber/red.

/// TPS vs the live fps target: green at/near rate, amber as it dips, red when
/// it falls well behind (visible netplay stutter). Muted before a target exists.
fn tone_for_tps(tps: f32, fps_target: f32) -> StatTone {
    if fps_target <= 0.0 {
        StatTone::Muted
    } else if tps >= fps_target - 1.0 {
        StatTone::Good
    } else if tps >= fps_target - 5.0 {
        StatTone::Warn
    } else {
        StatTone::Bad
    }
}

/// Clock skew: green near parity, amber drifting, red far out, by `|skew|`.
fn tone_for_skew(skew: i32) -> StatTone {
    match skew.unsigned_abs() {
        0..=3 => StatTone::Good,
        4..=7 => StatTone::Warn,
        _ => StatTone::Bad,
    }
}

/// Rollback depth: green shallow, amber climbing, red when speculation runs deep.
fn tone_for_depth(depth: u32) -> StatTone {
    match depth {
        0..=2 => StatTone::Good,
        3..=5 => StatTone::Warn,
        _ => StatTone::Bad,
    }
}

/// Latency band: green under 80 ms, amber under 140 ms, red beyond.
fn tone_for_ping(ping_ms: u128) -> StatTone {
    if ping_ms < 80 {
        StatTone::Good
    } else if ping_ms < 140 {
        StatTone::Warn
    } else {
        StatTone::Bad
    }
}

// Value formatting, shared by the instrument-panel cells and the popover
// sparkline readouts so the two always render a metric the same way.

/// Current tps over the live cap, both to two decimals (e.g. `60.00/60.00`).
fn fmt_tps(tps: f32, fps_target: f32) -> String {
    format!("{:.2}/{:.2}", tps, fps_target)
}
/// Signed skew in a 3-wide field; bare `0` at parity reads calmer than `+0`.
fn fmt_skew(skew: i32) -> String {
    if skew == 0 {
        "  0".to_string()
    } else {
        format!("{:>+3}", skew)
    }
}
/// Rollback depth, right-aligned in a 2-wide field.
fn fmt_depth(depth: u32) -> String {
    format!("{:>2}", depth)
}
/// Latency in ms, right-aligned in a 3-wide field.
fn fmt_ping(ping_ms: u128) -> String {
    format!("{:>3} ms", ping_ms)
}

/// One telemetry cell: a label `icon` and the current `value`, both
/// color-coded by the health `tone`. The full metric name lives in the
/// match-settings panel's captions, so the cell carries no hover tooltip.
fn stat_cell<'a>(icon: Icon, tone: StatTone, value: String) -> Element<'a, Message> {
    // Only the value carries the health tint; the icon always rides muted so
    // color reads as "this number means something", not decoration.
    let tone_style = move |theme: &iced::Theme| iced::widget::text::Style {
        color: Some(stat_tone_color(theme, tone)),
    };
    row![
        icon.widget().size(TEXT_BODY).style(widgets::muted_text_style),
        text(value)
            .size(TEXT_BODY)
            .font(iced::Font::MONOSPACE)
            .style(tone_style),
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .into()
}

/// P1/P2 identity tag leading the instrument cluster. Plain
/// monospaced text in the default color — the label tells you which
/// side you are; it's not a metric, so it carries no tint.
fn player_cell<'a>(player_index: u8) -> Element<'a, Message> {
    let label = if player_index == 0 { "P1" } else { "P2" };
    text(label).size(TEXT_BODY).font(iced::Font::MONOSPACE).into()
}

/// Hairline rule separating cells inside the telemetry deck.
fn stat_divider<'a>() -> Element<'a, Message> {
    container(
        iced::widget::Space::new()
            .width(Length::Fixed(1.0))
            .height(Length::Fixed(15.0)),
    )
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        let text = theme.palette().text;
        iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color {
                a: if p.is_dark { 0.16 } else { 0.13 },
                ..text
            })),
            ..Default::default()
        }
    })
    .into()
}

/// Flat plate behind the telemetry deck — a faint fill + hairline
/// border so the readout reads as one grouped module without drawing
/// attention to itself. Realized as a button style (not a static
/// container) because the instrument panel is clickable: a subtle
/// hover/press brighten marks it as the trigger for the match-settings
/// popover. PvP-only.
fn telemetry_plate_button(theme: &iced::Theme, status: sweeten::widget::button::Status) -> sweeten::widget::button::Style {
    use sweeten::widget::button::Status;
    let p = theme.extended_palette();
    let text = theme.palette().text;
    let base = if p.is_dark { 0.06 } else { 0.05 };
    let fill = match status {
        Status::Hovered => base + 0.06,
        Status::Pressed => base + 0.10,
        _ => base,
    };
    sweeten::widget::button::Style {
        background: Some(iced::Background::Color(iced::Color { a: fill, ..text })),
        text_color: text,
        border: iced::Border {
            radius: 6.0.into(),
            width: 1.0,
            color: iced::Color {
                a: if p.is_dark { 0.10 } else { 0.08 },
                ..text
            },
        },
        ..Default::default()
    }
}

/// Render the active session — framebuffer, header, and (for replays
/// only) the transport row with play/pause + scrubber + prefetch %.
/// Pass the App's `session: State` borrow.
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
    fractional_scaling: bool,
    hide_emulator_border: bool,
    effect: &'static Effect,
) -> Element<'a, Message> {
    let Some(session) = state.active.as_ref() else {
        return iced::widget::Space::new().width(Fill).height(Fill).into();
    };

    // Post-filter framebuffer dimensions. Drive the scale math below;
    // match the (w, h) `build_frame_pixels` stamps into the frame the
    // `framebuffer` shader uploads.
    // The widget is sized to native·scale — the same rectangle the old CPU
    // upscalers produced — and the effect's fragment shader magnifies the
    // native texture to fill it.
    let scale = effect.scale;
    let img_w = (replay_session::SCREEN_WIDTH * scale) as f32;
    let img_h = (replay_session::SCREEN_HEIGHT * scale) as f32;

    // The live framebuffer renders through a custom wgpu shader widget
    // (one persistent GPU texture, written in place each vblank) instead
    // of a per-frame `image` handle. The shader fills the widget's bounds,
    // so we size the widget to the framebuffer rect here — an exact
    // integer multiple (crisp, the default) or a smooth aspect-fit — using
    // `responsive` for the pane size both need. Before the first frame, a
    // 1×1 black placeholder keeps the pane opaque.
    let frame: Element<'a, Message> = iced::widget::responsive(move |size| {
        let raw = (size.width / img_w).min(size.height / img_h);
        let scale = if fractional_scaling {
            raw.max(0.0)
        } else {
            raw.floor().max(1.0)
        };
        let (w, h) = (img_w * scale, img_h * scale);

        let frame = state
            .current_frame
            .clone()
            .unwrap_or_else(crate::video::framebuffer::Frame::black);
        let fb = iced::widget::shader::Shader::new(crate::video::framebuffer::Program::new(frame))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h));

        let centered = |content: Element<'a, Message>| -> Element<'a, Message> {
            iced::widget::container(content)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into()
        };

        if fractional_scaling {
            // Smooth aspect-fit, centered, no drop shadow.
            centered(fb.into())
        } else {
            // Tight container around the Fixed-size framebuffer so the
            // shadow style traces its edges, not the surrounding pane.
            let framed = iced::widget::container(fb)
                .width(Length::Fixed(w))
                .height(Length::Fixed(h))
                .style(|_theme: &iced::Theme| iced::widget::container::Style {
                    shadow: iced::Shadow {
                        color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55),
                        offset: iced::Vector::new(0.0, 8.0),
                        blur_radius: 24.0,
                    },
                    ..Default::default()
                });
            centered(framed.into())
        }
    })
    .into();

    // Controls-strip sizing: one icon size + padding so the
    // play/pause, settings, close, opponent-toggle buttons all
    // sit at the same height as the scrubber + speed picker.
    // Matches the play-tab bottom bar so the chrome reads as
    // family across screens.
    const CTRL_ICON: f32 = 16.0;
    const CTRL_PAD: [f32; 2] = [10.0, 14.0];

    let ctrl_icon_btn_maybe =
        |icon: Icon,
         label: String,
         msg: Option<Message>,
         style: fn(&iced::Theme, sweeten::widget::button::Status) -> sweeten::widget::button::Style|
         -> Element<'a, Message> {
            let mut btn = button(icon.widget().size(CTRL_ICON))
                .padding(CTRL_PAD)
                .height(iced::Length::Fixed(crate::app::BAR_CONTROL_HEIGHT))
                .style(style);
            if let Some(m) = msg {
                btn = btn.on_press(m);
            }
            iced::widget::tooltip(
                btn,
                iced::widget::container(text(label).size(TEXT_CAPTION))
                    .padding(6)
                    .style(|theme: &iced::Theme| {
                        let p = theme.extended_palette();
                        iced::widget::container::Style {
                            background: Some(iced::Background::Color(p.background.strong.color)),
                            text_color: Some(p.background.strong.text),
                            border: iced::Border {
                                radius: 4.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }
                    }),
                iced::widget::tooltip::Position::Top,
            )
            .gap(4)
            .into()
        };
    let ctrl_icon_btn_styled =
        |icon: Icon,
         label: String,
         msg: Message,
         style: fn(&iced::Theme, sweeten::widget::button::Status) -> sweeten::widget::button::Style|
         -> Element<'a, Message> { ctrl_icon_btn_maybe(icon, label, Some(msg), style) };
    let ctrl_icon_btn = |icon: Icon, label: String, msg: Message| -> Element<'a, Message> {
        ctrl_icon_btn_styled(icon, label, msg, widgets::neutral)
    };

    // PvP-only: red "show my setup" toggle (left of the controls
    // strip) and blue "show opponent's setup" toggle (right).
    // Color-coded like the matchup-pane diagonal split — red = P1,
    // blue = P2. Opponent toggle is always rendered for PvP; it's
    // disabled when the peer didn't enable reveal-setup.
    let self_toggle: Option<Element<'a, Message>> = match session {
        ActiveSession::PvP(s) if s.local_loaded.is_some() => {
            let style: fn(&iced::Theme, sweeten::widget::button::Status) -> sweeten::widget::button::Style =
                if state.show_self_panel {
                    widgets::pvp_red_button
                } else {
                    widgets::neutral
                };
            Some(ctrl_icon_btn_styled(
                Icon::FileUser,
                t!(lang, "session-self"),
                Message::ToggleSelfPanel,
                style,
            ))
        }
        _ => None,
    };
    let opponent_toggle: Option<Element<'a, Message>> = match session {
        ActiveSession::PvP(s) => {
            let revealed = s.opponent_loaded.is_some();
            let style: fn(&iced::Theme, sweeten::widget::button::Status) -> sweeten::widget::button::Style =
                if state.show_opponent_panel && revealed {
                    widgets::pvp_blue_button
                } else {
                    widgets::neutral
                };
            let msg = if revealed {
                Some(Message::ToggleOpponentPanel)
            } else {
                None
            };
            Some(ctrl_icon_btn_maybe(
                Icon::FileUser,
                t!(lang, "session-opponent"),
                msg,
                style,
            ))
        }
        _ => None,
    };
    // "More options" trigger for the unified popover. Ellipsis
    // rather than a cogwheel so it doesn't visually duplicate the
    // Settings item INSIDE the popover (which uses the cogwheel).
    // Same widget across all session types — Replay puts a speed
    // picker in the popover body, SP/PvP don't; all three surface
    // Settings + a tear-down item (Close for SP/Replay, red
    // Disconnect for PvP).
    let options_btn = ctrl_icon_btn(Icon::Ellipsis, t!(lang, "playback-options"), Message::ToggleOptionsMenu);

    let mut layout = column![].spacing(0).width(Fill).height(Fill);

    // Body: framebuffer + optional setup panes layered over the
    // game's BNLC background art (cover-fit, crops as needed) or
    // a pure-black backdrop when BNLC isn't installed. The
    // backdrop spans the full body width so the setup panes
    // float on top of the same bezel art.
    let frame_container = container(frame).center(Fill);
    let bnlc_bg = if hide_emulator_border {
        None
    } else {
        background_handle(session.local_game())
    };
    let backdrop: Element<'a, Message> = match bnlc_bg {
        Some(bg_handle) => iced::widget::image(bg_handle)
            .width(Fill)
            .height(Fill)
            .content_fit(iced::ContentFit::Cover)
            .into(),
        None => container(iced::widget::Space::new().width(Fill).height(Fill))
            .style(|_: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(iced::Color::BLACK)),
                ..Default::default()
            })
            .into(),
    };

    // Optional left/right setup panes for PvP. Each occupies a
    // fixed width when shown; the emulator fills the rest of the
    // row. The panes ride on top of the backdrop layer so the
    // BNLC bezel art shows around their outer margins. Only the
    // panes carry padding — the emulator itself still extends to
    // the screen edges.
    const SETUP_PANE_WIDTH: f32 = 420.0;
    let mut content_row = row![].spacing(0).height(Fill).width(Fill);
    if let ActiveSession::PvP(s) = session {
        if state.show_self_panel && s.local_loaded.is_some() {
            let me = s.local_loaded.as_ref().unwrap();
            let panel = save_view::view(lang, me, &s.local_save_view, true, None, false, false)
                .map(Message::SelfSaveViewAction);
            let pane = container(panel)
                .width(iced::Length::Fixed(SETUP_PANE_WIDTH))
                .height(Fill)
                .padding(widgets::PANE_PADDING)
                .style(widgets::panel);
            content_row = content_row.push(container(pane).height(Fill).padding(widgets::PANE_PADDING));
        }
    }
    content_row = content_row.push(container(frame_container).width(Fill).height(Fill));
    if let ActiveSession::PvP(s) = session {
        if state.show_opponent_panel && s.opponent_loaded.is_some() {
            let opponent = s.opponent_loaded.as_ref().unwrap();
            let panel = save_view::view(lang, opponent, &s.opponent_save_view, true, None, false, false)
                .map(Message::OpponentSaveViewAction);
            let pane = container(panel)
                .width(iced::Length::Fixed(SETUP_PANE_WIDTH))
                .height(Fill)
                .padding(widgets::PANE_PADDING)
                .style(widgets::panel);
            content_row = content_row.push(container(pane).height(Fill).padding(widgets::PANE_PADDING));
        }
    }
    let emu_pane: Element<'a, Message> = container(stack![backdrop, Element::from(content_row)])
        .width(Fill)
        .height(Fill)
        .into();
    layout = layout.push(emu_pane);

    // Controls strip. Replay sessions get the full transport
    // (play/pause + scrubber + speed); single-player + PvP get a
    // thin strip with just the opponent-panel toggle (PvP only)
    // and the close button. Either way the close lives here so
    // there's no separate header eating vertical space.
    let mut controls = row![].spacing(10).align_y(Alignment::Center).padding([10, 8]);
    if let Some(r) = session.as_replay() {
        let total = r.total_ticks().max(1);
        let cur = r.current_tick().min(total);
        let prefetched = r.prefetch_progress().min(total);
        let (play_pause_icon, play_pause_label, paused) = if r.is_paused() {
            (Icon::Play, t!(lang, "playback-play"), true)
        } else {
            (Icon::Pause, t!(lang, "playback-pause"), false)
        };
        let scrub = scrubber::Scrubber::new(cur, total, prefetched, Message::Seek)
            .round_boundaries(r.round_boundaries())
            .view();

        // Play/Pause is the transport's centerpiece — promote to
        // the primary-button style when paused (the affordance
        // the user is most likely looking for at rest) and keep
        // it neutral while playing. Either way it sits a notch
        // bigger than the other strip controls and is rendered
        // as a perfect circle (square padding + huge radius) so
        // it reads as a console transport button instead of a
        // generic pill.
        let base_style: fn(&iced::Theme, sweeten::widget::button::Status) -> sweeten::widget::button::Style = if paused
        {
            widgets::primary_button
        } else {
            widgets::neutral
        };
        let play_pause_style = move |theme: &iced::Theme, status: sweeten::widget::button::Status| {
            let mut style = base_style(theme, status);
            style.border.radius = 999.0.into();
            style
        };
        // Square button sized to the shared bar-control height
        // so the media bar lines up exactly with the play-tab
        // link bar (both pin their interactive children to the
        // same constant).
        let play_pause_btn = iced::widget::tooltip(
            button(
                iced::widget::container(play_pause_icon.widget().size(18.0))
                    .width(iced::Length::Fixed(20.0))
                    .height(iced::Length::Fixed(20.0))
                    .center(Fill),
            )
            .padding(0)
            .width(iced::Length::Fixed(crate::app::BAR_CONTROL_HEIGHT))
            .height(iced::Length::Fixed(crate::app::BAR_CONTROL_HEIGHT))
            .style(play_pause_style)
            .on_press(Message::TogglePlay),
            iced::widget::container(text(play_pause_label).size(TEXT_CAPTION))
                .padding(6)
                .style(|theme: &iced::Theme| {
                    let p = theme.extended_palette();
                    iced::widget::container::Style {
                        background: Some(iced::Background::Color(p.background.strong.color)),
                        text_color: Some(p.background.strong.text),
                        border: iced::Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            iced::widget::tooltip::Position::Top,
        )
        .gap(4);

        // Tick readouts: monospaced + bumped one tier above caption
        // so they read as digital-clock numerals rather than
        // metadata, primary-tinted so the eye picks them up as
        // playback state.
        let tick_style = |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(theme.palette().primary),
        };
        controls = controls
            .push(play_pause_btn)
            .push(
                text(format_tick(cur))
                    .size(14)
                    .font(iced::Font::MONOSPACE)
                    .style(tick_style),
            )
            .push(scrub)
            .push(
                text(format_tick(total))
                    .size(14)
                    .font(iced::Font::MONOSPACE)
                    .style(widgets::muted_text_style),
            );
    } else {
        // No transport widgets for SP/PvP. Drop the self-setup
        // toggle on the left (PvP-only) so it pairs visually with
        // the right-anchored opponent toggle, then push a spacer
        // so the rest of the strip (metrics, opponent, options)
        // hugs the right edge.
        if let Some(t) = self_toggle {
            controls = controls.push(t);
        }
        // Frame delay used to live here; it now rides in the match-settings
        // popover anchored on the telemetry plate (see below).
        controls = controls.push(horizontal_space());
    }
    // PvP-only telemetry deck: P1/P2 tag, TPS, frame skew, rollback
    // depth, ping — each metric drawn next to its current value,
    // colored by health (green/amber/red), gathered into one
    // hairline-divided plate. P1/P2 leads the cluster as an identity tag.
    // Gate the whole deck on a live latency reading: `latency()` is `Some` while
    // the link is up (even at 0 ms on LAN) and `None` the moment the remote
    // drops — at which point the telemetry is frozen and meaningless, so the
    // panel retires itself.
    if let ActiveSession::PvP(pvp) = session {
        if let Some(latency) = pvp.latency() {
            let stats = pvp.round_stats();
            let ping_ms = latency.as_millis();
            let tps = pvp.tps();
            let fps_target = pvp.fps_target();

            let mut cells: Vec<Element<'a, Message>> = Vec::new();

            // P1/P2 identity tag now leads the instrument cluster, inside
            // the plate — it reads as the "which side am I" label sitting
            // ahead of the live metrics. It's a match-level constant, so it
            // shows whenever the panel is up — including between rounds, when
            // there's no live `RoundStats`.
            cells.push(player_cell(pvp.local_player_index()));

            // TPS: current rate vs target — green at/near rate, amber as it
            // dips, red when it falls well behind (visible netplay stutter).
            cells.push(stat_cell(
                Icon::Gauge,
                tone_for_tps(tps, fps_target),
                fmt_tps(tps, fps_target),
            ));

            if let Some(s) = stats {
                // Skew: how tight the sync is — green near parity, amber
                // drifting, red far out, by |skew| in frames.
                cells.push(stat_cell(Icon::ArrowLeftRight, tone_for_skew(s.skew), fmt_skew(s.skew)));

                // Rollback depth: lower = tighter prediction. Green when
                // shallow, amber as it climbs, red when speculation runs deep.
                cells.push(stat_cell(Icon::Layers2, tone_for_depth(s.depth), fmt_depth(s.depth)));
            }

            // Ping: latency band. The signal icon's bar strength tracks
            // the band too — full bars (SignalHigh) when ping is low,
            // dropping to SignalLow as latency climbs.
            let ping_icon = if ping_ms < 80 {
                Icon::SignalHigh
            } else if ping_ms < 140 {
                Icon::SignalMedium
            } else {
                Icon::SignalLow
            };
            cells.push(stat_cell(ping_icon, tone_for_ping(ping_ms), fmt_ping(ping_ms)));

            // Interleave hairline dividers into one flat plate. The plate is a
            // button: clicking the instrument panel toggles the match-settings
            // popover anchored above it.
            let mut strip = row![].spacing(6).align_y(Alignment::Center);
            for (i, cell) in cells.into_iter().enumerate() {
                if i > 0 {
                    strip = strip.push(stat_divider());
                }
                strip = strip.push(cell);
            }
            controls = controls.push(
                button(strip)
                    .padding([3, 9])
                    .style(telemetry_plate_button)
                    .on_press(Message::ToggleMatchSettings),
            );
        }
    }
    // Opponent setup-reveal toggle (PvP-only) sits to the left of
    // the options trigger so the ellipsis stays the rightmost
    // item — the popover anchors above it.
    if let Some(toggle) = opponent_toggle {
        controls = controls.push(toggle);
    }
    // Options ellipsis is the unified entry point to session-level
    // commands (Settings, replay speed, Close / Disconnect). Always
    // last so the popover lands above a consistent right-edge
    // anchor regardless of session type.
    controls = controls.push(options_btn);
    layout = layout
        .push(widgets::hud_scanline())
        .push(container(controls).width(Fill).style(widgets::hud_bar));

    // Ellipsis-anchored options popover. Built as a top Stack
    // layer anchored above the HUD bar so it floats over the
    // framebuffer without pushing the controls strip up. The
    // menu owns its own dismiss — picking any item closes it;
    // clicking the trigger again toggles it off.
    //
    // Content varies by session type:
    //   Replay → Settings, Speed picker, Close
    //   SP     → Settings, Close
    //   PvP    → Settings, (red) Disconnect
    let options_overlay: Option<Element<'a, Message>> = if state.show_options_menu {
        // Row item width. Wider than the historical 120px speed
        // picker so "Disconnect" + its icon sit on one line without
        // wrapping in any locale.
        const ROW_WIDTH: f32 = 160.0;
        // Total popover width = row + the panel's 6px-each-side
        // padding. Pinned explicitly so a Fill-width child (the
        // divider) can't propagate through the popover container
        // and stretch the menu out to the full bottom-right pane.
        const POPOVER_WIDTH: f32 = ROW_WIDTH + 12.0;
        // Menu-row hover/press/selected tints. Accent drives both
        // the selected-text color and the wash behind hover/press —
        // pass `palette.primary` for normal rows, the Disconnect red
        // for the destructive row. Hover/press alphas are pushed
        // high enough (0.28 / 0.45 on dark) that the red wash on
        // the panel plate actually reads as red, not as a slightly
        // pink-tinted shadow.
        fn menu_row_style(
            theme: &iced::Theme,
            status: sweeten::widget::button::Status,
            selected: bool,
            accent: iced::Color,
        ) -> sweeten::widget::button::Style {
            use sweeten::widget::button::Status;
            let p = theme.extended_palette();
            let text = theme.palette().text;
            let tint = |a: f32| iced::Background::Color(iced::Color { a, ..accent });
            let bg = match status {
                Status::Hovered => Some(tint(if p.is_dark { 0.28 } else { 0.22 })),
                Status::Pressed => Some(tint(if p.is_dark { 0.45 } else { 0.35 })),
                _ if selected => Some(tint(if p.is_dark { 0.14 } else { 0.12 })),
                _ => None,
            };
            sweeten::widget::button::Style {
                background: bg,
                text_color: if selected { accent } else { text },
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }
        // Build a "leading-icon + label" action row. `tint = None`
        // uses the standard primary accent (hover/press wash only,
        // label in normal text color); `Some(color)` swaps both the
        // icon, the resting label color, AND the hover/press wash
        // to the tint — used for Disconnect's danger red so the whole
        // row reads as destructive before the user even hovers.
        let action_row = |icon: Icon, label: String, msg: Message, tint: Option<iced::Color>| -> Element<'a, Message> {
            let tinted_text_style = move |theme: &iced::Theme| iced::widget::text::Style {
                color: Some(tint.unwrap_or_else(|| theme.palette().text)),
            };
            let icon_el: Element<'a, Message> = icon.widget().size(14.0).style(tinted_text_style).into();
            let content = row![icon_el, text(label).size(14).style(tinted_text_style)]
                .spacing(8)
                .align_y(iced::Alignment::Center);
            button(content)
                .padding([6, 10])
                .width(iced::Length::Fixed(ROW_WIDTH))
                .style(move |theme: &iced::Theme, status: sweeten::widget::button::Status| {
                    let accent = tint.unwrap_or(theme.palette().primary);
                    menu_row_style(theme, status, false, accent)
                })
                .on_press(msg)
                .into()
        };
        // Thin divider between sections — text-tinted hairline so
        // it reads as a separator rather than a hard line.
        let divider = || -> Element<'a, Message> {
            container(iced::widget::Space::new().width(Fill).height(iced::Length::Fixed(1.0)))
                .width(Fill)
                .style(|theme: &iced::Theme| {
                    let p = theme.extended_palette();
                    let text = theme.palette().text;
                    iced::widget::container::Style {
                        background: Some(iced::Background::Color(iced::Color {
                            a: if p.is_dark { 0.12 } else { 0.10 },
                            ..text
                        })),
                        ..Default::default()
                    }
                })
                .padding(iced::Padding {
                    top: 0.0,
                    right: 4.0,
                    bottom: 0.0,
                    left: 4.0,
                })
                .into()
        };

        let mut sections: Vec<Element<'a, Message>> = Vec::new();

        // Replay-only: speed picker section, anchored at the top.
        // The Settings + Close pair below sits as plain menu items
        // separated by a divider so Settings reads as a sibling of
        // Close, not a header for the Speed section above.
        if let Some(r) = session.as_replay() {
            let current = r.speed();
            let opts: &[f32] = &[0.5, 1.0, 2.0, 4.0];
            // Section header: gauge icon + "Speed" label, both
            // muted so the header reads as a category divider
            // instead of a clickable row.
            let header_row = row![
                Icon::Gauge.widget().size(TEXT_CAPTION).style(widgets::muted_text_style),
                text(t!(lang, "playback-speed"))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center);
            let header = container(header_row).padding(iced::Padding {
                top: 4.0,
                right: 10.0,
                bottom: 4.0,
                left: 10.0,
            });
            let mut speed_col = column![header].spacing(1);
            for &v in opts {
                let selected = (v - current).abs() < 1e-3;
                let label = if (v - v.trunc()).abs() < 1e-3 {
                    format!("{}×", v as i32)
                } else {
                    format!("{:.1}×", v)
                };
                let check: Element<'a, Message> = if selected {
                    Icon::Check.widget().size(14.0).into()
                } else {
                    iced::widget::Space::new()
                        .width(iced::Length::Fixed(14.0))
                        .height(iced::Length::Fixed(14.0))
                        .into()
                };
                let content = row![check, text(label).size(14)]
                    .spacing(8)
                    .align_y(iced::Alignment::Center);
                let btn = button(content)
                    .padding([6, 10])
                    .width(iced::Length::Fixed(ROW_WIDTH))
                    .style(move |theme: &iced::Theme, status: sweeten::widget::button::Status| {
                        menu_row_style(theme, status, selected, theme.palette().primary)
                    })
                    .on_press(Message::SetSpeed(v));
                speed_col = speed_col.push(btn);
            }
            sections.push(Element::from(speed_col));
            sections.push(divider());
        }

        // Settings menu item. Cogwheel matches the trigger button —
        // both refer to the same destination, so the icon doubles as
        // a reinforcement instead of a new symbol.
        sections.push(action_row(
            Icon::Settings,
            t!(lang, "tab-settings"),
            Message::OpenSettings,
            None,
        ));

        // Tear-down item. PvP gets the red Disconnect confirm gate;
        // SP and Replay get a direct Close (no gate — neither path
        // sacrifices game state on tear-down). Color matches
        // `widgets::pvp_red_button` so the menu's destructive row,
        // the confirm dialog's CTA, and the existing P1 toolbar
        // accent all read as one family. No divider between
        // Settings and the tear-down — they're sibling menu items.
        let tear_down: Element<'a, Message> = match session {
            ActiveSession::PvP(_) => action_row(
                Icon::Unplug,
                t!(lang, "playback-disconnect"),
                Message::OpenDisconnectConfirm,
                Some(iced::Color::from_rgb(0.85, 0.22, 0.28)),
            ),
            _ => action_row(Icon::X, t!(lang, "playback-close"), Message::Close, None),
        };
        sections.push(tear_down);

        let body = column(sections).spacing(2);
        let popover = container(body)
            .padding(6)
            .width(iced::Length::Fixed(POPOVER_WIDTH))
            .style(widgets::panel);
        let lift = crate::app::BAR_CONTROL_HEIGHT + 20.0 + 3.0 + 6.0;
        Some(
            container(popover)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 8.0,
                    bottom: lift,
                    left: 0.0,
                })
                .into(),
        )
    } else {
        None
    };

    // Match-settings popover (PvP-only), anchored above the telemetry
    // plate that triggers it. Currently holds just the live frame-delay
    // control (moved here from the footer), but it's the home for any
    // future in-match knobs. Like the options menu it owns no dismiss
    // backdrop — clicking the plate again or pressing Esc closes it. No
    // heading: the frame-delay row already labels itself.
    let match_settings_overlay: Option<Element<'a, Message>> = match session {
        ActiveSession::PvP(pvp) if state.show_match_settings && pvp.latency().is_some() => {
            let popover = container(match_settings_content(lang, pvp, &state.metric_history))
                .padding(12)
                .style(widgets::panel);
            // Same lift as the options menu so the popover floats just
            // above the HUD bar. Right padding aligns the popover's right
            // edge with the telemetry plate's: controls-container pad (8) +
            // options button + spacing (10) + opponent toggle + spacing
            // (10), where each button is CTRL_PAD·2 + CTRL_ICON ≈ 44 wide.
            let lift = crate::app::BAR_CONTROL_HEIGHT + 20.0 + 3.0 + 6.0;
            const PLATE_RIGHT_OFFSET: f32 = 8.0 + 44.0 + 10.0 + 44.0 + 10.0;
            Some(
                container(popover)
                    .width(Fill)
                    .height(Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Bottom)
                    .padding(iced::Padding {
                        top: 0.0,
                        right: PLATE_RIGHT_OFFSET,
                        bottom: lift,
                        left: 0.0,
                    })
                    .into(),
            )
        }
        _ => None,
    };

    // Disconnect confirmation modal (PvP-only). Centered panel with a
    // dimmed click-to-dismiss backdrop — same shape as app.rs's
    // in-session Settings modal so the two read as the same family
    // of "this interrupts what you're doing" dialogs. Sits above
    // the options popover in the stack so it covers the menu if
    // the user somehow re-opened it.
    let disconnect_overlay: Option<Element<'a, Message>> =
        if state.show_disconnect_confirm && matches!(session, ActiveSession::PvP(_)) {
            let title = text(t!(lang, "playback-disconnect-prompt")).size(TEXT_BODY + 4.0);
            let body_text = text(t!(lang, "playback-disconnect-detail")).style(widgets::muted_text_style);
            let cancel_btn = widgets::labeled_icon_button(
                Icon::X,
                t!(lang, "playback-cancel"),
                Message::CloseDisconnectConfirm,
                [8.0, 14.0],
                widgets::neutral,
            );
            let disconnect_btn = widgets::labeled_icon_button(
                Icon::Unplug,
                t!(lang, "playback-disconnect"),
                Message::Close,
                [8.0, 14.0],
                widgets::danger_button,
            );
            let buttons = row![horizontal_space(), cancel_btn, disconnect_btn]
                .spacing(8)
                .align_y(Alignment::Center);
            let panel = container(column![title, body_text, buttons].spacing(14).width(Fill))
                .width(iced::Length::Fixed(420.0))
                .padding(20)
                .style(widgets::panel);
            // Swallow clicks on the panel's inert regions (title,
            // body) so they don't fall through to the backdrop's
            // dismiss-on-press handler. Buttons inside the panel
            // still capture their own events.
            let panel_swallow = mouse_area(panel).on_press(|_| Message::NoOp);
            let placement = container(panel_swallow)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center);
            let backdrop = mouse_area(
                container(iced::widget::Space::new().width(Fill).height(Fill))
                    .width(Fill)
                    .height(Fill)
                    .style(|_: &iced::Theme| iced::widget::container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55))),
                        ..Default::default()
                    }),
            )
            .on_press(|_| Message::CloseDisconnectConfirm);
            Some(iced::widget::stack![Element::from(backdrop), Element::from(placement)].into())
        } else {
            None
        };

    let mut stacked = stack![Element::from(layout)];
    if let Some(o) = options_overlay {
        stacked = stacked.push(o);
    }
    if let Some(o) = match_settings_overlay {
        stacked = stacked.push(o);
    }
    if let Some(o) = disconnect_overlay {
        stacked = stacked.push(o);
    }
    stacked.into()
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
) -> anyhow::Result<replay_session::ReplaySession> {
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
        let entry = tango_gamedb::find_by_family_and_variant(&gi.rom_family, variant)
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
    replay_session::ReplaySession::new(
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
) -> anyhow::Result<pvp_session::PvpSession> {
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
        tango_gamedb::find_by_family_and_variant(&remote_gi.family_and_variant.0, remote_gi.family_and_variant.1)
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

    // Build the opponent's Loaded only if they enabled reveal-
    // setup — otherwise we don't have visibility into their save.
    // Loaded parses chip/navi/navicust assets from the rom + wram,
    // so the session pane can render them with the same widgets we
    // use for the local side.
    let opponent_loaded = if pre_match.remote_settings.reveal_setup {
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

    pvp_session::PvpSession::new(
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
) -> anyhow::Result<singleplayer_session::SinglePlayerSession> {
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
    singleplayer_session::SinglePlayerSession::new(
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
