#![windows_subsystem = "windows"]

mod audio;
mod config;
mod game;
mod i18n;
mod input;
mod navicust;
mod net;
mod netplay;
mod patch;
mod pvp_session;
mod randomcode;
mod replay_session;
mod replays;
mod rom;
mod rom_overrides;
mod save;
mod save_view;
mod scanner;
mod scrubber;
mod selection;
mod session;
mod singleplayer_session;
mod stats;
mod tabs;
mod widgets;

use session::ActiveSession;

/// Push an RGBA image to the OS clipboard. iced's clipboard API
/// only handles text, so we drop down to `arboard` on a tokio
/// background task — both because it can block briefly and
/// because arboard's Clipboard handle isn't Send-safe to keep on
/// the UI thread.
fn copy_image_to_clipboard(img: image::RgbaImage) {
    let (width, height) = (img.width() as usize, img.height() as usize);
    let bytes = img.into_raw();
    tokio::task::spawn_blocking(move || match arboard::Clipboard::new() {
        Ok(mut cb) => {
            let data = arboard::ImageData {
                width,
                height,
                bytes: bytes.into(),
            };
            if let Err(e) = cb.set_image(data) {
                log::warn!("clipboard set_image failed: {e}");
            }
        }
        Err(e) => log::warn!("clipboard open failed: {e}"),
    });
}

/// Bundle of decoded-replay state the export task needs.
/// Pulled together synchronously in `start_replay_export` so the
/// spawned future doesn't have to touch `&self`.
struct ExportPrep {
    local_hooks: &'static (dyn tango_pvp::hooks::Hooks + Send + Sync),
    local_rom: Vec<u8>,
    remote_hooks: &'static (dyn tango_pvp::hooks::Hooks + Send + Sync),
    remote_rom: Vec<u8>,
    replay: tango_pvp::replay::Replay,
}

use i18n::{t, FALLBACK_LANG};
use iced::widget::rule::horizontal as horizontal_rule;
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Fill, Theme};
use tabs::patches::PatchesState;
use tabs::play::{create_new_save, duplicate_save, rename_save, PlayState};
use tabs::replays::ReplaysState;
use unic_langid::LanguageIdentifier;

pub const SUPPORTED_LANGS: &[LanguageIdentifier] = &[unic_langid::langid!("en-US"), unic_langid::langid!("ja-JP")];

// Button sizing constants — three tiers that everything else maps onto.
// `NAV` for the top-level nav strip; `PRIMARY` for the single big
// call-to-action (Play). Standard body text comes from iced's
// `default_text_size` (set in main()), so there's no standalone
// STANDARD_TEXT_SIZE constant — widgets that don't pass an
// explicit size inherit the app default.
pub const NAV_TEXT_SIZE: f32 = 14.0;
pub const NAV_PADDING: [f32; 2] = [8.0, 16.0];
pub const PRIMARY_PADDING: [f32; 2] = [6.0, 14.0];
pub const STANDARD_PADDING: [f32; 2] = [6.0, 14.0];

// Typographic scale. Everything that renders text picks from this
// list; one-off sizes outside it tend to look like UI bugs
// (random 12px next to 11px next to 13px). If you need a new
// size, add it here and update the audit.
//
//   DISPLAY — splash titles ("Welcome to Tango").
//   TITLE   — section headers ("tab-settings", empty-state cards).
//   HEADING — sub-section labels (nickname on side cards).
//   BODY    — default body copy. Same value as the iced default.
//   CAPTION — muted hints, status lines, metadata labels.
pub const TEXT_DISPLAY: f32 = 22.0;
pub const TEXT_TITLE: f32 = 18.0;
pub const TEXT_HEADING: f32 = 15.0;
pub const TEXT_BODY: f32 = 13.0;
pub const TEXT_CAPTION: f32 = 11.0;

// Bundled fonts. We reuse the main app's font files (a few MB total)
// so JP / SC / TC scripts render instead of tofuing out, and so the
// monospace chip-code badge matches the rest of the UI. cosmic-text
// automatically falls back to whichever registered font has the
// requested glyph when the default doesn't.
const FONT_NOTO_SANS: &[u8] = include_bytes!("../../tango/fonts/NotoSans-Regular.ttf");
const FONT_NOTO_SANS_JP: &[u8] = include_bytes!("../../tango/fonts/NotoSansJP-Regular.otf");
const FONT_NOTO_SANS_SC: &[u8] = include_bytes!("../../tango/fonts/NotoSansSC-Regular.otf");
const FONT_NOTO_SANS_TC: &[u8] = include_bytes!("../../tango/fonts/NotoSansTC-Regular.otf");
const FONT_NOTO_SANS_MONO: &[u8] = include_bytes!("../../tango/fonts/NotoSansMono-Regular.ttf");
const FONT_NOTO_EMOJI: &[u8] = include_bytes!("../../tango/fonts/NotoEmoji-Regular.ttf");
// Lucide icon font ships with the `lucide-icons` crate as
// `LUCIDE_FONT_BYTES`; registered with iced below.

