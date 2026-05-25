//! Top-level `App` struct + iced glue. Split out of `main.rs`
//! so the bootstrap layer (supervisor + run_app + window setup)
//! stays small. The shape of an iced 0.14 app is:
//!
//!   * `App::new`        constructor used by `iced::application`
//!   * `App::title`      window title (live)
//!   * `App::update`     reducer for `Message`
//!   * `App::subscription` outside-the-app event streams
//!   * `App::view`       renderer
//!   * `App::theme`      live `iced::Theme`
//!
//! Per-tab `update_*` helpers fan out from `App::update`; per-tab
//! `view` modules render the tab body, which `App::view` chooses
//! between based on `self.tab`.

use crate::session::ActiveSession;
use crate::theme::theme_for;
use crate::{
    audio, config, discord, game, i18n, input, net, netplay, patch, pvp_session, replays, rom, save, selection,
    session, tabs, updater, widgets, INIT_LINK_CODE,
};
use i18n::t;
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{column, container, row};
use iced::{Alignment, Element, Fill, Theme};
use tabs::patches::PatchesState;
use tabs::play::{create_new_save, duplicate_save, rename_save, PlayState};
use tabs::replays::ReplaysState;
use unic_langid::LanguageIdentifier;

// Button sizing constants. `PRIMARY` is the big call-to-action
// (Play); `STANDARD` is everything else. Standard body text comes
// from iced's `default_text_size` (set in `run_app`), so there's
// no standalone STANDARD_TEXT_SIZE constant — widgets that don't
// pass an explicit size inherit the app default.
pub const PRIMARY_PADDING: [f32; 2] = [6.0, 14.0];
pub const STANDARD_PADDING: [f32; 2] = [6.0, 14.0];

/// Pinned inner-control height for the play-tab link-code bar
/// and the session media-controls bar — every button / picker
/// in both strips is sized to this so the bars come out the
/// same height naturally (no outer container pinning needed).
pub const BAR_CONTROL_HEIGHT: f32 = 40.0;

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

pub struct App {
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

    /// Discord rich-presence client (background tokio task auto-
    /// reconnects). Activity is pushed once per second via the
    /// `DiscordTick` subscription, plus on session start/end.
    discord: discord::Client,
    /// Wall-clock when the current session was first observed
    /// active — used as the `start_time` for the
    /// `make_single_player_activity` / `make_in_progress_activity`
    /// timestamps. Reset to `None` when the session ends.
    session_started_at: Option<std::time::SystemTime>,
    /// Background loop that pulls the patch repo every 15 min
    /// and refreshes the patches scanner in place.
    patch_autoupdater: patch::Autoupdater,
    /// Self-updater. Polls GitHub every 30 min, streams the
    /// platform installer into the cache dir, and on the
    /// `finish_update` call (or next launch) hands off to the
    /// installer. UI lives in Settings → About; toggle is in
    /// Settings → Network.
    updater: updater::Updater,
}

