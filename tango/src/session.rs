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
use crate::widgets;
use iced::widget::canvas::{self, Canvas, Frame, LineCap, Path, Stroke};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{container, stack, text};
use iced::{Alignment, Element, Fill, Length};
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

/// Number of recent samples each PvP footer sparkline retains (~3 s at
/// 60 fps). Cheap to clone per frame; old samples drop off the front.
const HISTORY_LEN: usize = 180;

/// Append a sample to a sparkline window, trimming the oldest to keep
/// it within [`HISTORY_LEN`].
fn push_history(buf: &mut std::collections::VecDeque<f32>, v: f32) {
    buf.push_back(v);
    while buf.len() > HISTORY_LEN {
        buf.pop_front();
    }
}

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
    /// PvP-only rolling sample windows feeding the footer sparklines.
    /// Pushed once per frame in [`Message::UpdateFramebuffer`], capped
    /// to [`HISTORY_LEN`], and cleared when the session tears down.
    /// `tps`/`ping` accrue every frame; `skew`/`depth` only while a
    /// round is live (i.e. `round_stats()` is `Some`).
    pub pvp_tps_history: std::collections::VecDeque<f32>,
    pub pvp_ping_history: std::collections::VecDeque<f32>,
    pub pvp_skew_history: std::collections::VecDeque<f32>,
    pub pvp_depth_history: std::collections::VecDeque<f32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            frame_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
            vbuf: std::sync::Arc::new(std::sync::Mutex::new(vec![
                0u8;
                (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
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
            current_frame: None,
            frame_revision: 0,
            pvp_tps_history: std::collections::VecDeque::new(),
            pvp_ping_history: std::collections::VecDeque::new(),
            pvp_skew_history: std::collections::VecDeque::new(),
            pvp_depth_history: std::collections::VecDeque::new(),
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
    /// PvP-only: footer frame-delay slider moved. Live-sets this side's local
    /// frame delay on the running session; the App also persists it to
    /// config. No peer coordination — it's purely a local display lag.
    SetFrameDelay(u32),
    /// Open/close the ellipsis-anchored options popover.
    ToggleOptionsMenu,
    /// User pressed Esc inside a session. Closes whichever overlay is on
    /// top (settings modal, disconnect confirm, options popover) if any,
    /// otherwise opens the options popover. Routed here rather than from
    /// the InputCapture so the decision sees the current overlay state.
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
            }
            Message::EscPressed => {
                if self.show_settings {
                    self.show_settings = false;
                } else if self.show_disconnect_confirm {
                    self.show_disconnect_confirm = false;
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
            }
            Message::CloseSettings => {
                self.show_settings = false;
            }
            Message::UpdateFramebuffer => {
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
                        self.pvp_tps_history.clear();
                        self.pvp_ping_history.clear();
                        self.pvp_skew_history.clear();
                        self.pvp_depth_history.clear();
                    } else {
                        let pixels = self.vbuf.lock().unwrap().clone();
                        let (width, height, buf) = build_frame_pixels(pixels, video_filter);
                        self.frame_revision = self.frame_revision.wrapping_add(1);
                        self.current_frame = Some(crate::video::framebuffer::Frame {
                            pixels: std::sync::Arc::new(buf),
                            width,
                            height,
                            revision: self.frame_revision,
                        });
                        // Sample the live PvP metrics into the footer
                        // sparkline windows, once per emulator frame.
                        if let ActiveSession::PvP(pvp) = session {
                            let tps = pvp.tps();
                            let ping = pvp.latency_blocking().as_secs_f64() as f32 * 1000.0;
                            let stats = pvp.round_stats();
                            push_history(&mut self.pvp_tps_history, tps);
                            push_history(&mut self.pvp_ping_history, ping);
                            if let Some(s) = stats {
                                push_history(&mut self.pvp_skew_history, s.skew as f32);
                                push_history(&mut self.pvp_depth_history, s.depth as f32);
                            }
                        }
                    }
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

/// Snapshot the framebuffer into upload-ready RGBA: apply the configured
/// upscale filter and re-stamp alpha to 0xff. Returns `(width, height,
/// pixels)` for [`crate::video::framebuffer::Frame`]. Called from
/// [`Message::UpdateFramebuffer`] once per emulator vblank.
fn build_frame_pixels(pixels: Vec<u8>, video_filter: &str) -> (u32, u32, Vec<u8>) {
    let src_w = replay_session::SCREEN_WIDTH as usize;
    let src_h = replay_session::SCREEN_HEIGHT as usize;
    // Run the upscale filter selected in settings, if any. Bad /
    // empty name falls back to NullFilter (pass-through).
    let filter = crate::video::filter_by_name(video_filter).unwrap_or_else(|| Box::new(crate::video::NullFilter));
    let [out_w, out_h] = filter.output_size([src_w, src_h]);
    let (w, h, mut buf) = if [out_w, out_h] == [src_w, src_h] {
        (src_w as u32, src_h as u32, pixels)
    } else {
        let mut dst = vec![0u8; out_w * out_h * 4];
        filter.apply(&pixels, &mut dst, [src_w, src_h]);
        (out_w as u32, out_h as u32, dst)
    };
    for chunk in buf.chunks_mut(4) {
        chunk[3] = 0xff;
    }
    (w, h, buf)
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

/// Live frame-delay control for the PvP footer. Built to look identical to the
/// lobby's frame-delay row — label + 160px slider + fixed-width numeric readout
/// + the latency-driven "suggest" wand. Frame delay is purely local display lag,
/// so dragging it mid-match takes effect on the next rendered frame with no peer
/// coordination.
fn frame_delay_control<'a>(lang: &'a LanguageIdentifier, pvp: &'a pvp_session::PvpSession) -> Element<'a, Message> {
    let fd = pvp.frame_delay();
    let latency = pvp.latency_blocking();

    let slider = iced::widget::slider(
        tango_pvp::battle::MIN_FRAME_DELAY..=tango_pvp::battle::MAX_FRAME_DELAY,
        fd,
        Message::SetFrameDelay,
    )
    .width(Length::Fixed(160.0));

    // "Suggest" button — same legacy formula as the lobby (one-way frames + 1 -
    // 2, clamped to the slider range). Disabled until the first ping reading
    // lands (`latency_blocking` returns zero until then).
    let suggest_msg = if latency.is_zero() {
        None
    } else {
        let one_way_frames = (latency.as_nanos() * 60 / 2 / std::time::Duration::from_secs(1).as_nanos()) as i32;
        let d = (one_way_frames + 1 - 2).clamp(
            tango_pvp::battle::MIN_FRAME_DELAY as i32,
            tango_pvp::battle::MAX_FRAME_DELAY as i32,
        ) as u32;
        Some(Message::SetFrameDelay(d))
    };
    let suggest = widgets::icon_button_maybe(
        Icon::Wand,
        t!(lang, "lobby-frame-delay-suggest"),
        suggest_msg,
        crate::app::STANDARD_PADDING,
    );

    row![
        text(t!(lang, "settings-netplay-frame-delay"))
            .size(TEXT_BODY)
            .style(widgets::muted_text_style),
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

/// Tiny historical line chart for one PvP metric. Strokes the recent
/// sample window across a small rect, tinted by the current health
/// `tone`, over a faint backing + area fill so it reads as a chart at
/// thumbnail size. `baseline` draws a faint reference rule (used by
/// skew for the zero line). Owns its samples — the window is tiny —
/// so the produced Element is `'static`, like the scrubber canvas.
struct Sparkline {
    samples: Vec<f32>,
    min: f32,
    max: f32,
    baseline: Option<f32>,
    tone: StatTone,
    width: f32,
    height: f32,
}

impl Sparkline {
    fn view(self) -> Element<'static, Message> {
        let (w, h) = (self.width, self.height);
        Canvas::new(self)
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .into()
    }
}

impl canvas::Program<Message> for Sparkline {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let color = stat_tone_color(theme, self.tone);

        // Faint backing so the chart reads as its own little panel.
        let bg = Path::rounded_rectangle(iced::Point::new(0.0, 0.0), iced::Size::new(w, h), 2.0.into());
        frame.fill(
            &bg,
            iced::Color {
                a: 0.08,
                ..widgets::muted_color(theme)
            },
        );

        // Value→y mapping (inverted: high value near the top), padded
        // a touch so peaks/troughs don't clip the edges.
        let span = (self.max - self.min).max(1e-3);
        let pad = 1.5;
        let y_of = |v: f32| {
            let t = ((v - self.min) / span).clamp(0.0, 1.0);
            pad + (1.0 - t) * (h - 2.0 * pad)
        };

        if let Some(bv) = self.baseline {
            let by = y_of(bv);
            frame.fill_rectangle(
                iced::Point::new(0.0, by - 0.5),
                iced::Size::new(w, 1.0),
                iced::Color {
                    a: 0.25,
                    ..widgets::muted_color(theme)
                },
            );
        }

        let n = self.samples.len();
        if n >= 2 {
            let dx = w / (n - 1) as f32;
            let pts: Vec<iced::Point> = self
                .samples
                .iter()
                .enumerate()
                .map(|(i, &v)| iced::Point::new(i as f32 * dx, y_of(v)))
                .collect();

            // Soft area fill under the trace.
            let area = Path::new(|b| {
                b.move_to(iced::Point::new(pts[0].x, h));
                for p in &pts {
                    b.line_to(*p);
                }
                b.line_to(iced::Point::new(pts[n - 1].x, h));
                b.close();
            });
            frame.fill(&area, iced::Color { a: 0.16, ..color });

            // The trace itself.
            let line = Path::new(|b| {
                b.move_to(pts[0]);
                for p in &pts[1..] {
                    b.line_to(*p);
                }
            });
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(color)
                    .with_width(1.5)
                    .with_line_cap(LineCap::Round),
            );
        }

        vec![frame.into_geometry()]
    }
}

/// One metric cell: a label `icon`, the current value, then its
/// sparkline. The whole cell — icon, value, and chart — is color-coded
/// by the health `tone`. `baseline` is forwarded to the chart (skew
/// passes `Some(0.0)` for a centered zero rule; the rest pass `None`).
fn sparkline_cell<'a>(
    icon: Icon,
    samples: Vec<f32>,
    min: f32,
    max: f32,
    baseline: Option<f32>,
    tone: StatTone,
    value: String,
) -> Element<'a, Message> {
    let tone_style = move |theme: &iced::Theme| iced::widget::text::Style {
        color: Some(stat_tone_color(theme, tone)),
    };
    let chart = Sparkline {
        samples,
        min,
        max,
        baseline,
        tone,
        width: 46.0,
        height: 16.0,
    }
    .view();
    let value = text(value).size(TEXT_BODY).font(iced::Font::MONOSPACE).style(tone_style);
    row![
        icon.widget().size(TEXT_BODY).style(tone_style),
        value,
        chart
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .into()
}

/// Min/max of a sample window, padded; falls back to `(0, 1)` when the
/// window is empty. Used to auto-scale a sparkline with no fixed range.
fn range_of(samples: &[f32], pad: f32) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for &v in samples {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if lo.is_finite() && hi.is_finite() {
        (lo - pad, hi + pad)
    } else {
        (0.0, 1.0)
    }
}

/// P1/P2 identity tag sitting beside the instrument cluster. Plain
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
/// attention to itself.
fn telemetry_plate(theme: &iced::Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let text = theme.palette().text;
    iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color {
            a: if p.is_dark { 0.06 } else { 0.05 },
            ..text
        })),
        text_color: Some(text),
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
    video_filter: &'a str,
) -> Element<'a, Message> {
    let Some(session) = state.active.as_ref() else {
        return iced::widget::Space::new().width(Fill).height(Fill).into();
    };

    // Post-filter framebuffer dimensions. Drive the scale math below;
    // match the (w, h) `build_frame_pixels` stamps into the frame the
    // `framebuffer` shader uploads.
    let filter = crate::video::filter_by_name(video_filter).unwrap_or_else(|| Box::new(crate::video::NullFilter));
    let [out_w, out_h] = filter.output_size([
        replay_session::SCREEN_WIDTH as usize,
        replay_session::SCREEN_HEIGHT as usize,
    ]);
    let img_w = out_w as f32;
    let img_h = out_h as f32;

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
        // PvP-only: live frame-delay control on the left, built to look identical
        // to the lobby's (label + slider + numeric readout + "suggest" wand).
        // Frame delay here is purely local display lag, so it's safe to drag
        // mid-match — no peer coordination.
        if let ActiveSession::PvP(pvp) = session {
            controls = controls.push(frame_delay_control(lang, pvp));
        }
        controls = controls.push(horizontal_space());
    }
    // PvP-only telemetry deck: TPS, frame skew, rollback depth, ping —
    // each metric drawn as a historical sparkline next to its current
    // value, colored by health (green/amber/red), gathered into one
    // hairline-divided plate. P1/P2 rides outside as an identity tag.
    if let ActiveSession::PvP(pvp) = session {
        let stats = pvp.round_stats();
        let ping_ms = pvp.latency_blocking().as_millis();
        let tps = pvp.tps();
        let fps_target = pvp.fps_target();

        let mut cells: Vec<Element<'a, Message>> = Vec::new();

        // TPS: trace vs. target, colored by how well the emulator keeps
        // up — green at/near rate, amber as it dips, red when it falls
        // well behind (visible netplay stutter). Scaled so a ~15 fps
        // drop spans the chart height.
        let tps_tone = if fps_target <= 0.0 {
            StatTone::Muted
        } else if tps >= fps_target - 1.0 {
            StatTone::Good
        } else if tps >= fps_target - 5.0 {
            StatTone::Warn
        } else {
            StatTone::Bad
        };
        let tps_samples: Vec<f32> = state.pvp_tps_history.iter().copied().collect();
        let (tps_lo, tps_hi) = if fps_target > 0.0 {
            ((fps_target - 15.0).max(0.0), fps_target + 1.0)
        } else {
            range_of(&tps_samples, 1.0)
        };
        cells.push(sparkline_cell(
            Icon::Gauge,
            tps_samples,
            tps_lo,
            tps_hi,
            None,
            tps_tone,
            format!("{:.2}/{:.2}", tps, fps_target),
        ));

        if let Some(s) = stats {
            // Skew: trace centered on a zero rule (above = ahead, below
            // = behind), colored by how tight the sync is — green near
            // parity, amber drifting, red far out. Symmetric scale, at
            // least ±4 frames so small skews don't fill the chart.
            let skew_tone = match s.skew.unsigned_abs() {
                0..=1 => StatTone::Good,
                2..=5 => StatTone::Warn,
                _ => StatTone::Bad,
            };
            let skew_samples: Vec<f32> = state.pvp_skew_history.iter().copied().collect();
            let skew_m = skew_samples.iter().fold(4.0_f32, |m, &v| m.max(v.abs()));
            let skew_label = if s.skew == 0 {
                "  0".to_string()
            } else {
                format!("{:>+3}", s.skew)
            };
            cells.push(sparkline_cell(
                Icon::ArrowLeftRight,
                skew_samples,
                -skew_m,
                skew_m,
                Some(0.0),
                skew_tone,
                skew_label,
            ));

            // Rollback depth: lower = tighter prediction. Green when
            // shallow, amber as it climbs, red when speculation runs
            // deep. Scaled from 0 to at least 4.
            let depth_tone = match s.depth {
                0..=1 => StatTone::Good,
                2..=5 => StatTone::Warn,
                _ => StatTone::Bad,
            };
            let depth_samples: Vec<f32> = state.pvp_depth_history.iter().copied().collect();
            let depth_hi = depth_samples.iter().fold(4.0_f32, |m, &v| m.max(v));
            cells.push(sparkline_cell(
                Icon::Layers2,
                depth_samples,
                0.0,
                depth_hi,
                None,
                depth_tone,
                format!("{:>2}", s.depth),
            ));
        }

        // Ping: trace from 0 to the window peak (with headroom) so
        // spikes stand out, colored by latency band.
        let ping_tone = if ping_ms < 80 {
            StatTone::Good
        } else if ping_ms < 140 {
            StatTone::Warn
        } else {
            StatTone::Bad
        };
        let ping_samples: Vec<f32> = state.pvp_ping_history.iter().copied().collect();
        let ping_hi = (ping_samples.iter().fold(0.0_f32, |m, &v| m.max(v)) * 1.15).max(10.0);
        cells.push(sparkline_cell(
            Icon::Signal,
            ping_samples,
            0.0,
            ping_hi,
            None,
            ping_tone,
            format!("{:>3} ms", ping_ms),
        ));

        // Interleave hairline dividers, then wrap in one flat plate.
        let mut strip = row![].spacing(6).align_y(Alignment::Center);
        for (i, cell) in cells.into_iter().enumerate() {
            if i > 0 {
                strip = strip.push(stat_divider());
            }
            strip = strip.push(cell);
        }
        // P1/P2 sits OUTSIDE the instrument plate — it's an identity
        // label, not a metric, so it reads as a separate tag next to
        // the gauge cluster.
        if let Some(s) = stats {
            controls = controls.push(player_cell(s.local_player_index));
        }
        controls = controls.push(container(strip).padding([3, 9]).style(telemetry_plate));
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