pub fn main() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // Route mgba's global default logger through `c_log` too — without
    // this, the prefetcher's bare Core falls through to mgba's printf
    // stub and spams `GBA BIOS: SWI: …` lines straight to stdout.
    mgba::log::install_default_logger();

    // Body text default. Every text widget that doesn't pass an
    // explicit `.size(...)` picks this up — that's the bulk of the
    // UI. Iced's bare default is 16 px; 13 matches what the rest
    // of the typographic scale (TEXT_TITLE / TEXT_HEADING /
    // TEXT_CAPTION) was tuned against.
    //
    // `vsync: false` cuts the present queue. With vsync iced 0.14
    // pipes frames through wgpu's vsync-locked surface, which on
    // 60 Hz monitors adds a full frame (~16 ms) of presentation
    // latency on top of the emulator's own 1-frame input delay.
    // Without vsync the emulator's freshly rendered frame paints
    // immediately, dropping the perceived input lag from ~3 frames
    // to ~1. Risks light tearing on a 60 Hz monitor; the screen
    // area being moved (the GBA screen) is mostly static so this
    // is barely visible in practice.
    let settings = iced::Settings {
        default_text_size: iced::Pixels(13.0),
        vsync: false,
        ..iced::Settings::default()
    };
    iced::application(App::new, App::update, App::view)
        .settings(settings)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size((1000.0, 640.0))
        .font(FONT_NOTO_SANS)
        .font(FONT_NOTO_SANS_JP)
        .font(FONT_NOTO_SANS_SC)
        .font(FONT_NOTO_SANS_TC)
        .font(FONT_NOTO_SANS_MONO)
        .font(FONT_NOTO_EMOJI)
        .font(lucide_icons::LUCIDE_FONT_BYTES)
        // iced 0.14's cosmic-text falls back across registered
        // faces, so we can default to the Latin Noto Sans and let
        // CJK / emoji glyphs come from the JP / SC / TC / Emoji
        // fonts above.
        .default_font(iced::Font::with_name("Noto Sans"))
        .run()
}

#[derive(Clone)]
pub struct Scanners {
    pub roms: rom::Scanner,
    pub saves: save::Scanner,
    pub patches: patch::Scanner,
    pub replays: replays::Scanner,
}

impl Scanners {
    fn new() -> Self {
        Self {
            roms: rom::Scanner::new(),
            saves: save::Scanner::new(),
            patches: patch::Scanner::new(),
            replays: replays::Scanner::new(),
        }
    }

    fn rescan(&self, config: &config::Config) {
        let roms_path = config.roms_path();
        let saves_path = config.saves_path();
        let patches_path = config.patches_path();
        let replays_path = config.replays_path();
        self.roms.rescan(|| Some(rom::scan_roms(&roms_path)));
        self.saves.rescan(|| Some(save::scan_saves(&saves_path)));
        self.patches.rescan(|| patch::scan(&patches_path).ok());
        self.replays.rescan(|| Some(replays::scan_replays(&replays_path)));
    }
}

struct App {
    config: config::Config,
    tab: Tab,
    scanners: Scanners,
    /// Cloned into every session so they can bind their MGBAStream
    /// without owning the cpal backend. The cpal Backend lives in
    /// `_audio_backend` so the underlying stream keeps playing.
    audio_binder: audio::LateBinder,
    /// Kept alive for the program's lifetime; dropping it would tear
    /// down the cpal output stream and the app would go silent.
    _audio_backend: Option<audio::cpal::Backend>,

    /// Owned game+save+assets for the current selection. Rebuilt only
    /// when game or save changes; per-frame view() borrows it.
    loaded: Option<selection::Loaded>,

    play: PlayState,
    replays: ReplaysState,
    patches: PatchesState,
    settings: tabs::settings::State,
    welcome: tabs::welcome::State,
    netplay: netplay::State,

    /// Active emulator session (replay playback or single-player) plus
    /// the cached framebuffer Handle. While `session.is_active()`, the
    /// main body is replaced by `session::view`.
    session: session::State,
}

impl App {
    fn new() -> (Self, iced::Task<Message>) {
        let config = config::Config::load_or_create();
        let _ = FALLBACK_LANG; // re-exported for use in config; suppress unused warning here

        let scanners = Scanners::new();
        scanners.rescan(&config);
        log::info!(
            "initial scan: {} rom(s), {} save game(s), {} patch(es), {} replay(s)",
            scanners.roms.read().len(),
            scanners.saves.read().values().map(|v| v.len()).sum::<usize>(),
            scanners.patches.read().len(),
            scanners.replays.read().len(),
        );

        // Restore the last play selection from config, but only the bits
        // that still resolve against the current scanners.
        let mut play = PlayState::default();
        if let Some((family, variant)) = config.last_game.as_ref() {
            if let Some(game) = tango_gamedb::find_by_family_and_variant(family, *variant) {
                if scanners.roms.read().contains_key(&game) {
                    play.local_game = Some(game);
                    if let Some(p) = config.last_save.as_ref() {
                        if scanners
                            .saves
                            .read()
                            .get(&game)
                            .map(|v| v.iter().any(|s| s.path == *p))
                            .unwrap_or(false)
                        {
                            play.local_save = Some(p.clone());
                        }
                    }
                    if let Some(n) = config.last_patch.as_ref() {
                        if let Some(p) = scanners.patches.read().get(n) {
                            let v = config.last_patch_version.as_ref().and_then(|v| {
                                if p.versions.contains_key(v)
                                    && p.versions
                                        .get(v)
                                        .map(|vm| vm.supported_games.contains(&game))
                                        .unwrap_or(false)
                                {
                                    Some(v.clone())
                                } else {
                                    None
                                }
                            });
                            if v.is_some() {
                                play.local_patch = Some(n.clone());
                                play.local_patch_version = v;
                            }
                        }
                    }
                }
            }
        }
        let welcome = tabs::welcome::State::from_nickname(config.nickname.as_deref());

        // Spin up cpal once at startup with the LateBinder as the
        // source. Sessions later bind their MGBAStream into the binder
        // and the cpal stream keeps going across selections.
        let mut audio_binder = audio::LateBinder::new();
        let audio_backend = match audio::cpal::Backend::new(audio_binder.clone()) {
            Ok(b) => {
                use audio::Backend;
                audio_binder.set_sample_rate(b.sample_rate());
                log::info!("audio: cpal backend up at {} Hz", b.sample_rate());
                Some(b)
            }
            Err(e) => {
                log::warn!("audio: cpal init failed, running silent: {e:?}");
                None
            }
        };

        let mut app = Self {
            config,
            tab: Tab::default(),
            welcome,
            settings: tabs::settings::State::default(),
            scanners,
            audio_binder,
            _audio_backend: audio_backend,
            loaded: None,
            play,
            replays: ReplaysState::default(),
            patches: PatchesState::default(),
            session: session::State::new(),
            netplay: netplay::State::new(),
        };
        app.refresh_loaded();
        let stats_task = app.kick_replay_stats_loader().map(Message::Replays);
        (app, stats_task)
    }

