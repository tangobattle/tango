//! Live emulator-session machinery: a thin enum over the per-mode
//! session structs in `replay_session` / `singleplayer_session`, the
//! constructor helpers that wire ROMs + patches + audio into them,
//! the iced session view, and the keyboard-event subscription mapping.
//!
//! Split out of `main.rs` so the top-level App is just routing.
//! Handlers that need `&mut App` (Message dispatch) stay there.

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
use crate::{game, Message, Scanners, STANDARD_PADDING, STANDARD_TEXT_SIZE};
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

/// Render the active session — framebuffer, header, and (for replays
/// only) the transport row with play/pause + scrubber + prefetch %.
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    session: &'a ActiveSession,
    frame_handle: Option<&'a iced::widget::image::Handle>,
) -> Element<'a, Message> {
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
                Message::SessionClose,
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
        let scrub = scrubber::Scrubber::new(cur, total, prefetched, Message::SessionSeek)
            .round_boundaries(r.round_boundaries())
            .view();
        layout = layout.push(horizontal_rule(1));
        layout = layout.push(
            container(
                row![
                    icons::icon_button(
                        play_pause_icon,
                        t(lang, play_pause_key),
                        Message::SessionTogglePlay,
                        STANDARD_TEXT_SIZE,
                        STANDARD_PADDING,
                    ),
                    text(format_tick(cur)).size(11).style(save_view::muted_text_style),
                    scrub,
                    text(format_tick(total)).size(11).style(save_view::muted_text_style),
                    text(format!("{pct}%"))
                        .size(11)
                        .style(save_view::muted_text_style),
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
/// only emit messages for keys we actually bind.
pub fn map_keyboard_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::Event as Kb;
    match event {
        iced::Event::Keyboard(Kb::KeyPressed { key, .. }) => {
            singleplayer_session::key_to_mgba_bit(&key).map(Message::SessionKeyDown)
        }
        iced::Event::Keyboard(Kb::KeyReleased { key, .. }) => {
            singleplayer_session::key_to_mgba_bit(&key).map(Message::SessionKeyUp)
        }
        _ => None,
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