impl App {
    pub fn new() -> (Self, iced::Task<Message>) {
        let config = config::Config::load_or_create();
        let _ = i18n::FALLBACK_LANG; // re-exported for use in config; suppress unused warning here

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
                    // Save restore happens after patch+version so the per-
                    // (game, patch, version) memory key resolves correctly.
                    let key =
                        config::save_memory_key(game, play.local_patch.as_deref(), play.local_patch_version.as_ref());
                    if let Some(rel) = config.last_save_per_game_per_patch.get(&key) {
                        let abs = config.data_relative_to_absolute(rel);
                        if scanners
                            .saves
                            .read()
                            .get(&game)
                            .map(|v| v.iter().any(|s| s.path == abs))
                            .unwrap_or(false)
                        {
                            play.local_save = Some(abs);
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
        audio_binder.set_volume(config.volume);
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

        let mut patch_autoupdater = patch::Autoupdater::new(
            config.patches_path(),
            config.patch_repo.clone(),
            scanners.patches.clone(),
        );
        if config.enable_patch_autoupdate {
            patch_autoupdater.start();
        }

        // Self-updater. Cache dir must exist before the
        // download stream tries to write into it.
        let updater_cache = updater::updater_cache_dir(&config);
        let _ = std::fs::create_dir_all(&updater_cache);
        let mut updater = updater::Updater::new(&updater_cache, config.allow_prerelease_upgrades);
        // Apply any installer left over from a previous
        // session BEFORE the UI comes up — if it succeeds,
        // do_update exits the process here.
        updater.finish_update();
        if config.enable_updater {
            updater.set_enabled(true);
        }

        // CLI `Join <code>` (or Discord deep-link routed through
        // the same channel) lands here — prefill the link code
        // and start on the Play tab so the user can hit Fight.
        let init_link_code = INIT_LINK_CODE.get().and_then(|c| c.clone());
        let mut starting_tab = Tab::default();
        if let Some(code) = &init_link_code {
            play.link_code = code.clone();
            starting_tab = Tab::Play;
        }

        let mut app = Self {
            config,
            tab: starting_tab,
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
            discord: discord::Client::new(),
            session_started_at: None,
            patch_autoupdater,
            updater,
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
        let live: std::collections::HashSet<std::path::PathBuf> =
            self.scanners.replays.read().iter().map(|r| r.path.clone()).collect();
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
            .filter_map(|(path, stats)| async move { stats.map(|s| tabs::replays::Message::StatsLoaded(path, s)) });
        iced::Task::stream(stream)
    }

    /// Persist `self.config` to disk. Failures are logged but otherwise
    /// swallowed so a transient write error doesn't crash the UI.
    fn persist_config(&self) {
        if let Err(e) = self.config.save() {
            log::error!("failed to save config: {e}");
        }
    }

    /// Record the current selection back to config; called after any
    /// selection change so the next launch restores it. Save paths are
    /// stored relative to `data_path` under a per-(game, patch,
    /// version) key, so switching back to any prior combination
    /// restores its save.
    fn persist_selection(&mut self) {
        self.config.last_game = self
            .play
            .local_game
            .map(|g| (g.family_and_variant().0.to_string(), g.family_and_variant().1));
        self.config.last_patch = self.play.local_patch.clone();
        self.config.last_patch_version = self.play.local_patch_version.clone();
        if let (Some(g), Some(p)) = (self.play.local_game, self.play.local_save.as_ref()) {
            if let Some(rel) = self.config.data_relative_string(p) {
                let key = config::save_memory_key(
                    g,
                    self.play.local_patch.as_deref(),
                    self.play.local_patch_version.as_ref(),
                );
                self.config.last_save_per_game_per_patch.insert(key, rel);
            }
        }
        self.persist_config();
    }

    /// Snapshot of the inputs that determine `loaded`, used to skip
    /// rebuilds when nothing relevant changed.
    /// Build the current Settings packet + dispatch SendLocalSettings
    /// Default match-type policy:
    ///   - Game JUST changed (or first selection in this lobby):
    ///     pick Triple (mode=1) if the game supports it, else
    ///     Single. This is the "default to triple" the user wants
    ///     — keyed off `default_mt_for_game` so it only fires once
    ///     per (lobby, game) pair.
    ///   - Same game, current value invalid for it: same fallback
    ///     (paranoia).
    ///   - Same game, valid value: leave alone — sticky user pick.
    ///
    /// Called any time the current game or lobby state could have
    /// changed in a way that affects the right default: on Connect
    /// (cancel_and_renew wiped the lobby), on selection change,
    /// and defensively inside `resend_settings_if_lobby`.
    fn apply_default_match_type(&mut self) {
        let Some(game) = self.play.local_game else { return };
        let mt_table = game::from_gamedb_entry(game).map(|g| g.match_types).unwrap_or(&[]);
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

    /// — only meaningful while netplay is in Lobby phase; outside
    /// that this returns `Task::none()`. Wrapped in a helper because
    /// it has three callers: lobby entry, selection change, and
    /// match-type change.
    fn resend_settings_if_lobby(&mut self) -> iced::Task<Message> {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        self.apply_default_match_type();
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
        self.play
            .make_local_settings(&self.config, &self.netplay.lobby, &self.scanners)
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
            // Save was deleted out from under us (e.g. user deleted
            // it then hit Rescan). Drop the stale selection so the
            // picker stops showing a missing entry.
            self.loaded = None;
            drop(saves);
            drop(roms);
            drop(patches);
            self.play.local_save = None;
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
    /// No-op message — used by overlay layers (e.g. the
    /// settings-modal panel itself) to swallow clicks without
    /// triggering any state change.
    NoOp,
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
    /// 1 Hz tick: refresh Discord rich-presence + drain any
    /// Discord-initiated join secret into the play link-code
    /// field.
    DiscordTick,
    /// Raw window event (resize, move, etc.). Filtered in the
    /// handler — only Resized currently triggers anything.
    Window(iced::window::Id, iced::window::Event),
    /// Result of an `iced::window::get_maximized` task spawned
    /// after a Resized event. Carries the resize-time size so the
    /// handler can decide whether to persist it (only if the
    /// window isn't maximized).
    WindowMaximizedQueried {
        size: iced::Size,
        maximized: bool,
    },
}

impl App {
    pub fn title(&self) -> String {
        t!(&self.config.language, "window-title")
    }

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::NoOp => iced::Task::none(),
            Message::TabSelected(t) => {
                self.tab = t;
                iced::Task::none()
            }
            // FightPressed branches to the netplay path when the user
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
            Message::DiscordTick => {
                self.handle_discord_tick();
                iced::Task::none()
            }
            Message::Window(id, ev) => {
                if let iced::window::Event::Resized(size) = ev {
                    // The Resized size could be either a user-driven
                    // resize or the result of maximize/unmaximize.
                    // We need is_maximized to decide whether to keep
                    // it as the restore size, so query it and finish
                    // the bookkeeping in WindowMaximizedQueried.
                    return iced::window::is_maximized(id)
                        .map(move |maximized| Message::WindowMaximizedQueried { size, maximized });
                }
                iced::Task::none()
            }
            Message::WindowMaximizedQueried { size, maximized } => {
                if !maximized {
                    self.config.last_window_size = Some((size.width, size.height));
                }
                self.config.last_window_maximized = maximized;
                self.persist_config();
                iced::Task::none()
            }
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
                // Snapshot "was PvP" before dispatch — PvP
                // sessions can auto-tear-down inside
                // `UpdateFramebuffer` (peer-end / disconnect /
                // grace timeout), not just from a Close message.
                // We trigger the replay rescan whenever a PvP
                // session was active before and isn't after.
                let was_pvp = matches!(self.session.active, Some(ActiveSession::PvP(_)));
                let task = self
                    .session
                    .update(m, &self.config.input_mapping, &self.config.video_filter)
                    .map(Message::Session);
                if sp_closing {
                    let saves_path = self.config.saves_path();
                    self.scanners.saves.rescan(|| Some(save::scan_saves(&saves_path)));
                    // Bypass refresh_loaded's same-key dedupe —
                    // the path + game haven't changed, only the
                    // bytes have.
                    self.loaded = None;
                    self.refresh_loaded();
                }
                // PvP sessions write a `.tangoreplay` next to
                // the saves dir on match end; once the session
                // clears we want the new file to show up in the
                // Replays tab without a manual rescan.
                let pvp_closed = was_pvp && self.session.active.is_none();
                if pvp_closed {
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
                let frame_notify = self.session.frame_notify.clone();
                let vbuf = self.session.vbuf.clone();
                let local_game = self.play.local_game;
                let local_patch = self.play.local_patch.clone().zip(self.play.local_patch_version.clone());
                iced::Task::perform(
                    async move {
                        let Some(local_game) = local_game else {
                            return Err(anyhow::anyhow!("no local game selected"));
                        };
                        session::spawn_pvp(
                            scanners,
                            config,
                            audio_binder,
                            frame_notify,
                            vbuf,
                            local_game,
                            local_patch,
                            pre_match,
                        )
                        .await
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
                let was_lobby = matches!(self.netplay.phase, netplay::Phase::Lobby { .. });
                let task = self.netplay.update(m).map(Message::Netplay);
                let became_lobby = !was_lobby && matches!(self.netplay.phase, netplay::Phase::Lobby { .. });
                // Opponent just completed the handshake — flash the
                // taskbar / bounce the dock so the lobby host
                // notices even if Tango isn't focused. No-op if the
                // window is already focused (per iced docs).
                let attention = if became_lobby {
                    iced::window::latest().and_then(|id| {
                        iced::window::request_user_attention(id, Some(iced::window::UserAttention::Critical))
                    })
                } else {
                    iced::Task::none()
                };
                iced::Task::batch([task, self.resend_settings_if_lobby(), attention])
            }
            Message::PvpSessionBuilt(slot) => {
                let Some(result) = slot.lock().take() else {
                    return iced::Task::none();
                };
                match result {
                    Ok(session) => {
                        let has_opponent_panel = session.opponent_loaded.is_some();
                        self.session.active = Some(ActiveSession::PvP(session));
                        self.session.show_opponent_panel = has_opponent_panel;
                    }
                    Err(e) => {
                        log::error!("pvp session build failed: {e}");
                        self.play.last_error = Some(format!("{e}"));
                    }
                }
                // Drop the post-handoff lobby snapshot now that the
                // PvP view (or the error banner) is taking over the
                // screen. take_pre_match deliberately left it in
                // place so the bottom strip didn't flash blank
                // while spawn_pvp ran.
                self.netplay.finish_handoff();
                iced::Task::none()
            }
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        iced::Subscription::batch([
            session::subscription(&self.session).map(Message::Session),
            netplay::subscription(&self.netplay).map(Message::Netplay),
            // 1 Hz Discord refresh — cheap (compares activity for
            // equality before re-sending) and gives us the join-
            // secret pickup loop too.
            iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::DiscordTick),
            // Window events drive the geometry-persistence loop.
            iced::window::events().map(|(id, ev)| Message::Window(id, ev)),
        ])
    }

    /// Refresh Discord rich-presence + drain any Discord-initiated
    /// join secret. Called from the 1 Hz tick.
    fn handle_discord_tick(&mut self) {
        // Stamp / clear the session-start wall clock based on
        // whether a session is currently active.
        match (&self.session.active, &self.session_started_at) {
            (Some(_), None) => self.session_started_at = Some(std::time::SystemTime::now()),
            (None, Some(_)) => self.session_started_at = None,
            _ => {}
        }

        // Discord "Join Game" handoff: the peer accepted our
        // invite, Discord handed us their link code as the join
        // secret. Drop it into the play tab + jump to it.
        if self.discord.has_current_join_secret() {
            if let Some(secret) = self.discord.take_current_join_secret() {
                log::info!("discord: accepted join with link code");
                self.play.link_code = secret;
                self.tab = Tab::Play;
            }
        }

        let activity = self.derive_discord_activity();
        self.discord.set_current_activity(Some(activity));
    }

    /// Derive the current Discord activity from app state. Maps
    /// roughly:
    ///   * PvP session active  → make_in_progress_activity
    ///   * Single-player active → make_single_player_activity
    ///   * Replay active        → make_base_activity(None)
    ///   * Netplay lobby (both peers connected) → in_lobby
    ///   * Netplay connecting/negotiating       → looking
    ///   * Otherwise → make_base_activity(current game info)
    fn derive_discord_activity(&self) -> discord::activity::Activity {
        let lang = &self.config.language;
        let game_info = self.play.local_game.map(|g| {
            let patch = self
                .play
                .local_patch
                .as_ref()
                .zip(self.play.local_patch_version.as_ref())
                .map(|(n, v)| (n.as_str(), v));
            discord::make_game_info(g, patch, lang)
        });

        if let Some(active) = &self.session.active {
            let start = self.session_started_at.unwrap_or_else(std::time::SystemTime::now);
            return match active {
                ActiveSession::Replay(_) => discord::make_base_activity(None),
                ActiveSession::SinglePlayer(_) => discord::make_single_player_activity(start, lang, game_info),
                ActiveSession::PvP(_) => discord::make_in_progress_activity(start, lang, game_info),
            };
        }

        match &self.netplay.phase {
            netplay::Phase::Lobby { ident } => discord::make_in_lobby_activity(ident, lang, game_info),
            netplay::Phase::Connecting { ident, .. } | netplay::Phase::Negotiating { ident } => {
                discord::make_looking_activity(ident, lang, game_info)
            }
            netplay::Phase::Idle | netplay::Phase::Failed { .. } => discord::make_base_activity(game_info),
        }
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
                // Game might have just changed — if so, the lobby
                // picker should show this game's default match
                // type (Triple where supported) instead of the
                // last game's pick.
                self.apply_default_match_type();
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
                match session::spawn_singleplayer(
                    &self.scanners,
                    &self.config,
                    &self.audio_binder,
                    self.session.frame_notify.clone(),
                    self.session.vbuf.clone(),
                    loaded,
                ) {
                    Ok(s) => {
                        self.session.active = Some(ActiveSession::SinglePlayer(s));
                    }
                    Err(e) => {
                        log::warn!("singleplayer start failed: {e}");
                        self.play.last_error = Some(format!("{e}"));
                    }
                }
                iced::Task::none()
            }
            E::NetplayConnect(ident) => {
                let msg = match ident {
                    netplay::LinkIdent::Matchmaking(link_code) => netplay::Message::Connect {
                        link_code,
                        endpoint: self.config.matchmaking_endpoint.clone(),
                    },
                    netplay::LinkIdent::Direct(role) => netplay::Message::ConnectDirect { role },
                };
                let task = self.netplay.update(msg).map(Message::Netplay);
                // Connect wipes lobby state — re-apply the
                // default-MT policy now so the picker shows the
                // right value from the moment the waiting screen
                // appears, instead of flickering to Triple later
                // when the first Lobby-phase resend runs.
                self.apply_default_match_type();
                task
            }
            E::Netplay(m) => {
                // An explicit user pick of match type pre-Lobby
                // would otherwise be clobbered the first time
                // `resend_settings_if_lobby` runs in Lobby —
                // that helper's "default to Triple" policy
                // fires whenever `default_mt_for_game` doesn't
                // match the current game, which is the case
                // when the user picked their match type before
                // any default was applied. Stamp the slot here
                // so the policy treats the pick as already
                // having defaulted for this game.
                if let netplay::Message::SetMatchType(_) = &m {
                    if let Some(g) = self.play.local_game {
                        let (fam, var) = g.family_and_variant();
                        self.netplay.lobby.default_mt_for_game = Some((fam.to_string(), var));
                    }
                }
                self.netplay.update(m).map(Message::Netplay)
            }
            E::NetplayReadyWithSave => {
                // View-time gating disables the Ready button when
                // no save is loaded, so this is just defense in
                // depth — fall through silently if reached.
                let Some(loaded) = self.loaded.as_ref() else {
                    return iced::Task::none();
                };
                let save_sram = loaded.save.to_sram_dump();
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
            E::SaveViewTask(t) => t.map(Message::Play),
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
            E::ToggleFavorite(name) => {
                if !self.config.favorite_patches.remove(&name) {
                    self.config.favorite_patches.insert(name);
                }
                self.persist_config();
                iced::Task::none()
            }
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
                match session::build_playback(
                    &self.scanners,
                    &self.config,
                    &self.audio_binder,
                    self.session.frame_notify.clone(),
                    self.session.vbuf.clone(),
                    &p,
                ) {
                    Ok(s) => {
                        self.session.active = Some(ActiveSession::Replay(s));
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
                        // User dismissed the dialog without picking — keep
                        // the form open and untouched. ExportDismiss would
                        // also close the panel, which is wrong here since
                        // no job ever started.
                        None => tabs::replays::Message::NoOp,
                    },
                )
            }
            E::StartExport {
                replay,
                output,
                settings,
                rounds,
            } => self.spawn_replay_export(replay, output, settings, rounds),
            E::SaveViewTask(t) => t,
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
                let mut job = tabs::replays::ExportJob::new(output_path.clone());
                job.result = Some(Err(format!("{e}")));
                self.replays.per.entry(replay_path).or_default().job = Some(job);
                return iced::Task::none();
            }
        };

        if !rounds_mask.iter().any(|b| *b) {
            let mut job = tabs::replays::ExportJob::new(output_path.clone());
            job.result = Some(Err("no rounds selected for export".to_string()));
            self.replays.per.entry(replay_path).or_default().job = Some(job);
            return iced::Task::none();
        }

        let (progress_tx, progress_rx) = futures::channel::mpsc::unbounded::<(usize, usize)>();
        let done_arc: std::sync::Arc<parking_lot::Mutex<Option<Result<std::path::PathBuf, String>>>> =
            std::sync::Arc::new(parking_lot::Mutex::new(None));
        let done_arc_thread = done_arc.clone();
        let output_for_thread = output_path.clone();
        // The ExportJob the tab module created in `ExportStart` already
        // owns the canceller. Clone it for the thread; the tab's
        // Cancel button calls `kill()` on its copy.
        let canceller_thread = self
            .replays
            .per
            .get(&replay_path)
            .and_then(|e| e.job.as_ref())
            .map(|j| j.canceller.clone())
            .unwrap_or_default();
        // Run the export on a dedicated OS thread. The export is fully
        // synchronous (std::process ffmpeg subprocesses, no async), so
        // it lives entirely outside the iced/tokio worker pool — no
        // shared-runtime starvation regardless of how tight the
        // export inner loop runs.
        std::thread::Builder::new()
            .name("replay-export".to_string())
            .spawn(move || {
                let ExportPrep {
                    local_hooks,
                    local_rom,
                    remote_hooks,
                    remote_rom,
                    replay,
                } = prep;
                // scale == 0 is the slider's lossless stop → libx264rgb
                // -qp 0 (RGB-domain lossless); 1..=10 → libx264 + nearest
                // upscale at that factor. `default_with_scale` builds the
                // ffmpeg flags accordingly.
                let scale_arg = if user_settings.scale == 0 {
                    None
                } else {
                    Some(user_settings.scale as usize)
                };
                let mut settings = tango_pvp::replay::export::Settings::default_with_scale(scale_arg);
                settings.disable_bgm = user_settings.disable_bgm;
                let selected_rounds = vec![rounds_mask];
                // Clone the sender into the callback. The original
                // `progress_tx` stays alive on the thread scope until
                // *after* `done_arc_thread` is set; otherwise the
                // futures channel closes the moment `cb` (and thus the
                // moved sender) is dropped, the iced stream wakes up,
                // sees `None`, races to read `done_arc` while it's
                // still unset, and reports "export task ended without
                // result".
                let cb_tx = progress_tx.clone();
                let cb = move |current: usize, total: usize| {
                    let _ = cb_tx.unbounded_send((current, total));
                };
                let result = if user_settings.twosided {
                    tango_pvp::replay::export::export_twosided(
                        &local_rom,
                        local_hooks,
                        &remote_rom,
                        remote_hooks,
                        &[replay],
                        &selected_rounds,
                        &output_for_thread,
                        &settings,
                        &canceller_thread,
                        cb,
                    )
                } else {
                    tango_pvp::replay::export::export(
                        &local_rom,
                        local_hooks,
                        &remote_rom,
                        remote_hooks,
                        &[replay],
                        &selected_rounds,
                        &output_for_thread,
                        &settings,
                        &canceller_thread,
                        cb,
                    )
                }
                .map(|()| output_for_thread)
                .map_err(|e| format!("{e}"));
                *done_arc_thread.lock() = Some(result);
                // `progress_tx` drops here, closing the channel, which
                // signals the iced stream to read `done_arc` — which is
                // now safely set above.
                drop(progress_tx);
            })
            .expect("spawn replay-export thread");

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
        // UpdateNow is a side effect (kicks the installer +
        // exits the process) not a config change; intercept
        // before delegating to settings::State::update.
        if matches!(msg, tabs::settings::Message::UpdateNow) {
            self.updater.finish_update();
            return iced::Task::none();
        }
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
            C::PatchAutoupdate(b) => {
                self.config.enable_patch_autoupdate = b;
                if b {
                    self.patch_autoupdater.start();
                } else {
                    self.patch_autoupdater.stop();
                }
            }
            C::VideoFilter(s) => self.config.video_filter = s,
            C::FractionalScaling(b) => self.config.fractional_scaling = b,
            C::HideEmulatorBorder(b) => self.config.hide_emulator_border = b,
            C::Fullscreen(b) => {
                self.config.fullscreen = b;
                self.persist_config();
                let mode = if b {
                    iced::window::Mode::Fullscreen
                } else {
                    iced::window::Mode::Windowed
                };
                return iced::window::latest()
                    .and_then(move |id| iced::window::set_mode(id, mode));
            }
            C::UiScale(s) => self.config.ui_scale = s,
            C::Resolution(w, h) => {
                // Picking a windowed resolution implies leaving
                // fullscreen — iced's Mode::Fullscreen is
                // borderless and always covers the monitor, so a
                // sub-monitor resize has no visible effect until
                // we drop back to Windowed. Do both atomically.
                let was_fullscreen = self.config.fullscreen;
                self.config.fullscreen = false;
                self.config.last_window_size = Some((w, h));
                self.persist_config();
                let size = iced::Size::new(w, h);
                return iced::window::latest().and_then(move |id| {
                    let resize = iced::window::resize(id, size);
                    if was_fullscreen {
                        iced::window::set_mode(id, iced::window::Mode::Windowed).chain(resize)
                    } else {
                        resize
                    }
                });
            }
            C::EnableUpdater(b) => {
                self.config.enable_updater = b;
                self.updater.set_enabled(b);
            }
            C::AllowPrereleaseUpgrades(b) => {
                // Sampled by Updater at start; takes effect on
                // next launch. Config change still gets
                // persisted so it survives the restart.
                self.config.allow_prerelease_upgrades = b;
            }
            C::Volume(v) => {
                let v = v.clamp(0.0, 1.0);
                self.config.volume = v;
                self.audio_binder.set_volume(v);
            }
            C::NetplayThrottler(t) => {
                // Persist + propagate to the live match (if any) so the
                // change takes effect immediately — both for future
                // rounds (factory replaced) and for the current round
                // (its throttler is swapped in-place, resetting state).
                self.config.netplay_throttler = t;
                if let Some(ActiveSession::PvP(pvp)) = &self.session.active {
                    let match_handle = pvp.match_handle();
                    let factory_now = session::throttler_factory_for(t);
                    tokio::spawn(async move {
                        if let Some(m) = match_handle.lock().await.clone() {
                            m.set_throttler_factory(factory_now, true).await;
                        }
                    });
                }
            }
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

    pub fn view(&self) -> Element<'_, Message> {
        let lang = &self.config.language;

        // First-run gate: no main UI until the user picks a nickname.
        if self.config.nickname.is_none() {
            let roms_count = self.scanners.roms.read().len();
            return tabs::welcome::view(lang, &self.welcome, roms_count, &self.config.roms_path())
                .map(Message::Welcome);
        }

        if self.session.is_active() {
            // Deliver keyboard + gamepad input through the
            // synchronous widget path so each event reaches
            // `program.update()` on the same winit iteration it
            // arrived in. Going through subscriptions would
            // round-trip through an `mpsc::try_send` and cost ~1
            // winit iteration of input lag per event.
            let session_view = session::view(
                lang,
                &self.session,
                self.config.fractional_scaling,
                self.config.hide_emulator_border,
                &self.config.video_filter,
            )
            .map(Message::Session);
            // In-session settings modal: floats centered over the
            // running session with a dimmed click-to-dismiss
            // backdrop. The emulator keeps running underneath.
            let composed: Element<'_, Message> = if self.session.show_settings {
                let body = tabs::settings::view(lang, &self.config, &self.settings, self.updater.status_blocking())
                    .map(Message::Settings);
                // Top header row carrying the X close button. The
                // close is the only affordance for dismissing the
                // modal — the backdrop is inert. Inline (not a
                // floating overlay) so the body lays out beneath.
                let close_btn = widgets::icon_button(
                    lucide_icons::Icon::X,
                    t!(lang, "playback-close"),
                    Message::Session(session::Message::CloseSettings),
                    [4.0, 8.0],
                );
                let heading = iced::widget::text(t!(lang, "tab-settings")).size(TEXT_HEADING);
                let header = iced::widget::container(
                    iced::widget::row![heading, iced::widget::space::horizontal(), close_btn]
                        .padding(iced::Padding {
                            top: 8.0,
                            right: 8.0,
                            bottom: 0.0,
                            left: 14.0,
                        })
                        .align_y(iced::Alignment::Center),
                )
                .width(Fill);
                let modal_panel = iced::widget::container(
                    iced::widget::column![header, body]
                        .spacing(0)
                        .width(Fill)
                        .height(Fill),
                )
                .width(iced::Length::Fixed(820.0))
                .height(iced::Length::Fixed(560.0))
                .style(widgets::panel);
                // Wrap the panel in a mouse_area so clicks on
                // its inert regions (background, headings) get
                // swallowed instead of falling through to the
                // dismiss-on-press backdrop layer below.
                let modal_panel_swallow = iced::widget::mouse_area(modal_panel).on_press(Message::NoOp);
                let placement = iced::widget::container(modal_panel_swallow)
                    .width(Fill)
                    .height(Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center);
                // Backdrop — dim wash that also dismisses the
                // modal on click. Captures the press so it
                // doesn't reach the session HUD beneath.
                let backdrop = iced::widget::mouse_area(
                    iced::widget::container(iced::widget::Space::new().width(Fill).height(Fill))
                        .width(Fill)
                        .height(Fill)
                        .style(|_: &iced::Theme| iced::widget::container::Style {
                            background: Some(iced::Background::Color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45))),
                            ..Default::default()
                        }),
                )
                .on_press(Message::Session(session::Message::CloseSettings));
                iced::widget::stack![
                    Element::from(session_view),
                    Element::from(backdrop),
                    Element::from(placement),
                ]
                .into()
            } else {
                session_view
            };
            return crate::input_capture::InputCapture::new(composed, |input| {
                let ev = match input {
                    crate::input_capture::Input::Keyboard(kb) => match kb {
                        iced::keyboard::Event::KeyPressed { physical_key, .. } => Some(session::InputEvent::Key {
                            physical: *physical_key,
                            pressed: true,
                        }),
                        iced::keyboard::Event::KeyReleased { physical_key, .. } => Some(session::InputEvent::Key {
                            physical: *physical_key,
                            pressed: false,
                        }),
                        _ => None,
                    },
                    crate::input_capture::Input::Gamepad(ev) => match *ev {
                        crate::gamepad::GamepadEvent::ButtonDown(b) => crate::input::GamepadButton::from_sdl3(b)
                            .map(|button| session::InputEvent::Button { button, pressed: true }),
                        crate::gamepad::GamepadEvent::ButtonUp(b) => crate::input::GamepadButton::from_sdl3(b)
                            .map(|button| session::InputEvent::Button { button, pressed: false }),
                        crate::gamepad::GamepadEvent::AxisMotion { axis, value } => {
                            Some(session::InputEvent::Axis { axis, value })
                        }
                        crate::gamepad::GamepadEvent::DeviceRemoved => Some(session::InputEvent::GamepadDisconnected),
                    },
                };
                ev.map(|ev| Message::Session(session::Message::Input(ev)))
            })
            .into();
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
                    self.netplay.handoff_pending(),
                )
                .map(Message::Play),
            Tab::Replays => self
                .replays
                .view(lang, &self.scanners, &self.config, &self.netplay.phase)
                .map(Message::Replays),
            Tab::Patches => self
                .patches
                .view(lang, &self.scanners, &self.config)
                .map(Message::Patches),
            Tab::Settings => tabs::settings::view(lang, &self.config, &self.settings, self.updater.status_blocking())
                .map(Message::Settings),
        };

        // Body container picks up the palette background and adds
        // a faint inner tint so the HUD bar visibly sits on top of
        // a "screen surface" rather than a flat sheet of pixels.
        let body_surface = container(body).width(Fill).height(Fill).style(widgets::body_surface);
        column![top_bar(lang, self.tab), widgets::hud_scanline(), body_surface,]
            .spacing(0)
            .width(Fill)
            .height(Fill)
            .into()
    }

    pub fn theme(&self) -> Theme {
        // Single source of truth — anything else that needs the
        // active palette (markdown link colors etc.) calls this
        // free fn too so we never drift.
        theme_for(&self.config)
    }

    /// Global UI scale multiplier — fed to `iced::application().scale_factor`.
    /// Sourced from the user's pick in graphics settings; multiplies on
    /// top of the OS DPI scale.
    pub fn scale_factor(&self) -> f32 {
        self.config.ui_scale
    }
}