    /// Drops cached replay stats for paths that no longer exist in
    /// the latest scan, then kicks the worker for any newly-scanned
    /// paths that don't have stats yet. Returns tab-scoped Task —
    /// caller wraps with `.map(Message::Replays)` if at App level.
    fn refresh_replay_stats(&mut self) -> iced::Task<tabs::replays::Message> {
        let live: std::collections::HashSet<std::path::PathBuf> = self
            .scanners
            .replays
            .read()
            .iter()
            .map(|r| r.path.clone())
            .collect();
        self.replays.stats.retain(|p, _| live.contains(p));
        self.kick_replay_stats_loader()
    }

    /// Spawn a streaming task that decodes each not-yet-cached
    /// replay on a blocking worker, one at a time, posting each
    /// result back as a `StatsLoaded` message. Returns Task::none
    /// when there's no work to do.
    fn kick_replay_stats_loader(&self) -> iced::Task<tabs::replays::Message> {
        let paths: Vec<std::path::PathBuf> = self
            .scanners
            .replays
            .read()
            .iter()
            .filter(|r| !self.replays.stats.contains_key(&r.path))
            .map(|r| r.path.clone())
            .collect();
        if paths.is_empty() {
            return iced::Task::none();
        }
        use futures::StreamExt;
        let stream = futures::stream::iter(paths)
            .then(|path| async move {
                let p = path.clone();
                let stats = tokio::task::spawn_blocking(move || replays::compute_stats(&p).ok())
                    .await
                    .ok()
                    .flatten();
                (path, stats)
            })
            .filter_map(|(path, stats)| async move {
                stats.map(|s| tabs::replays::Message::StatsLoaded(path, s))
            });
        iced::Task::stream(stream)
    }

    /// Persist `self.config` to disk. Failures are logged but otherwise
    /// swallowed so a transient write error doesn't crash the UI.
    fn persist_config(&self) {
        if let Err(e) = self.config.save() {
            log::error!("failed to save config: {e}");
        }
    }

    /// Record the current selection back to config.last_*; called after
    /// any selection change so the next launch restores it.
    fn persist_selection(&mut self) {
        self.config.last_game = self
            .play
            .local_game
            .map(|g| (g.family_and_variant().0.to_string(), g.family_and_variant().1));
        self.config.last_save = self.play.local_save.clone();
        self.config.last_patch = self.play.local_patch.clone();
        self.config.last_patch_version = self.play.local_patch_version.clone();
        self.persist_config();
    }

    /// Snapshot of the inputs that determine `loaded`, used to skip
    /// rebuilds when nothing relevant changed.
    /// Build the current Settings packet + dispatch SendLocalSettings
    /// — only meaningful while netplay is in Lobby phase; outside
    /// that this returns `Task::none()`. Wrapped in a helper because
    /// it has three callers: lobby entry, selection change, and
    /// match-type change.
    fn resend_settings_if_lobby(&mut self) -> iced::Task<Message> {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        // Default match-type policy:
        //   - Game JUST changed (or first selection in this lobby):
        //     pick Triple (mode=1) if the game supports it, else
        //     Single. This is the "default to triple" the user wants
        //     — keyed off `default_mt_for_game` so it only fires once
        //     per (lobby, game) pair.
        //   - Same game, current value invalid for it: same fallback
        //     (paranoia).
        //   - Same game, valid value: leave alone — sticky user pick.
        if let Some(game) = self.play.local_game {
            let mt_table = game::from_gamedb_entry(game).map(|g| g.match_types()).unwrap_or(&[]);
            let game_key = {
                let (fam, var) = game.family_and_variant();
                (fam.to_string(), var)
            };
            let game_changed = self.netplay.lobby.default_mt_for_game.as_ref() != Some(&game_key);
            let (mode, sub) = self.netplay.lobby.match_type;
            let current_valid =
                (mode as usize) < mt_table.len() && (sub as usize) < *mt_table.get(mode as usize).unwrap_or(&0);
            if game_changed || !current_valid {
                let new_mt = if mt_table.get(1).copied().unwrap_or(0) > 0 {
                    (1, 0) // Triple
                } else {
                    (0, 0) // Single
                };
                self.netplay.lobby.match_type = new_mt;
                self.netplay.lobby.default_mt_for_game = Some(game_key);
            }
        }
        let settings = self.make_local_settings();
        self.netplay
            .update(netplay::Message::SendLocalSettings(Box::new(settings)))
            .map(Message::Netplay)
    }

