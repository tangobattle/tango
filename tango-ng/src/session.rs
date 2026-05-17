//! Live emulator-session machinery: state struct, per-session
//! Message + update + view + subscription. Owned by App as
//! `session: session::State` and routed via `Message::Session(_)`.
//!
//! The Play / Replays tabs are responsible for STARTING sessions
//! (they construct an ActiveSession via [`build_playback`] /
//! [`spawn_singleplayer`] and stuff it into `state.active`); this
//! module handles everything that happens after.

use crate::audio;
use crate::config;
use crate::i18n::t;
use crate::icons;
use crate::patch;
use crate::pvp_session;
use crate::replay_session;
use crate::save_view;
use crate::scrubber;
use crate::selection;
use crate::singleplayer_session;
use crate::{game, Scanners, STANDARD_PADDING, STANDARD_TEXT_SIZE, TEXT_CAPTION, TEXT_HEADING};
use iced::widget::{column, container, horizontal_rule, horizontal_space, row, text};
use iced::{Alignment, Element, Fill};
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

    pub fn as_replay(&self) -> Option<&replay_session::ReplaySession> {
        match self {
            Self::Replay(s) => Some(s),
            _ => None,
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
    /// Show/hide the opponent's reveal-setup side panel. PvP-only.
    ToggleOpponentPanel,
    /// User interacted with the opponent's save-view (tab swap,
    /// folder-group toggle, hover, …). PvP-only.
    OpponentSaveViewAction(save_view::Action),
}

/// Atomic input event we feed to the mapping resolver. Carries
/// the raw key/button/axis info so the session layer can drive
/// both joyflags and the speed-up edge detector.
#[derive(Debug, Clone)]
pub enum InputEvent {
    Key {
        key: iced::keyboard::Key,
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
    pub fn update(
        &mut self,
        msg: Message,
        mapping: &crate::input::Mapping,
    ) -> iced::Task<Message> {
        match msg {
            Message::Tick => {
                if let Some(session) = self.active.as_ref() {
                    // Skip the rebuild + GPU re-upload when the
                    // emulator hasn't advanced. On a 144 Hz host
                    // running a 60 fps game that's >50% of ticks.
                    let fid = session.frame_id();
                    if fid == self.displayed_frame_id && self.frame.is_some() {
                        return iced::Task::none();
                    }
                    let pixels = session.snapshot_vbuf();
                    self.frame = Some(iced::widget::image::Handle::from_rgba(
                        replay_session::SCREEN_WIDTH,
                        replay_session::SCREEN_HEIGHT,
                        pixels,
                    ));
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
            }
            Message::Input(ev) => {
                match ev {
                    InputEvent::Key { key, pressed } => self.input_held.set_key(&key, pressed),
                    InputEvent::Button { button, pressed } => {
                        self.input_held.set_button(button, pressed)
                    }
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
                    s.set_paused(!s.is_paused());
                }
            }
            Message::Seek(target) => {
                if let Some(s) = self.active.as_ref().and_then(ActiveSession::as_replay) {
                    s.seek_to(target);
                }
            }
            Message::SetSpeed(factor) => match self.active.as_ref() {
                Some(ActiveSession::Replay(s)) => s.set_speed(factor),
                Some(ActiveSession::SinglePlayer(s)) => s.set_speed(factor),
                Some(ActiveSession::PvP(_)) => {
                    // PvP runs at fixed EXPECTED_FPS so both sides
                    // stay in sync — no speed control.
                }
                None => {}
            },
            Message::ToggleOpponentPanel => {
                self.show_opponent_panel = !self.show_opponent_panel;
            }
            Message::OpponentSaveViewAction(action) => {
                if let Some(ActiveSession::PvP(s)) = self.active.as_mut() {
                    s.opponent_save_view.apply(&action);
                }
            }
        }
        iced::Task::none()
    }
}

/// Per-frame redraw tick (only while a session is active) + keyboard
/// subscription (only for single-player sessions, where joyflag input
/// is meaningful). The tick comes from `iced::window::frames`, which
/// fires once per host-display vsync — much better than a fixed-rate
/// timer because the texture upload happens in lock-step with the
/// surface present, with no overshoot when the display is slower or
/// faster than 60 Hz.
pub fn subscription(state: &State) -> iced::Subscription<Message> {
    let mut subs: Vec<iced::Subscription<Message>> = Vec::new();
    if state.is_active() {
        subs.push(iced::window::frames().map(|_| Message::Tick));
    }
    if matches!(
        state.active,
        Some(ActiveSession::SinglePlayer(_)) | Some(ActiveSession::PvP(_))
    ) {
        subs.push(iced::event::listen_with(map_keyboard_event));
        subs.push(gamepad_subscription());
    }
    iced::Subscription::batch(subs)
}

/// Polls gilrs in the background and forwards events to the
/// session pipeline. Subscription ID is shared across renders so
/// iced doesn't tear it down + recreate it every frame. Uses a
/// short blocking-with-timeout poll so we don't peg a CPU core.
fn gamepad_subscription() -> iced::Subscription<Message> {
    iced::Subscription::run_with_id(
        "tango-ng-gamepad",
        iced::stream::channel(64, |mut tx| async move {
            use futures::SinkExt;
            let mut gilrs = match gilrs::Gilrs::new() {
                Ok(g) => g,
                Err(e) => {
                    log::warn!("gilrs init failed: {e:?}");
                    return;
                }
            };
            // gilrs is sync; bounce its event polling through
            // spawn_blocking so iced's reactor stays unblocked.
            // Polling every 4 ms is plenty for input fidelity
            // (250 Hz) and well under one GBA frame.
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(4)).await;
                while let Some(event) = gilrs.next_event() {
                    let msg = match event.event {
                        gilrs::EventType::ButtonPressed(b, _) => crate::input::GamepadButton::from_gilrs(b)
                            .map(|btn| InputEvent::Button { button: btn, pressed: true }),
                        gilrs::EventType::ButtonReleased(b, _) => crate::input::GamepadButton::from_gilrs(b)
                            .map(|btn| InputEvent::Button { button: btn, pressed: false }),
                        gilrs::EventType::AxisChanged(a, v, _) => crate::input::GamepadAxis::from_gilrs(a)
                            .map(|axis| InputEvent::Axis { axis, value: v }),
                        gilrs::EventType::Disconnected => Some(InputEvent::GamepadDisconnected),
                        _ => None,
                    };
                    if let Some(ev) = msg {
                        if tx.send(Message::Input(ev)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }),
    )
}

/// Render the active session — framebuffer, header, and (for replays
/// only) the transport row with play/pause + scrubber + prefetch %.
/// Pass the App's `session: State` borrow.
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
) -> Element<'a, Message> {
    let Some(session) = state.active.as_ref() else {
        return iced::widget::Space::new(Fill, Fill).into();
    };
    let frame_handle = state.frame.as_ref();
    use iced::widget::{image, Space};

    let frame: Element<'a, Message> = if let Some(handle) = frame_handle {
        image(handle.clone())
            .width(Fill)
            .height(Fill)
            .filter_method(image::FilterMethod::Nearest)
            .content_fit(iced::ContentFit::Contain)
            .into()
    } else {
        Space::new(Fill, Fill).into()
    };

    let (title_icon, title_key) = match session {
        ActiveSession::Replay(_) => (icons::WATCH, "replays-watch"),
        ActiveSession::SinglePlayer(_) => (icons::TAB_PLAY, "play-play"),
        ActiveSession::PvP(_) => (icons::TAB_PLAY, "play-play"),
    };
    // PvP-only: if the opponent revealed their setup, expose a
    // toggle for the side panel so the user can collapse it
    // mid-match without losing it.
    let opponent_toggle: Option<Element<'a, Message>> = match session {
        ActiveSession::PvP(s) if s.opponent_loaded.is_some() => {
            let (icon, label_key) = if state.show_opponent_panel {
                (icons::CLOSE, "session-hide-opponent")
            } else {
                (icons::WATCH, "session-show-opponent")
            };
            Some(icons::icon_button(
                icon,
                t(lang, label_key),
                Message::ToggleOpponentPanel,
                STANDARD_TEXT_SIZE,
                STANDARD_PADDING,
            ))
        }
        _ => None,
    };
    let mut header_row = row![
        icons::glyph(title_icon, 14),
        text(t(lang, title_key)).size(TEXT_HEADING),
        horizontal_space(),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding(8);
    if let Some(toggle) = opponent_toggle {
        header_row = header_row.push(toggle);
    }
    header_row = header_row.push(icons::icon_button(
        icons::CLOSE,
        t(lang, "playback-close"),
        Message::Close,
        STANDARD_TEXT_SIZE,
        STANDARD_PADDING,
    ));
    let header = container(header_row).width(Fill);

    let mut layout = column![header, horizontal_rule(1)]
        .spacing(0)
        .width(Fill)
        .height(Fill);

    // Body: framebuffer, optionally split with the opponent's
    // save view on the right when reveal-setup is active +
    // panel toggled on.
    let body: Element<'a, Message> = match session {
        ActiveSession::PvP(s) if state.show_opponent_panel && s.opponent_loaded.is_some() => {
            let opponent = s.opponent_loaded.as_ref().unwrap();
            let panel = save_view::view(lang, opponent, &s.opponent_save_view, true)
                .map(Message::OpponentSaveViewAction);
            iced::widget::row![
                container(frame).center(Fill).padding(8),
                iced::widget::vertical_rule(1),
                container(panel)
                    .width(iced::Length::Fixed(380.0))
                    .height(Fill),
            ]
            .height(Fill)
            .into()
        }
        _ => container(frame).center(Fill).padding(8).into(),
    };
    layout = layout.push(body);

    // Transport (play/pause + scrubber) only makes sense for replay
    // playback — single-player has no defined timeline.
    if let Some(r) = session.as_replay() {
        let total = r.total_ticks().max(1);
        let cur = r.current_tick().min(total);
        let prefetched = r.prefetch_progress().min(total);
        let pct = (prefetched as f32 / total as f32 * 100.0).round() as u32;
        let (play_pause_icon, play_pause_key) = if r.is_paused() {
            (icons::PLAY, "playback-play")
        } else {
            (icons::PAUSE, "playback-pause")
        };
        let scrub = scrubber::Scrubber::new(cur, total, prefetched, Message::Seek)
            .round_boundaries(r.round_boundaries())
            .view();

        // Speed selector — values that don't drift audio noticeably
        // (mgba audio sync starts dropping samples above ~4x).
        let speed_opts = vec![
            SpeedOption(0.5),
            SpeedOption(1.0),
            SpeedOption(2.0),
            SpeedOption(4.0),
        ];
        let current_speed = SpeedOption(r.speed());
        let speed_picker = iced::widget::pick_list(speed_opts, Some(current_speed), |o| {
            Message::SetSpeed(o.0)
        })
        .text_size(STANDARD_TEXT_SIZE)
        .padding(STANDARD_PADDING);

        layout = layout.push(horizontal_rule(1));
        layout = layout.push(
            container(
                row![
                    icons::icon_button(
                        play_pause_icon,
                        t(lang, play_pause_key),
                        Message::TogglePlay,
                        STANDARD_TEXT_SIZE,
                        STANDARD_PADDING,
                    ),
                    text(format_tick(cur)).size(TEXT_CAPTION).style(save_view::muted_text_style),
                    scrub,
                    text(format_tick(total)).size(TEXT_CAPTION).style(save_view::muted_text_style),
                    text(format!("{pct}%"))
                        .size(TEXT_CAPTION)
                        .style(save_view::muted_text_style),
                    speed_picker,
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding(8),
            )
            .width(Fill),
        );
    }

    layout.into()
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
        &'static (dyn game::Game + Send + Sync),
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
            anyhow::anyhow!("no tango-ng impl for {}/{}", gi.rom_family, gi.rom_variant)
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
    local_game: crate::rom::GameRef,
    local_patch: Option<(String, semver::Version)>,
    pre_match: crate::netplay::PreMatchData,
) -> anyhow::Result<pvp_session::PvpSession> {
    let local_game_impl = game::from_gamedb_entry(local_game)
        .ok_or_else(|| anyhow::anyhow!("no tango-ng impl for local game"))?;
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
    let remote_game = tango_gamedb::find_by_family_and_variant(
        &remote_gi.family_and_variant.0,
        remote_gi.family_and_variant.1,
    )
    .ok_or_else(|| anyhow::anyhow!("unknown remote rom"))?;
    let remote_game_impl = game::from_gamedb_entry(remote_game)
        .ok_or_else(|| anyhow::anyhow!("no tango-ng impl for remote game"))?;
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
    loaded: &selection::Loaded,
) -> anyhow::Result<singleplayer_session::SinglePlayerSession> {
    let game = game::from_gamedb_entry(loaded.game).ok_or_else(|| {
        anyhow::anyhow!(
            "no tango-ng game impl for {:?}",
            loaded.game.family_and_variant()
        )
    })?;
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
        patch::apply_patch_from_disk(
            &raw,
            loaded.game,
            &config.patches_path(),
            &p.name,
            &p.version,
        )?
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

/// Forwards every iced keyboard event into the session pipeline
/// as an `InputEvent::Key`. The mapping resolution happens at
/// `Message::Input` handling time — this fn just packages the
/// raw event up so the resolver has access to the user's full
/// Mapping table.
fn map_keyboard_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::Event as Kb;
    match event {
        iced::Event::Keyboard(Kb::KeyPressed { key, .. }) => {
            Some(Message::Input(InputEvent::Key { key, pressed: true }))
        }
        iced::Event::Keyboard(Kb::KeyReleased { key, .. }) => {
            Some(Message::Input(InputEvent::Key { key, pressed: false }))
        }
        _ => None,
    }
}

/// pick_list option newtype for the playback speed selector. f32
/// alone can't go in a pick_list because it doesn't impl Eq/Hash.
#[derive(Clone, Copy)]
pub struct SpeedOption(pub f32);

impl PartialEq for SpeedOption {
    fn eq(&self, other: &Self) -> bool {
        (self.0 - other.0).abs() < 1e-3
    }
}
impl Eq for SpeedOption {}

impl std::fmt::Display for SpeedOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if (self.0 - self.0.trunc()).abs() < 1e-3 {
            write!(f, "{}x", self.0 as i32)
        } else {
            write!(f, "{:.1}x", self.0)
        }
    }
}

/// Convert a tick count (60 Hz GBA frames) into `m:ss` for the scrub
/// bar's wallclock labels.
pub fn format_tick(tick: u32) -> String {
    let total_s = tick / 60;
    let m = total_s / 60;
    let s = total_s % 60;
    format!("{m}:{s:02}")
}