fn top_bar(lang: &LanguageIdentifier, active: Tab) -> Element<'_, Message> {
    use iced::widget::image::{Handle, Image};
    use lucide_icons::Icon;
    use std::sync::LazyLock;

    // Small Tango logo at the left edge of the nav strip.
    // Uses `icon.png` (the standalone logo mark) — the emblem
    // image is the long About-page banner, not what we want
    // next to a button-sized tab strip. Parsed once via
    // LazyLock so the image bytes aren't re-decoded every
    // render.
    static LOGO: LazyLock<Handle> = LazyLock::new(|| {
        let raw: &'static [u8] = include_bytes!("icon.png");
        Handle::from_bytes(raw)
    });

    let tab =
        |icon, label, target: Tab| widgets::nav_tab_button(icon, label, Message::TabSelected(target), target == active);
    container(
        row![
            iced::widget::container(
                Image::new(LOGO.clone())
                    .width(iced::Length::Fixed(28.0))
                    .height(iced::Length::Fixed(28.0))
                    .content_fit(iced::ContentFit::Contain),
            )
            .padding([2, 8]),
            tab(Icon::Gamepad, t!(lang, "tab-play"), Tab::Play),
            tab(Icon::Film, t!(lang, "tab-replays"), Tab::Replays),
            tab(Icon::Puzzle, t!(lang, "tab-patches"), Tab::Patches),
            horizontal_space(),
            // Settings = low-emphasis utility tab. The gear glyph
            // is already an interface convention, so the "Settings"
            // text would be redundant; expose it as a hover
            // tooltip instead.
            widgets::nav_icon_tab_button(
                Icon::Settings,
                t!(lang, "tab-settings"),
                Message::TabSelected(Tab::Settings),
                Tab::Settings == active,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([10, 16]),
    )
    .width(Fill)
    .style(widgets::hud_bar)
    .into()
}
