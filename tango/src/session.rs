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
    pub fn snapshot_vbuf(&self) -> Vec<u8> {
        match self {
            Self::Replay(s) => s.snapshot_vbuf(),
            Self::SinglePlayer(s) => s.snapshot_vbuf(),
            Self::PvP(s) => s.snapshot_vbuf(),
        }
    }

    /// Monotonic per-session frame counter. The UI tick compares
    /// against the last value it pushed to GPU and skips the
    /// rebuild when unchanged — without this the high-refresh
    /// display would re-upload the same texture multiple times
    /// per emulator frame, racing with the present and showing as
    /// tearing.
    pub fn frame_id(&self) -> u64 {
        match self {
            Self::Replay(s) => s.frame_id(),
            Self::SinglePlayer(s) => s.frame_id(),
            Self::PvP(s) => s.frame_id(),
        }
    }

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
#[derive(Default)]
pub struct State {
    pub active: Option<ActiveSession>,
    pub frame: Option<iced::widget::image::Handle>,
    /// Bumped each tick to give the iced `image::Handle::Rgba` an
    /// always-fresh id (without that, iced caches the texture and the
    /// emulator picture freezes).
    pub frame_counter: u64,
    /// Frame id of the source emulator frame the current `frame`
    /// Handle was built from. Set in the Tick handler and used to
    /// skip texture rebuilds when mgba hasn't produced a new
    /// frame yet (host vsync > emu fps).
    pub displayed_frame_id: u64,
    /// PvP-only: shows the opponent's save view in a side panel
    /// when they enabled reveal-setup. Defaults to visible when
    /// the panel is available; user can hide it via the toggle
    /// button in the header.
    pub show_opponent_panel: bool,
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
    /// 60 Hz tick from the subscription. Pulls a fresh framebuffer
    /// out of the emulator and updates `state.frame`.
    Tick,
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
    /// User interacted with the opponent's save-view (tab swap,
    /// folder-group toggle, hover, …). PvP-only.
    OpponentSaveViewAction(save_view::Action),
    /// Show the in-session Settings overlay. The emulator keeps
    /// running; only the visible body swaps. Replaces the
    /// legacy in-game pause menu.
    OpenSettings,
    /// Hide the in-session Settings overlay (the "back to
    /// session" button on the overlay's header).
    CloseSettings,
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
            Message::Tick => {
                if let Some(session) = self.active.as_ref() {
                    // Match background task signaled it's done
                    // (clean finish / peer disconnect / comm
                    // error). Self-close so the user isn't stuck
                    // on a frozen final frame.
                    if session.is_ended() {
                        return iced::Task::done(Message::Close);
                    }
                    // Skip the rebuild + GPU re-upload when the
                    // emulator hasn't advanced. On a 144 Hz host
                    // running a 60 fps game that's >50% of ticks.
                    let fid = session.frame_id();
                    if fid == self.displayed_frame_id && self.frame.is_some() {
                        return iced::Task::none();
                    }
                    let pixels = session.snapshot_vbuf();
                    let src_w = replay_session::SCREEN_WIDTH as usize;
                    let src_h = replay_session::SCREEN_HEIGHT as usize;
                    // Run the upscale filter selected in
                    // settings, if any. Bad / empty name falls
                    // back to NullFilter (pass-through).
                    let filter = crate::video::filter_by_name(video_filter)
                        .unwrap_or_else(|| Box::new(crate::video::NullFilter));
                    let [out_w, out_h] = filter.output_size([src_w, src_h]);
                    let (w, h, mut buf) = if [out_w, out_h] == [src_w, src_h] {
                        (src_w as u32, src_h as u32, pixels)
                    } else {
                        let mut dst = vec![0u8; out_w * out_h * 4];
                        filter.apply(&pixels, &mut dst, [src_w, src_h]);
                        (out_w as u32, out_h as u32, dst)
                    };
                    // hqx operates on 24-bit RGB and masks the
                    // alpha byte to 0 in every output pixel
                    // (see `MASK_RGB = 0x00FFFFFF` in the hqx
                    // crate). The result reads as fully
                    // transparent in iced and shows as black /
                    // strobing depending on what's underneath.
                    // Pure-2x MMPX preserves alpha, but it's
                    // cheap to re-stamp unconditionally.
                    for chunk in buf.chunks_mut(4) {
                        chunk[3] = 0xff;
                    }
                    self.frame = Some(iced::widget::image::Handle::from_rgba(w, h, buf));
                    self.frame_counter = self.frame_counter.wrapping_add(1);
                    self.displayed_frame_id = fid;
                }
            }
            Message::Close => {
                if let Some(s) = self.active.as_ref() {
                    s.request_close();
                }
                self.active = None;
                self.frame = None;
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
            Message::OpponentSaveViewAction(action) => {
                if let Some(ActiveSession::PvP(s)) = self.active.as_mut() {
                    let sv_task = s.opponent_save_view.apply(&action);
                    return sv_task.map(Message::OpponentSaveViewAction);
                }
            }
            Message::OpenSettings => {
                self.show_settings = true;
            }
            Message::CloseSettings => {
                self.show_settings = false;
            }
        }
        iced::Task::none()
    }
}

