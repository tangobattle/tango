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
use crate::replay_session;
use crate::save_view;
use crate::scrubber;
use crate::selection;
use crate::singleplayer_session;
use crate::{game, Scanners, STANDARD_PADDING, STANDARD_TEXT_SIZE};
use iced::widget::{column, container, horizontal_rule, horizontal_space, row, text};
use iced::{Alignment, Element, Fill};
use unic_langid::LanguageIdentifier;

/// At most one of these can be active at a time: replay playback, or
/// single-player. The two variants share enough surface (vbuf,
/// close-request) that the view + tick loop wrap them uniformly.
pub enum ActiveSession {
    Replay(replay_session::ReplaySession),
    SinglePlayer(singleplayer_session::SinglePlayerSession),
}

impl ActiveSession {
    pub fn snapshot_vbuf(&self) -> Vec<u8> {
        match self {
            Self::Replay(s) => s.snapshot_vbuf(),
            Self::SinglePlayer(s) => s.snapshot_vbuf(),
        }
    }

    pub fn request_close(&self) {
        match self {
            Self::Replay(s) => s.request_close(),
            Self::SinglePlayer(s) => s.request_close(),
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
    /// Mapped key went down; payload is the mgba joypad bit. Inert
    /// for replay sessions.
    KeyDown(u32),
    /// Mapped key went up.
    KeyUp(u32),
    /// Toggle play/pause on a replay session. No-op for single-player.
    TogglePlay,
    /// Drag the scrub bar — fires on every value change. Replay-only.
    Seek(u32),
    /// Set the playback speed factor (1.0 = realtime). Replay-only.
    SetSpeed(f32),
}

impl State {
    /// Apply a session message to the state. Returns the iced Task
    /// that should be scheduled (always Task::none today — kept for
    /// API parity with the other tabs).
    pub fn update(&mut self, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::Tick => {
                if let Some(session) = self.active.as_ref() {
                    let pixels = session.snapshot_vbuf();
                    self.frame = Some(iced::widget::image::Handle::from_rgba(
                        replay_session::SCREEN_WIDTH,
                        replay_session::SCREEN_HEIGHT,
                        pixels,
                    ));
                    self.frame_counter = self.frame_counter.wrapping_add(1);
                }
            }
            Message::Close => {
                if let Some(s) = self.active.as_ref() {
                    s.request_close();
                }
                self.active = None;
                self.frame = None;
            }
            Message::KeyDown(bit) => {
                if let Some(ActiveSession::SinglePlayer(s)) = self.active.as_ref() {
                    s.set_joyflag(bit, true);
                }
            }
            Message::KeyUp(bit) => {
                if let Some(ActiveSession::SinglePlayer(s)) = self.active.as_ref() {
                    s.set_joyflag(bit, false);
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
                None => {}
            },
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
    if matches!(state.active, Some(ActiveSession::SinglePlayer(_))) {
        subs.push(iced::event::listen_with(map_keyboard_event));
    }
    iced::Subscription::batch(subs)
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
    };
    let header = container(
        row![
            icons::glyph(title_icon, 14),
            text(t(lang, title_key)).size(14),
            horizontal_space(),
            icons::icon_button(
                icons::CLOSE,
                t(lang, "playback-close"),
                Message::Close,
                STANDARD_TEXT_SIZE,
                STANDARD_PADDING,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding(8),
    )
    .width(Fill);

    let mut layout = column![header, horizontal_rule(1)]
        .spacing(0)
        .width(Fill)
        .height(Fill);

    layout = layout.push(container(frame).center(Fill).padding(8));

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
                    text(format_tick(cur)).size(11).style(save_view::muted_text_style),
                    scrub,
                    text(format_tick(total)).size(11).style(save_view::muted_text_style),
                    text(format!("{pct}%"))
                        .size(11)
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

/// `iced::event::listen_with` needs a free `fn` (no captures), so we
/// fold the key→mgba-bit translation into the subscription itself and
/// only emit messages for keys we actually bind. LShift is a special
/// hold-to-fast-forward binding — separate from the joypad mapping
/// so it doesn't collide with any GBA button.
pub fn map_keyboard_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{key::Named, Event as Kb, Key};
    const FAST_FORWARD: f32 = 4.0;
    match event {
        iced::Event::Keyboard(Kb::KeyPressed { key: Key::Named(Named::Shift), .. }) => {
            Some(Message::SetSpeed(FAST_FORWARD))
        }
        iced::Event::Keyboard(Kb::KeyReleased { key: Key::Named(Named::Shift), .. }) => {
            Some(Message::SetSpeed(1.0))
        }
        iced::Event::Keyboard(Kb::KeyPressed { key, .. }) => {
            singleplayer_session::key_to_mgba_bit(&key).map(Message::KeyDown)
        }
        iced::Event::Keyboard(Kb::KeyReleased { key, .. }) => {
            singleplayer_session::key_to_mgba_bit(&key).map(Message::KeyUp)
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
