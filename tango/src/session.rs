//! Live emulator-session machinery: state struct, per-session
//! Message + update + view + subscription. Owned by App as
//! `session: session::State` and routed via `Message::Session(_)`.
//!
//! The Play / Replays tabs are responsible for STARTING sessions
//! (they construct an ActiveSession via [`build_playback`] /
//! [`spawn_singleplayer`] and stuff it into `state.active`); this
//! module handles everything that happens after.

use crate::app::{Scanners, TEXT_CAPTION};
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
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{column, container, row, stack, text};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
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
    /// [`State::current_handle`]. Pre-sized to GBA dimensions and
    /// reused across sessions — saves the per-session
    /// `Arc<Mutex<Vec<u8>>>` allocation dance and lets the handler
    /// read straight off `State` without dispatching through
    /// `ActiveSession`.
    pub vbuf: std::sync::Arc<parking_lot::Mutex<Vec<u8>>>,
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
    /// Replay-only: cogwheel-anchored options popover. Currently
    /// hosts the playback-speed picker; future per-replay knobs
    /// (filter overrides, audio toggle, etc.) live here too.
    /// Closes when a setting is changed or the session is closed.
    pub show_options_menu: bool,
    /// Latest GBA framebuffer rebuilt into an iced image handle.
    /// Refreshed in [`Message::UpdateFramebuffer`] (which the
    /// session subscription fires once per emulator vblank); the
    /// view widget just renders this handle. `None` between
    /// sessions and before the first frame lands.
    pub current_handle: Option<iced::widget::image::Handle>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            frame_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
            vbuf: std::sync::Arc::new(parking_lot::Mutex::new(vec![
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
            current_handle: None,
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
    /// Open/close the cogwheel-anchored options popover. Replay-only.
    ToggleOptionsMenu,
    /// Show/hide the opponent's reveal-setup side panel. PvP-only.
    ToggleOpponentPanel,
    /// Show/hide the local player's save-view panel. PvP-only.
    ToggleSelfPanel,
    /// Bottom-bar frame-delay slider moved. PvP-only. Intercepted at the
    /// app level (it owns config + the live match handle); the session State
    /// itself has nothing to do with it.
    SetFrameDelay(u32),
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
    /// handler rebuilds the iced texture handle from the active
    /// session's vbuf into [`State::current_handle`] and tears
    /// the session down if it's now ended. Fired by the session
    /// subscription, which wakes on [`State::frame_notify`] —
    /// `notify_one()`'d by both the frame callback and the PvP
    /// end-detection wires.
    UpdateFramebuffer,
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
                self.current_handle = None;
                self.show_options_menu = false;
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
            Message::ToggleOptionsMenu => {
                self.show_options_menu = !self.show_options_menu;
            }
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
            Message::SetFrameDelay(_) => {
                // Handled by the app (config + live match); nothing here.
            }
            Message::OpenSettings => {
                self.show_settings = true;
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
                        self.current_handle = None;
                        self.show_options_menu = false;
                    } else {
                        let pixels = self.vbuf.lock().clone();
                        self.current_handle = Some(build_frame_handle(pixels, video_filter));
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

/// Build an iced texture handle from a freshly-snapshotted
/// framebuffer, applying the configured upscale filter and re-
/// stamping alpha to 0xff. Called from [`Message::UpdateFramebuffer`]
/// once per emulator vblank.
fn build_frame_handle(pixels: Vec<u8>, video_filter: &str) -> iced::widget::image::Handle {
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
    iced::widget::image::Handle::from_rgba(w, h, buf)
}

/// Optional iced texture handle for a Game's background art. Pulls
/// the TGA out of the appropriate BNLC volume's shared `exe.dat` and
/// caches the decoded iced `Handle` per game. `None` whenever Steam
/// / BNLC / the target entry can't be read — caller drops the
/// background widget instead of degrading to a placeholder.
fn background_handle(game: &'static crate::game::Game) -> Option<iced::widget::image::Handle> {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static CACHE: LazyLock<parking_lot::Mutex<HashMap<usize, Option<iced::widget::image::Handle>>>> =
        LazyLock::new(Default::default);
    let key = game as *const _ as usize;
    if let Some(cached) = CACHE.lock().get(&key).cloned() {
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
    CACHE.lock().insert(key, handle.clone());
    handle
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
    frame_delay: u32,
) -> Element<'a, Message> {
    let Some(session) = state.active.as_ref() else {
        return iced::widget::Space::new().width(Fill).height(Fill).into();
    };

    // Post-filter framebuffer dimensions. Drives the integer-scale
    // math below; matches the (w, h) `build_frame_handle` bakes
    // into the iced texture handle stored in `state.current_handle`.
    let filter = crate::video::filter_by_name(video_filter).unwrap_or_else(|| Box::new(crate::video::NullFilter));
    let [out_w, out_h] = filter.output_size([
        replay_session::SCREEN_WIDTH as usize,
        replay_session::SCREEN_HEIGHT as usize,
    ]);
    let img_w = out_w as f32;
    let img_h = out_h as f32;

    let make_image = move || -> iced::widget::Image<iced::widget::image::Handle> {
        // No frame yet → show opaque black (an all-zero RGBA buffer would be
        // transparent). The first `Message::UpdateFramebuffer` (one vblank in)
        // drops the real handle into `state.current_handle`.
        let handle = state.current_handle.clone().unwrap_or_else(|| {
            let mut px = vec![0u8; out_w * out_h * 4];
            for p in px.chunks_exact_mut(4) {
                p[3] = 0xFF;
            }
            iced::widget::image::Handle::from_rgba(out_w as u32, out_h as u32, px)
        });
        iced::widget::image(handle).filter_method(iced::widget::image::FilterMethod::Nearest)
    };

    let frame: Element<'a, Message> = if fractional_scaling {
        // Smooth Fill+Contain — let the renderer scale the image to
        // fit the pane at any ratio.
        make_image()
            .width(Fill)
            .height(Fill)
            .content_fit(iced::ContentFit::Contain)
            .into()
    } else {
        // Integer scaling: pick the largest whole-integer multiple
        // of the source texture that fits — needs the pane size,
        // which only `responsive` can provide.
        iced::widget::responsive(move |size| {
            let scale = (size.width / img_w).min(size.height / img_h).floor().max(1.0);
            let (w, h) = (img_w * scale, img_h * scale);
            let image = make_image()
                .width(Length::Fixed(w))
                .height(Length::Fixed(h))
                .content_fit(iced::ContentFit::Fill);
            // Tight container around the Fixed-size framebuffer so
            // the shadow style traces its edges, not the surrounding
            // pane.
            let framed = iced::widget::container(image)
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
            iced::widget::container(framed)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into()
        })
        .into()
    };

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
         style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style|
         -> Element<'a, Message> {
            let mut btn = iced::widget::button(icon.widget().size(CTRL_ICON))
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
         style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style|
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
            let style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style =
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
            let style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style =
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
    let close_btn = ctrl_icon_btn(Icon::X, t!(lang, "playback-close"), Message::Close);

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
            let panel =
                save_view::view(lang, me, &s.local_save_view, true, None, false).map(Message::SelfSaveViewAction);
            let pane = container(panel)
                .width(iced::Length::Fixed(SETUP_PANE_WIDTH))
                .height(Fill)
                .padding(widgets::PANE_PADDING)
                .style(widgets::panel);
            content_row = content_row.push(
                container(pane).height(Fill).padding(widgets::PANE_PADDING),
            );
        }
    }
    content_row = content_row.push(container(frame_container).width(Fill).height(Fill));
    if let ActiveSession::PvP(s) = session {
        if state.show_opponent_panel && s.opponent_loaded.is_some() {
            let opponent = s.opponent_loaded.as_ref().unwrap();
            let panel =
                save_view::view(lang, opponent, &s.opponent_save_view, true, None, false).map(Message::OpponentSaveViewAction);
            let pane = container(panel)
                .width(iced::Length::Fixed(SETUP_PANE_WIDTH))
                .height(Fill)
                .padding(widgets::PANE_PADDING)
                .style(widgets::panel);
            content_row = content_row.push(
                container(pane).height(Fill).padding(widgets::PANE_PADDING),
            );
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
    let mut controls = row![].spacing(10).align_y(Alignment::Center).padding([10, 16]);
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

        // Cogwheel toggle for the options popover (speed, future
        // per-replay knobs). YouTube-style — keeps the transport at
        // a single row of glyphs while the menu is closed.
        let options_btn = ctrl_icon_btn(Icon::Settings, t!(lang, "playback-options"), Message::ToggleOptionsMenu);

        // Play/Pause is the transport's centerpiece — promote to
        // the primary-button style when paused (the affordance
        // the user is most likely looking for at rest) and keep
        // it neutral while playing. Either way it sits a notch
        // bigger than the other strip controls and is rendered
        // as a perfect circle (square padding + huge radius) so
        // it reads as a console transport button instead of a
        // generic pill.
        let base_style: fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style = if paused {
            widgets::primary_button
        } else {
            widgets::neutral
        };
        let play_pause_style = move |theme: &iced::Theme, status: iced::widget::button::Status| {
            let mut style = base_style(theme, status);
            style.border.radius = 999.0.into();
            style
        };
        // Square button sized to the shared bar-control height
        // so the media bar lines up exactly with the play-tab
        // link bar (both pin their interactive children to the
        // same constant).
        let play_pause_btn = iced::widget::tooltip(
            iced::widget::button(
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
            )
            .push(options_btn);
    } else {
        // No transport widgets for SP/PvP. Drop the self-setup
        // toggle on the left (PvP-only) so it pairs visually
        // with the right-anchored opponent toggle, then push a
        // spacer so the rest of the strip (metrics, settings,
        // opponent, close) hugs the right edge.
        if let Some(t) = self_toggle {
            controls = controls.push(t);
        }
        // PvP-only: frame-delay slider on the left, backed by the same
        // config.frame_delay the lobby + Settings sliders write. Adjusts the
        // live match's presentation delay in place.
        if matches!(session, ActiveSession::PvP(_)) {
            controls = controls.push(
                row![
                    text(t!(lang, "settings-netplay-frame-delay"))
                        .size(TEXT_CAPTION)
                        .style(widgets::muted_text_style),
                    iced::widget::slider(2..=10u32, frame_delay, Message::SetFrameDelay)
                        .width(Length::Fixed(120.0)),
                    text(format!("{}", frame_delay))
                        .size(TEXT_CAPTION)
                        .font(iced::Font::MONOSPACE)
                        .width(Length::Fixed(16.0)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        controls = controls.push(horizontal_space());
    }
    // PvP-only status readout: P1/P2, TPS, frame advantage, ping.
    // Mirrors the legacy bottom-bar metrics in
    // `tango/src/gui/session_view.rs`. Monospaced so values don't
    // wiggle as they tick up. PvP also DOESN'T expose a manual
    // close button — leaving a match is the in-game match-end
    // hook's job (auto-close); the session view auto-tears down
    // when `completion_token.is_complete()`.
    let is_pvp = matches!(session, ActiveSession::PvP(_));
    if let ActiveSession::PvP(pvp) = session {
        let stats = pvp.round_stats();
        let ping_ms = pvp.latency_blocking().as_millis();
        let tps = pvp.tps();
        let fps_target = pvp.fps_target();
        let mut metrics = row![].spacing(10).align_y(Alignment::Center);
        if let Some(s) = stats {
            metrics = metrics.push(
                text(format!("P{}", s.local_player_index + 1))
                    .size(TEXT_CAPTION)
                    .font(iced::Font::MONOSPACE)
                    .style(widgets::muted_text_style),
            );
        }
        metrics = metrics.push(
            text(format!("tps {:5.1}/{:5.1}", tps, fps_target))
                .size(TEXT_CAPTION)
                .font(iced::Font::MONOSPACE)
                .style(widgets::muted_text_style),
        );
        if let Some(s) = stats {
            metrics = metrics.push(
                text(format!("skew {:+3}", s.skew))
                .size(TEXT_CAPTION)
                .font(iced::Font::MONOSPACE)
                .style(widgets::muted_text_style),
            );
        }
        metrics = metrics.push(
            text(format!("ping {:>3} ms", ping_ms))
                .size(TEXT_CAPTION)
                .font(iced::Font::MONOSPACE)
                .style(widgets::muted_text_style),
        );
        controls = controls.push(metrics);
    }
    // Settings shortcut — available in any non-replay session
    // (both PvP and single-player). Replaces the legacy in-game
    // pause menu; the App handler intercepts `OpenSettings`,
    // switches tabs, and tears the session down (the session
    // view replaces the main body while active, so we can't
    // overlay settings in place).
    let is_sp = matches!(session, ActiveSession::SinglePlayer(_));
    if is_pvp || is_sp {
        controls = controls.push(ctrl_icon_btn(
            lucide_icons::Icon::Settings,
            t!(lang, "tab-settings"),
            Message::OpenSettings,
        ));
    }
    if let Some(toggle) = opponent_toggle {
        controls = controls.push(toggle);
    }
    if !is_pvp {
        controls = controls.push(close_btn);
    } else {
        // Silences the unused-binding warning when we skip the
        // close button on PvP.
        let _ = close_btn;
    }
    layout = layout
        .push(widgets::hud_scanline())
        .push(container(controls).width(Fill).style(widgets::hud_bar));

    // Replay options popover. Built as a top Stack layer anchored
    // above the HUD bar so it floats over the framebuffer without
    // pushing the controls strip up. Only present while the
    // cogwheel toggle is engaged on a replay session — the menu
    // owns its own dismiss (changing a setting closes it; clicking
    // the cogwheel again toggles it off).
    let options_overlay: Option<Element<'a, Message>> = if state.show_options_menu && session.as_replay().is_some() {
        let r = session.as_replay().unwrap();
        let current = r.speed();
        let opts: &[f32] = &[0.5, 1.0, 2.0, 4.0];
        let menu_row_style = |selected: bool| {
            move |theme: &iced::Theme, status: iced::widget::button::Status| {
                use iced::widget::button::Status;
                let p = theme.extended_palette();
                let text = theme.palette().text;
                let primary = theme.palette().primary;
                let tint = |a: f32| iced::Background::Color(iced::Color { a, ..primary });
                let bg = match status {
                    Status::Hovered => Some(tint(if p.is_dark { 0.18 } else { 0.14 })),
                    Status::Pressed => Some(tint(if p.is_dark { 0.28 } else { 0.22 })),
                    _ if selected => Some(tint(if p.is_dark { 0.12 } else { 0.10 })),
                    _ => None,
                };
                iced::widget::button::Style {
                    background: bg,
                    text_color: if selected { primary } else { text },
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            }
        };
        let mut speed_col = column![].spacing(1);
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
            let btn = iced::widget::button(content)
                .padding([6, 10])
                .width(iced::Length::Fixed(120.0))
                .style(menu_row_style(selected))
                .on_press(Message::SetSpeed(v));
            speed_col = speed_col.push(btn);
        }
        let speed_section = column![
            container(
                text(t!(lang, "playback-speed"))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
            )
            .padding(iced::Padding {
                top: 4.0,
                right: 10.0,
                bottom: 6.0,
                left: 10.0,
            }),
            speed_col,
        ]
        .spacing(2);
        let popover = container(speed_section).padding(6).style(widgets::panel);
        let lift = crate::app::BAR_CONTROL_HEIGHT + 20.0 + 3.0 + 6.0;
        Some(
            container(popover)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 16.0,
                    bottom: lift,
                    left: 0.0,
                })
                .into(),
        )
    } else {
        None
    };

    let mut stacked = stack![Element::from(layout)];
    if let Some(o) = options_overlay {
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
    vbuf: std::sync::Arc<parking_lot::Mutex<Vec<u8>>>,
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
    vbuf: std::sync::Arc<parking_lot::Mutex<Vec<u8>>>,
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
    // Loaded::build parses chip/navi/navicust assets from the
    // rom + wram, so the session pane can render them with the
    // same widgets we use for the local side.
    let opponent_loaded = if pre_match.remote_settings.reveal_setup {
        let remote_save = remote_game
            .parse_save(&pre_match.remote_save_data)
            .map_err(|e| anyhow::anyhow!("parse remote save: {e:?}"))?;
        let patch_meta = remote_gi.patch.as_ref().and_then(|p| {
            let patches = scanners.patches.read();
            let pinfo = patches.get(&p.name)?;
            let v = pinfo.versions.get(&p.version).cloned()?;
            Some((p.name.clone(), p.version.clone(), v))
        });
        Some(crate::selection::Loaded::build(
            remote_game,
            remote_rom_bytes.clone(),
            std::path::PathBuf::new(),
            remote_save,
            &config.patches_path(),
            patch_meta,
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
        let patch_meta = local_patch.as_ref().and_then(|(name, version)| {
            let patches = scanners.patches.read();
            let pinfo = patches.get(name)?;
            let v = pinfo.versions.get(version).cloned()?;
            Some((name.clone(), version.clone(), v))
        });
        Some(crate::selection::Loaded::build(
            local_game,
            local_rom_bytes.clone(),
            std::path::PathBuf::new(),
            local_save,
            &config.patches_path(),
            patch_meta,
        ))
    };

    pvp_session::PvpSession::new(
        local_game_impl,
        std::sync::Arc::new(local_rom_bytes),
        remote_game_impl,
        std::sync::Arc::new(remote_rom_bytes),
        pre_match,
        &config.replays_path(),
        &audio_binder,
        opponent_loaded,
        local_loaded,
        throttler_factory_for(config.netplay_throttler),
        frame_notify,
        vbuf,
        config.frame_delay,
    )
    .await
}

/// Build a throttler factory closure for the given config setting.
/// Shared between `spawn_pvp` (initial round) and the app's settings
/// handler (live mid-round swap).
pub fn throttler_factory_for(throttler: config::NetplayThrottler) -> tango_pvp::battle::ThrottlerFactory {
    use tango_pvp::battle::throttler::{Clamp, Ema, Linear, Power, Watchdog};
    match throttler {
        // Ema + Linear are symmetric — they can request both
        // slowdown and speed-up. Clamp `min` to 0 here so the
        // surfaced strategies stay slowdown-only, matching the
        // historical asymmetric-EMA / linear-watchdog behavior.
        config::NetplayThrottler::AsymmetricEma => {
            Box::new(|| Box::new(Clamp::<Ema>::default().with_min(0.0)))
        }
        config::NetplayThrottler::LinearWatchdog => {
            Box::new(|| Box::new(Clamp::<Watchdog<Linear>>::default().with_min(0.0)))
        }
        config::NetplayThrottler::Power => Box::new(|| Box::new(Clamp::<Power>::default())),
    }
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
    vbuf: std::sync::Arc<parking_lot::Mutex<Vec<u8>>>,
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