/// Keyboard, gamepad, and the per-redraw Tick all come through
/// `InputCapture` in `App::view` — see that widget's module
/// docs for why the subscription path is too laggy.
pub fn subscription(_state: &State) -> iced::Subscription<Message> {
    iced::Subscription::none()
}

/// Iced texture handle for a Game's background art, cached by Game
/// pointer. The Game's `background_image` LazyImage decodes once on
/// first deref; this helper additionally caches the RGBA → iced
/// `Handle` conversion so we don't re-upload the same texture every
/// frame.
fn background_handle(game: &'static crate::game::Game) -> iced::widget::image::Handle {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static CACHE: LazyLock<parking_lot::Mutex<HashMap<usize, iced::widget::image::Handle>>> =
        LazyLock::new(Default::default);
    let key = game as *const _ as usize;
    if let Some(h) = CACHE.lock().get(&key).cloned() {
        return h;
    }
    let rgba = game.background_image.to_rgba8();
    let (w, h) = rgba.dimensions();
    let handle = iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw());
    CACHE.lock().insert(key, handle.clone());
    handle
}

/// Render the active session — framebuffer, header, and (for replays
/// only) the transport row with play/pause + scrubber + prefetch %.
/// Pass the App's `session: State` borrow.
pub fn view<'a>(lang: &'a LanguageIdentifier, state: &'a State, fractional_scaling: bool) -> Element<'a, Message> {
    let Some(session) = state.active.as_ref() else {
        return iced::widget::Space::new().width(Fill).height(Fill).into();
    };
    let frame_handle = state.frame.as_ref();
    use iced::widget::{image, Space};

    let frame: Element<'a, Message> = if let Some(handle) = frame_handle {
        // Source texture dimensions — drive scale calc inside
        // `responsive`. `Handle::Rgba` carries them; other
        // variants shouldn't appear here but fall back to the
        // native GBA size.
        let (img_w, img_h) = match handle {
            iced::widget::image::Handle::Rgba { width, height, .. } => (*width as f32, *height as f32),
            _ => (
                replay_session::SCREEN_WIDTH as f32,
                replay_session::SCREEN_HEIGHT as f32,
            ),
        };
        // `responsive` to grab the available size and produce a
        // Fixed-size framebuffer. We always go through it (even
        // when fractional scaling is on) so the surrounding
        // shadow container can be sized to the *displayed*
        // pixels, not the pane — that way the drop shadow traces
        // the GBA screen edges instead of the whole pane.
        let handle = handle.clone();
        iced::widget::responsive(move |size| {
            let raw_scale = (size.width / img_w).min(size.height / img_h).max(0.0);
            let scale = if fractional_scaling {
                raw_scale
            } else {
                raw_scale.floor().max(1.0)
            };
            let (w, h) = (img_w * scale, img_h * scale);
            let img = image(handle.clone())
                .width(Length::Fixed(w))
                .height(Length::Fixed(h))
                .filter_method(image::FilterMethod::Nearest)
                .content_fit(iced::ContentFit::Fill);
            // Tight wrapper so the shadow style hugs the
            // displayed framebuffer rather than its parent pane.
            let framed = iced::widget::container(img)
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
    } else {
        Space::new().width(Fill).height(Fill).into()
    };

    // Controls-strip sizing: one icon size + padding so the
    // play/pause, settings, close, opponent-toggle buttons all
    // sit at the same height as the scrubber + speed picker.
    // Matches the play-tab bottom bar so the chrome reads as
    // family across screens.
    const CTRL_ICON: f32 = 16.0;
    const CTRL_PAD: [f32; 2] = [10.0, 14.0];

    let ctrl_icon_btn = |icon: Icon, label: String, msg: Message| -> Element<'a, Message> {
        iced::widget::tooltip(
            iced::widget::button(icon.widget().size(CTRL_ICON))
                .padding(CTRL_PAD)
                .height(iced::Length::Fixed(crate::app::BAR_CONTROL_HEIGHT))
                .style(widgets::neutral)
                .on_press(msg),
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

    // PvP-only: if the opponent revealed their setup, expose a
    // toggle for the side panel so the user can collapse it
    // mid-match without losing it. Folded into the controls strip
    // below alongside the close button.
    let opponent_toggle: Option<Element<'a, Message>> = match session {
        ActiveSession::PvP(s) if s.opponent_loaded.is_some() => {
            let (icon, label) = if state.show_opponent_panel {
                (Icon::ArrowRightFromLine, t!(lang, "session-hide-opponent"))
            } else {
                (Icon::ArrowLeftFromLine, t!(lang, "session-show-opponent"))
            };
            Some(ctrl_icon_btn(icon, label, Message::ToggleOpponentPanel))
        }
        _ => None,
    };
    let close_btn = ctrl_icon_btn(Icon::X, t!(lang, "playback-close"), Message::Close);

    let mut layout = column![].spacing(0).width(Fill).height(Fill);

    // Body: framebuffer over the game's background art, optionally
    // split with the opponent's save view on the right when
    // reveal-setup is active + panel toggled on. The background
    // image is cover-fit so it fills the pane and crops as needed.
    let bg_handle = background_handle(session.local_game());
    let bg = iced::widget::image(bg_handle)
        .width(Fill)
        .height(Fill)
        .content_fit(iced::ContentFit::Cover);
    let emu_pane: Element<'a, Message> = stack![bg, container(frame).center(Fill).padding(8)].into();
    let body: Element<'a, Message> = match session {
        ActiveSession::PvP(s) if state.show_opponent_panel && s.opponent_loaded.is_some() => {
            let opponent = s.opponent_loaded.as_ref().unwrap();
            let panel =
                save_view::view(lang, opponent, &s.opponent_save_view, true, None).map(Message::OpponentSaveViewAction);
            iced::widget::row![
                emu_pane,
                iced::widget::rule::vertical(1),
                container(panel).width(iced::Length::Fixed(380.0)).height(Fill),
            ]
            .height(Fill)
            .into()
        }
        _ => emu_pane,
    };
    layout = layout.push(body);

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
        // No transport widgets for SP/PvP — push a spacer so the
        // close button (and opponent toggle) hug the right edge.
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
                text(format!(
                    "fadv {:+3}:{:+3}",
                    s.local_frame_advantage, s.remote_frame_advantage
                ))
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

    // Options popover. Built as a top Stack layer anchored above
    // the HUD bar so it floats over the framebuffer without pushing
    // the controls strip up. Only present while the cogwheel toggle
    // is engaged on a replay session — the menu owns its own dismiss
    // (changing a setting closes it; clicking the cogwheel again
    // toggles it off).
    //
    // Sectioned: a small caption labels each settings group so when
    // we add more replay knobs they slot in alongside Speed instead
    // of needing their own popover.
    if state.show_options_menu && session.as_replay().is_some() {
        let r = session.as_replay().unwrap();
        let current = r.speed();
        let opts: &[f32] = &[0.5, 1.0, 2.0, 4.0];
        // Flat menu-row style: no border / shadow / chunky bevel at
        // rest, just a subtle hover wash. The button chrome we use
        // elsewhere reads as transport widgets and looks busy when
        // a column of them is stacked — a select-menu row needs to
        // read as a list line item, not a button.
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
            // Reserve a fixed slot for the check glyph so the labels
            // stay vertically aligned regardless of which row is
            // selected.
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
        // Anchor to bottom-right and lift above the HUD bar (control
        // height + bar padding + scanline + a small gap).
        let lift = crate::app::BAR_CONTROL_HEIGHT + 20.0 + 3.0 + 6.0;
        let overlay = container(popover)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(iced::Padding {
                top: 0.0,
                right: 16.0,
                bottom: lift,
                left: 0.0,
            });
        stack![layout, overlay].into()
    } else {
        layout.into()
    }
}