    /// Build a `protocol::Settings` packet from the App's current
    /// state: nickname from config, match_type defaults to (0, 0),
    /// game_info from the Play tab's local selection, and the
    /// available_games / available_patches lists from the scanners
    /// so the peer can see what we have locally. Mirrors
    /// `tango/src/gui/play_pane.rs::make_local_settings`.
    fn make_local_settings(&self) -> net::protocol::Settings {
        use net::protocol::{GameInfo, PatchInfo, Settings};
        let roms = self.scanners.roms.read();
        let patches = self.scanners.patches.read();
        Settings {
            nickname: self.config.nickname.clone().unwrap_or_default(),
            match_type: self.netplay.lobby.match_type,
            game_info: self.play.local_game.map(|game| {
                let (family, variant) = game.family_and_variant();
                GameInfo {
                    family_and_variant: (family.to_string(), variant),
                    patch: match (&self.play.local_patch, &self.play.local_patch_version) {
                        (Some(name), Some(version)) => Some(PatchInfo {
                            name: name.clone(),
                            version: version.clone(),
                        }),
                        _ => None,
                    },
                }
            }),
            available_games: roms
                .keys()
                .map(|g| {
                    let (family, variant) = g.family_and_variant();
                    (family.to_string(), variant)
                })
                .collect(),
            available_patches: patches
                .iter()
                .map(|(name, info)| (name.clone(), info.versions.keys().cloned().collect()))
                .collect(),
            reveal_setup: self.netplay.lobby.reveal_setup,
        }
    }

    fn loaded_key(&self) -> Option<(rom::GameRef, std::path::PathBuf, Option<(String, semver::Version)>)> {
        let game = self.play.local_game?;
        let save_path = self.play.local_save.clone()?;
        let patch = match (&self.play.local_patch, &self.play.local_patch_version) {
            (Some(n), Some(v)) => Some((n.clone(), v.clone())),
            _ => None,
        };
        Some((game, save_path, patch))
    }

    /// Recompute `self.loaded` from `play.local_game` + `play.local_save`
    /// + `play.local_patch[+version]`. Cheap when nothing's changed;
    /// expensive when ROM/assets need a fresh parse (BPS + asset
    /// parsing + icon decode), which is why we don't call it from view().
    fn refresh_loaded(&mut self) {
        let Some((game, save_path, patch)) = self.loaded_key() else {
            self.loaded = None;
            return;
        };

        // Reuse existing if all inputs still match.
        if let Some(l) = &self.loaded {
            let cur_patch = l.patch.as_ref().map(|p| (p.name.clone(), p.version.clone()));
            if l.game == game && l.save_path == save_path && cur_patch == patch {
                return;
            }
        }

        let roms = self.scanners.roms.read();
        let saves = self.scanners.saves.read();
        let patches = self.scanners.patches.read();
        let Some(rom) = roms.get(&game).cloned() else {
            self.loaded = None;
            return;
        };
        let Some(scanned) = saves.get(&game).and_then(|v| v.iter().find(|s| s.path == save_path)) else {
            self.loaded = None;
            return;
        };
        let save = scanned.save.clone_box();
        let patch_meta = patch.and_then(|(name, version)| {
            patches
                .get(&name)
                .and_then(|p| p.versions.get(&version).map(|v| (name.clone(), version, v.clone())))
        });
        drop(patches);
        drop(saves);
        drop(roms);

        log::info!(
            "loading selection: {:?} {} {}",
            game.family_and_variant(),
            save_path.display(),
            patch_meta
                .as_ref()
                .map(|(n, v, _)| format!("[{n} v{v}]"))
                .unwrap_or_default(),
        );
        let patches_path = self.config.patches_path();
        self.loaded = Some(selection::Loaded::build(
            game,
            rom,
            save_path,
            save,
            &patches_path,
            patch_meta,
        ));
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Play,
    Replays,
    Patches,
    Settings,
}

/// Top-level Message. Tab-specific messages live in each tab module
/// and are wrapped here; the dispatch in `App::update` routes them to
/// per-tab `update_*` methods below.
#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(Tab),
    Play(tabs::play::Message),
    Patches(tabs::patches::Message),
    Replays(tabs::replays::Message),
    Settings(tabs::settings::Message),
    Welcome(tabs::welcome::Message),
    Session(session::Message),
    Netplay(netplay::Message),
    /// Carries the freshly-constructed PvP session back into the
    /// App after the async build task in `spawn_pvp` resolves.
    /// `Slot` because PvpSession isn't Clone.
    PvpSessionBuilt(netplay::Slot<anyhow::Result<pvp_session::PvpSession>>),
}

impl App {
    fn title(&self) -> String {
        t(&self.config.language, "window-title")
    }

    fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::TabSelected(t) => {
                self.tab = t;
                iced::Task::none()
            }
            // PlayPressed branches to the netplay path when the user
            // typed a link code. We special-case it here because
            // update_play returns Task<play::Message>, not
            Message::Play(m) => {
                // Play tab handlers funnel through update_play +
                // an Effect dispatch (including the netplay ones).
                // Always follow with a Settings resend — the
                // netplay handler dedupes against the last-sent
                // value via `Settings: Eq`, so unchanged
                // dispatches are free.
                let task = self.update_play(m);
                iced::Task::batch([task, self.resend_settings_if_lobby()])
            }
            Message::Patches(m) => self.update_patches(m).map(Message::Patches),
            Message::Replays(m) => self.update_replays(m).map(Message::Replays),
            Message::Settings(m) => self.update_settings(m).map(Message::Settings),
            Message::Welcome(m) => self.update_welcome(m).map(Message::Welcome),
            Message::Session(m) => {
                // The active session may have mutated the user's
                // save file on disk (single-player writes via
                // mgba's RW VFile). On Close, drop the session
                // first so mgba's thread joins + flushes its
                // file handle, then re-scan saves + force a
                // Loaded rebuild so the play tab's save view
                // reflects the fresh on-disk SRAM.
                let sp_closing = matches!(m, session::Message::Close)
                    && matches!(self.session.active, Some(ActiveSession::SinglePlayer(_)));
                // PvP sessions write a `.tangoreplay` next to the
                // saves dir on match end; once the session closes
                // we want the new file to show up in the Replays
                // tab without a manual rescan.
                let pvp_closing =
                    matches!(m, session::Message::Close) && matches!(self.session.active, Some(ActiveSession::PvP(_)));
                let task = self.session.update(m, &self.config.input_mapping).map(Message::Session);
                if sp_closing {
                    let saves_path = self.config.saves_path();
                    self.scanners.saves.rescan(|| Some(save::scan_saves(&saves_path)));
                    // Bypass refresh_loaded's same-key dedupe —
                    // the path + game haven't changed, only the
                    // bytes have.
                    self.loaded = None;
                    self.refresh_loaded();
                }
                if pvp_closing {
                    let replays_path = self.config.replays_path();
                    self.scanners
                        .replays
                        .rescan(|| Some(replays::scan_replays(&replays_path)));
                    // The freshly-finished match just landed on
                    // disk — kick the stats worker so its sidebar
                    // row gets duration / round / complete info
                    // without waiting for app restart.
                    iced::Task::batch([task, self.refresh_replay_stats().map(Message::Replays)])
                } else {
                    task
                }
            }
            Message::Netplay(netplay::Message::MatchHandoffReady) => {
                // Drain the lobby-side state into a PreMatchData
                // and kick off async PvP setup. The lobby loop
                // has been cancel-signaled; spawn_pvp polls the
                // receiver-handoff slot until the loop releases
                // ownership. On success we land back in
                // Message::PvpSessionBuilt below.
                let Some(pre_match) = self.netplay.take_pre_match() else {
                    return iced::Task::none();
                };
                let scanners = self.scanners.clone();
                let config = self.config.clone();
                let audio_binder = self.audio_binder.clone();
                let local_game = self.play.local_game;
                let local_patch = self.play.local_patch.clone().zip(self.play.local_patch_version.clone());
                iced::Task::perform(
                    async move {
                        let Some(local_game) = local_game else {
                            return Err(anyhow::anyhow!("no local game selected"));
                        };
                        session::spawn_pvp(scanners, config, audio_binder, local_game, local_patch, pre_match).await
                    },
                    |result| Message::PvpSessionBuilt(std::sync::Arc::new(parking_lot::Mutex::new(Some(result)))),
                )
            }
            Message::Netplay(m) => {
                // Always resend after a netplay message too: this
                // covers the Negotiating → Lobby transition (first
                // announce) and lobby-state mutations like
                // SetMatchType / SetInputDelay. The dedupe inside
                // netplay::State::update::SendLocalSettings makes
                // unchanged dispatches a no-op.
                let task = self.netplay.update(m).map(Message::Netplay);
                iced::Task::batch([task, self.resend_settings_if_lobby()])
            }
            Message::PvpSessionBuilt(slot) => {
                let Some(result) = slot.lock().take() else {
                    return iced::Task::none();
                };
                match result {
                    Ok(session) => {
                        let has_opponent_panel = session.opponent_loaded.is_some();
                        self.session.active = Some(ActiveSession::PvP(session));
                        self.session.frame = None;
                        self.session.show_opponent_panel = has_opponent_panel;
                    }
                    Err(e) => {
                        log::error!("pvp session build failed: {e}");
                        self.play.last_error = Some(format!("{e}"));
                        // netplay state is already back to Idle.
                    }
                }
                iced::Task::none()
            }
        }
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        iced::Subscription::batch([
            session::subscription(&self.session).map(Message::Session),
            netplay::subscription(&self.netplay).map(Message::Netplay),
            tabs::settings::subscription(&self.settings).map(Message::Settings),
        ])
    }

    fn update_play(&mut self, msg: tabs::play::Message) -> iced::Task<Message> {
        let Some(effect) = self
            .play
            .update(msg, &self.scanners, &self.config, self.loaded.as_ref())
        else {
            return iced::Task::none();
        };
        use tabs::play::Effect as E;
        match effect {
            E::SelectionChanged => {
                self.refresh_loaded();
                self.persist_selection();
                iced::Task::none()
            }
            E::Rescan => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
                iced::Task::none()
            }
            E::OpenPath(p) => {
                if let Err(e) = open::that(&p) {
                    log::error!("open {}: {e}", p.display());
                }
                iced::Task::none()
            }
            E::CopyText(s) => iced::clipboard::write(s),
            E::CopyImage(img) => {
                copy_image_to_clipboard(img);
                iced::Task::none()
            }
            E::StartSinglePlayer => {
                let Some(loaded) = self.loaded.as_ref() else {
                    return iced::Task::none();
                };
                match session::spawn_singleplayer(&self.scanners, &self.config, &self.audio_binder, loaded) {
                    Ok(s) => {
                        self.session.active = Some(ActiveSession::SinglePlayer(s));
                        self.session.frame = None;
                    }
                    Err(e) => {
                        log::warn!("singleplayer start failed: {e}");
                        self.play.last_error = Some(format!("{e}"));
                    }
                }
                iced::Task::none()
            }
            E::NetplayConnect(link_code) => {
                let endpoint = self.config.matchmaking_endpoint.clone();
                self.netplay
                    .update(netplay::Message::Connect { link_code, endpoint })
                    .map(Message::Netplay)
            }
            E::Netplay(m) => self.netplay.update(m).map(Message::Netplay),
            E::NetplayReadyWithSave => {
                // View-time gating disables the Ready button when
                // no save is loaded, so this is just defense in
                // depth — fall through silently if reached.
                let Some(loaded) = self.loaded.as_ref() else {
                    return iced::Task::none();
                };
                let save_sram = loaded.save.as_sram_dump();
                self.netplay
                    .update(netplay::Message::Commit { save_sram })
                    .map(Message::Netplay)
            }
            E::SaveDuplicate => {
                if let Some(src) = self.play.local_save.clone() {
                    match duplicate_save(&src) {
                        Ok(dst) => {
                            log::info!("duplicated save: {}", dst.display());
                            self.scanners.rescan(&self.config);
                            self.play.local_save = Some(dst);
                            self.refresh_loaded();
                            self.persist_selection();
                        }
                        Err(e) => log::error!("duplicate save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::SaveRename { new_stem } => {
                if let Some(src) = self.play.local_save.clone() {
                    match rename_save(&src, &new_stem) {
                        Ok(dst) => {
                            log::info!("renamed save: {} → {}", src.display(), dst.display());
                            self.scanners.rescan(&self.config);
                            self.play.local_save = Some(dst);
                            self.refresh_loaded();
                            self.persist_selection();
                        }
                        Err(e) => log::error!("rename save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::SaveDelete => {
                if let Some(src) = self.play.local_save.clone() {
                    if let Err(e) = std::fs::remove_file(&src) {
                        log::error!("delete save: {e}");
                    } else {
                        log::info!("deleted save: {}", src.display());
                    }
                    self.scanners.rescan(&self.config);
                    self.play.local_save = self.play.local_game.and_then(|g| {
                        self.scanners
                            .saves
                            .read()
                            .get(&g)
                            .and_then(|v| v.first().map(|s| s.path.clone()))
                    });
                    self.refresh_loaded();
                    self.persist_selection();
                }
                iced::Task::none()
            }
            E::SaveNew { name, template } => {
                if let Some(game) = self.play.local_game {
                    if let Some(templates) = tabs::play::templates_for_selection_public(&self.play, &self.scanners) {
                        // Use the chosen template name; fall back
                        // to default ("") then first available.
                        let chosen = templates
                            .get(template.as_str())
                            .or_else(|| templates.get(""))
                            .or_else(|| templates.values().next())
                            .map(|s| s.clone_box());
                        if let Some(template) = chosen {
                            match create_new_save(&self.config.saves_path(), &name, template.as_ref()) {
                                Ok(dst) => {
                                    log::info!(
                                        "created new save for {:?}: {}",
                                        game.family_and_variant(),
                                        dst.display()
                                    );
                                    self.scanners.rescan(&self.config);
                                    self.play.local_save = Some(dst);
                                    self.refresh_loaded();
                                    self.persist_selection();
                                }
                                Err(e) => log::error!("create save: {e}"),
                            }
                        }
                    }
                }
                iced::Task::none()
            }
        }
    }

    fn update_patches(&mut self, msg: tabs::patches::Message) -> iced::Task<tabs::patches::Message> {
        let Some(effect) = self.patches.update(msg, &self.scanners, &self.config) else {
            return iced::Task::none();
        };
        use tabs::patches::Effect as E;
        match effect {
            E::OpenPath(s) => {
                if let Err(e) = open::that(&s) {
                    log::error!("open {s}: {e}");
                }
                iced::Task::none()
            }
            E::Rescan => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
                iced::Task::none()
            }
            E::UpdateRescan => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
                iced::Task::none()
            }
            E::StartUpdate { url, root } => iced::Task::perform(
                async move { patch::update(url, root).await.map_err(|e| e.to_string()) },
                tabs::patches::Message::UpdateFinished,
            ),
        }
    }

    fn update_replays(&mut self, msg: tabs::replays::Message) -> iced::Task<tabs::replays::Message> {
        // Pure state mutations live in the tab module; only side
        // effects (clipboard, OS open, session host handoff,
        // file dialog, export task spawn) come back here as an
        // Effect for the App to interpret.
        let Some(effect) = self.replays.update(msg, &self.scanners, &self.config) else {
            return iced::Task::none();
        };
        use tabs::replays::Effect as E;
        match effect {
            E::OpenPath(p) => {
                if let Err(e) = open::that(&p) {
                    log::error!("open {}: {e}", p.display());
                }
                iced::Task::none()
            }
            E::Watch(p) => {
                match session::build_playback(&self.scanners, &self.config, &self.audio_binder, &p) {
                    Ok(s) => {
                        self.session.active = Some(ActiveSession::Replay(s));
                        self.session.frame = None;
                    }
                    Err(e) => log::warn!("failed to play replay {}: {e}", p.display()),
                }
                iced::Task::none()
            }
            E::Rescan => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
                // User triggered a full rescan — re-validate the
                // stats cache and warm it for any new replays.
                self.refresh_replay_stats()
            }
            E::CopyText(s) => iced::clipboard::write(s),
            E::CopyImage(img) => {
                copy_image_to_clipboard(img);
                iced::Task::none()
            }
            E::OpenExportSaveDialog(replay_path) => {
                let default_name = replay_path
                    .file_stem()
                    .map(|s| format!("{}.mp4", s.to_string_lossy()))
                    .unwrap_or_else(|| "replay.mp4".to_string());
                let initial_dir = replay_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| self.config.replays_path());
                let replay_for_msg = replay_path;
                iced::Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_directory(&initial_dir)
                            .set_file_name(&default_name)
                            .add_filter("MP4", &["mp4"])
                            .save_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    move |maybe_path| match maybe_path {
                        Some(output) => tabs::replays::Message::ExportStart {
                            replay: replay_for_msg.clone(),
                            output,
                        },
                        // User cancelled — no-op. Dismiss on a
                        // path that never had a job entry is a
                        // safe HashMap remove; status untouched.
                        None => tabs::replays::Message::ExportDismiss(replay_for_msg.clone()),
                    },
                )
            }
            E::StartExport {
                replay,
                output,
                settings,
                rounds,
            } => self.spawn_replay_export(replay, output, settings, rounds),
        }
    }

    /// Spawn the tango_pvp::replay::export task with a progress
    /// callback that forwards into the replays-tab message
    /// stream. The user-picked output path + form snapshot come
    /// from the tab module's `ExportStart` effect.
    fn spawn_replay_export(
        &mut self,
        replay_path: std::path::PathBuf,
        output_path: std::path::PathBuf,
        user_settings: tabs::replays::ExportSettings,
        rounds_mask: Vec<bool>,
    ) -> iced::Task<tabs::replays::Message> {
        // Decode just enough of the replay to get the local side's
        // metadata + hook lookups + raw ROM bytes. Failures show up
        // as a Done(Err) status — same as runtime errors below.
        let prep = (|| -> anyhow::Result<ExportPrep> {
            let f = std::fs::File::open(&replay_path)?;
            let replay = tango_pvp::replay::Replay::decode(f)?;
            let resolve = |side: Option<&tango_pvp::replay::metadata::Side>| -> anyhow::Result<(
                &'static (dyn tango_pvp::hooks::Hooks + Send + Sync),
                Vec<u8>,
            )> {
                let gi = side
                    .and_then(|s| s.game_info.as_ref())
                    .ok_or_else(|| anyhow::anyhow!("replay side missing game info"))?;
                let variant = u8::try_from(gi.rom_variant)?;
                let entry = tango_gamedb::find_by_family_and_variant(&gi.rom_family, variant)
                    .ok_or_else(|| {
                        anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, variant)
                    })?;
                let hooks = tango_pvp::hooks::hooks_for_gamedb_entry(entry)
                    .ok_or_else(|| anyhow::anyhow!("no hooks for {:?}", entry.family_and_variant()))?;
                let rom = self
                    .scanners
                    .roms
                    .read()
                    .get(&entry)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("rom for {:?} not scanned", entry.family_and_variant()))?;
                Ok((hooks, rom))
            };
            let (local_hooks, local_rom) = resolve(replay.metadata.local_side.as_ref())?;
            let (remote_hooks, remote_rom) = resolve(replay.metadata.remote_side.as_ref())?;
            Ok(ExportPrep {
                local_hooks,
                local_rom,
                remote_hooks,
                remote_rom,
                replay,
            })
        })();
        let prep = match prep {
            Ok(p) => p,
            Err(e) => {
                self.replays.per.entry(replay_path).or_default().job = Some(tabs::replays::ExportJob {
                    completed: 0,
                    total: 0,
                    result: Some(Err(format!("{e}"))),
                });
                return iced::Task::none();
            }
        };

        if !rounds_mask.iter().any(|b| *b) {
            self.replays.per.entry(replay_path).or_default().job = Some(tabs::replays::ExportJob {
                completed: 0,
                total: 0,
                result: Some(Err("no rounds selected for export".to_string())),
            });
            return iced::Task::none();
        }

        let (progress_tx, progress_rx) = futures::channel::mpsc::unbounded::<(usize, usize)>();
        let done_arc: std::sync::Arc<parking_lot::Mutex<Option<Result<std::path::PathBuf, String>>>> =
            std::sync::Arc::new(parking_lot::Mutex::new(None));
        let done_arc_task = done_arc.clone();
        let output_for_task = output_path.clone();
        tokio::task::spawn(async move {
            let ExportPrep {
                local_hooks,
                local_rom,
                remote_hooks,
                remote_rom,
                replay,
            } = prep;
            // Lossless => Settings::ffmpeg_video_flags uses
            // libx264rgb -qp 0 (legacy parity); otherwise pass
            // the scale factor through to the swscale neighbor
            // filter inside default_with_scale.
            let scale_arg = if user_settings.lossless {
                None
            } else {
                Some(user_settings.scale as usize)
            };
            let mut settings = tango_pvp::replay::export::Settings::default_with_scale(scale_arg);
            settings.disable_bgm = user_settings.disable_bgm;
            let selected_rounds = vec![rounds_mask];
            let progress_tx = parking_lot::Mutex::new(progress_tx);
            let cb = |current: usize, total: usize| {
                let _ = progress_tx.lock().unbounded_send((current, total));
            };
            let result = tango_pvp::replay::export::export(
                &local_rom,
                local_hooks,
                &remote_rom,
                remote_hooks,
                &[replay],
                &selected_rounds,
                &output_for_task,
                &settings,
                cb,
            )
            .await
            .map(|()| output_for_task)
            .map_err(|e| format!("{e}"));
            *done_arc_task.lock() = Some(result);
        });

        // Drain progress + a synthetic final ExportFinished from
        // the same stream. We poll done_arc whenever the channel
        // drains so the finished message arrives even if the
        // export errored before sending any progress.
        let replay_for_stream = replay_path;
        let stream = futures::stream::unfold(
            (progress_rx, done_arc, replay_for_stream, false),
            |(mut rx, done, replay, finished_sent)| async move {
                use futures::StreamExt;
                if finished_sent {
                    return None;
                }
                tokio::select! {
                    biased;
                    next = rx.next() => match next {
                        Some((c, t)) => Some((
                            tabs::replays::Message::ExportProgress {
                                replay: replay.clone(),
                                completed: c,
                                total: t,
                            },
                            (rx, done, replay, false),
                        )),
                        None => {
                            // Channel closed — the task is done.
                            // Pull the result out of done_arc.
                            let r = done.lock().take().unwrap_or_else(|| {
                                Err("export task ended without result".to_string())
                            });
                            Some((
                                tabs::replays::Message::ExportFinished {
                                    replay: replay.clone(),
                                    result: r,
                                },
                                (rx, done, replay, true),
                            ))
                        }
                    }
                }
            },
        );
        iced::Task::stream(stream)
    }

    fn update_settings(&mut self, msg: tabs::settings::Message) -> iced::Task<tabs::settings::Message> {
        use tabs::settings::ConfigChange as C;
        let Some(change) = self.settings.update(msg) else {
            return iced::Task::none();
        };
        match change {
            C::Language(l) => self.config.language = l,
            C::Nickname(s) => self.config.nickname = if s.is_empty() { None } else { Some(s) },
            C::StreamerMode(b) => self.config.streamer_mode = b,
            C::MatchmakingEndpoint(s) => self.config.matchmaking_endpoint = s,
            C::PatchRepo(s) => self.config.patch_repo = s,
            C::Theme(t) => self.config.theme = t,
            C::AddInputBinding(slot, binding) => {
                let bindings = self.config.input_mapping.slot_mut(slot);
                // Avoid dupes — a single binding could be added
                // twice if the user hits the same key fast.
                if !bindings.contains(&binding) {
                    bindings.push(binding);
                }
            }
            C::RemoveInputBinding(slot, idx) => {
                let bindings = self.config.input_mapping.slot_mut(slot);
                if idx < bindings.len() {
                    bindings.remove(idx);
                }
            }
            C::ResetInputBindings => {
                self.config.input_mapping = input::Mapping::default();
            }
        }
        self.persist_config();
        iced::Task::none()
    }

    fn update_welcome(&mut self, msg: tabs::welcome::Message) -> iced::Task<tabs::welcome::Message> {
        use tabs::welcome::Message as M;
        match msg {
            M::NicknameChanged(s) => {
                self.welcome.nickname_draft = s;
            }
            M::Continue => {
                if let Some(nickname) = self.welcome.finalize_nickname() {
                    self.config.nickname = Some(nickname);
                    self.persist_config();
                }
            }
            M::LanguageSelected(l) => {
                self.config.language = l;
                self.persist_config();
            }
            M::OpenRomsFolder => {
                let p = self.config.roms_path();
                let _ = std::fs::create_dir_all(&p);
                if let Err(e) = open::that(&p) {
                    log::error!("open roms folder: {e}");
                }
            }
            M::RescanRoms => {
                self.scanners.rescan(&self.config);
                self.refresh_loaded();
            }
        }
        iced::Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let lang = &self.config.language;

        // First-run gate: no main UI until the user picks a nickname.
        if self.config.nickname.is_none() {
            let roms_count = self.scanners.roms.read().len();
            return tabs::welcome::view(lang, &self.welcome, roms_count, &self.config.roms_path())
                .map(Message::Welcome);
        }

        if self.session.is_active() {
            return session::view(lang, &self.session).map(Message::Session);
        }

        let body: Element<'_, Message> = match self.tab {
            Tab::Play => self
                .play
                .view(
                    lang,
                    &self.scanners,
                    self.loaded.as_ref(),
                    self.config.streamer_mode,
                    &self.config,
                    &self.netplay.phase,
                    &self.netplay.lobby,
                )
                .map(Message::Play),
            Tab::Replays => self
                .replays
                .view(lang, &self.scanners, &self.config)
                .map(Message::Replays),
            Tab::Patches => self.patches.view(lang, &self.scanners).map(Message::Patches),
            Tab::Settings => tabs::settings::view(lang, &self.config, &self.settings).map(Message::Settings),
        };

        column![top_bar(lang, self.tab), horizontal_rule(1), body]
            .spacing(0)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn theme(&self) -> Theme {
        // Custom palettes derived from the built-in Light/Dark, with the
        // accent (primary) swapped to the BN-green that the main egui
        // app uses for selection / accents.
        const TANGO_GREEN: iced::Color =
            iced::Color::from_rgb(0x4c as f32 / 255.0, 0xaf as f32 / 255.0, 0x50 as f32 / 255.0);
        match self.config.theme {
            config::ThemeMode::Light => Theme::custom(
                "Tango Light".to_string(),
                iced::theme::Palette {
                    primary: TANGO_GREEN,
                    ..iced::theme::Palette::LIGHT
                },
            ),
            config::ThemeMode::Dark => Theme::custom(
                "Tango Dark".to_string(),
                iced::theme::Palette {
                    primary: TANGO_GREEN,
                    ..iced::theme::Palette::DARK
                },
            ),
        }
    }
}

fn top_bar(lang: &LanguageIdentifier, active: Tab) -> Element<'_, Message> {
    use lucide_icons::Icon;
    let tab =
        |icon, label, target: Tab| widgets::tab_button(icon, label, Message::TabSelected(target), target == active);
    container(
        row![
            tab(Icon::Gamepad, t(lang, "tab-play"), Tab::Play),
            tab(Icon::Film, t(lang, "tab-replays"), Tab::Replays),
            tab(Icon::Puzzle, t(lang, "tab-patches"), Tab::Patches),
            horizontal_space(),
            // Settings = low-emphasis utility tab. The gear glyph
            // is already an interface convention, so the "Settings"
            // text would be redundant; expose it as a hover
            // tooltip instead.
            widgets::icon_tab_button(
                Icon::Settings,
                t(lang, "tab-settings"),
                Message::TabSelected(Tab::Settings),
                Tab::Settings == active,
            ),
        ]
        .spacing(2)
        .align_y(Alignment::End)
        .padding([4, 6]),
    )
    .width(Fill)
    .into()
}