/// Decode a `.tangoreplay`, resolve both sides' ROM (+ optional
/// patch) from the scanners, and spin up a playback session bound to
/// the shared audio binder. Ready to drop straight into the app's
/// `session` slot.
pub fn build_playback(
    scanners: &Scanners,
    config: &config::Config,
    audio_binder: &audio::LateBinder,
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
    replay_session::ReplaySession::new(local_game, local_rom, remote_game, remote_rom, replay, audio_binder)
}

/// Build the live PvP session from the netplay handoff data
/// plus the local selection + scanners. Async because
/// PvpSession::new awaits the lobby loop's receiver handoff,
/// and because remote-side rom resolution might apply a patch.
pub async fn spawn_pvp(
    scanners: Scanners,
    config: config::Config,
    audio_binder: audio::LateBinder,
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

    pvp_session::PvpSession::new(
        local_game_impl,
        std::sync::Arc::new(local_rom_bytes),
        remote_game_impl,
        std::sync::Arc::new(remote_rom_bytes),
        pre_match,
        &config.replays_path(),
        &audio_binder,
        opponent_loaded,
        throttler_factory_for(config.netplay_throttler),
    )
    .await
}

/// Build a throttler factory closure for the given config setting.
/// Shared between `spawn_pvp` (initial round) and the app's settings
/// handler (live mid-round swap).
pub fn throttler_factory_for(throttler: config::NetplayThrottler) -> tango_pvp::battle::ThrottlerFactory {
    match throttler {
        config::NetplayThrottler::AsymmetricEma => {
            Box::new(|| Box::new(tango_pvp::battle::throttler::AsymmetricEma::default()))
        }
        config::NetplayThrottler::LinearWatchdog => Box::new(|| {
            Box::new(tango_pvp::battle::throttler::Watchdog::new(
                tango_pvp::battle::throttler::Linear::new(),
            ))
        }),
        config::NetplayThrottler::Power => Box::new(|| Box::new(tango_pvp::battle::throttler::Power::default())),
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
