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
    anim, audio, config, discord, game, i18n, input, loadout, net, netplay, patch, pvp_session, replays, rom, save,
    selection, session, tabs, updater, widgets, INIT_LINK_CODE,
};
use i18n::t;
use iced::widget::container;
use iced::widget::space::horizontal as horizontal_space;
use iced::{Alignment, Element, Fill, Theme};
use sweeten::widget::{column, mouse_area, row};
use tabs::patches::PatchesState;
use tabs::play::{create_new_save, duplicate_save, rename_save};
use tabs::replays::ReplaysState;
use unic_langid::LanguageIdentifier;

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
    /// without owning the audio backend. The sdl Backend lives in
    /// `_audio_backend` so the underlying stream keeps playing.
    audio_binder: audio::LateBinder,
    /// Kept alive for the program's lifetime; dropping it would tear
    /// down the SDL audio stream and the app would go silent.
    _audio_backend: Option<audio::sdl::Backend>,

    /// Owned game+save+assets for the current selection. Rebuilt only
    /// when game or save changes; per-frame view() borrows it.
    loaded: Option<selection::Loaded>,

    /// The local loadout (family / game / save + patch overlay) —
    /// App-level so the lobby settings-resend sees every change the
    /// Play tab's selector makes.
    loadout: loadout::Loadout,
    play: tabs::play::State,
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
    /// Number of in-flight `rescan_off_thread` tasks. Drives the
    /// per-tab Rescan button gate — `view` reads it to render a
    /// disabled rescan button while a rescan worker is still busy.
    /// A counter (not a bool) because rescans can overlap (e.g.
    /// patch autoupdater fires its own rescan separately from the
    /// user clicking the button).
    rescans_in_flight: u32,
    /// Entrance glide played on freshly-swapped content whenever
    /// the [`screen_key`] changes (tab switch, welcome → main,
    /// session start/end). Restarted at 0 → 1 on each trigger;
    /// `view` draws the new screen a few px off its rest position
    /// and slides it in, so the swap reads as the new screen
    /// arriving rather than a hard cut. (A fade was tried first,
    /// but without subtree opacity it has to blank to the
    /// background color for a frame — worse than the cut.)
    ///
    /// [`screen_key`]: App::screen_key
    screen_enter: anim::Enter,
    /// What the current `screen_enter` moves and which way — see
    /// [`EnterScope`].
    screen_enter_scope: EnterScope,
    /// Two-phase swap between the Play tab's bottom bands (link-code
    /// strip ↔ lobby) — mirrors [`App::lobby_on_screen`], synced after
    /// every update. Runs two transition lengths with a linear ramp:
    /// the view spends the first half sinking + dissolving the
    /// outgoing band into the page surface and the second half
    /// rising + condensing the incoming one out of it, so the swap
    /// reads as the code strip turning into the lobby and back.
    lobby_swap: anim::Transition,
    /// The lobby's last live (phase, lobby) pair, frozen on the
    /// frame the lobby leaves the screen. The exiting half of the
    /// band swap renders from this so the verdict (e.g. the failure
    /// banner being dismissed) holds steady through the dissolve
    /// instead of flashing to the idle handshake line.
    lobby_exit_snapshot: Option<(netplay::Phase, netplay::LobbyState)>,
}

/// See [`App::screen_enter_scope`].
#[derive(Clone, Copy, PartialEq)]
enum EnterScope {
    /// Top-level tab switch: the whole tab body slides in
    /// horizontally while the top bar stays planted. `dx` is the
    /// starting offset — positive enters from the right (moving
    /// forward in nav order), negative from the left (moving
    /// back).
    Body { dx: f32 },
    /// Welcome/session swaps: the whole window glides into place
    /// vertically. `dy` is the starting offset — positive rises in
    /// from below (the default), negative descends from above
    /// (closing a session, so the return to the menu reads as
    /// stepping back down rather than climbing further).
    Root { dy: f32 },
}

/// How far a pane starts off-position when sliding in.
const PANE_SLIDE: f32 = 28.0;

/// How far the whole window starts off-position on a Root enter.
const ROOT_SLIDE: f32 = 10.0;

/// Identity of what `view` is fundamentally showing. Computed
/// before and after every `update` dispatch; a change means the
/// screen got swapped wholesale and triggers [`App::screen_enter`].
/// (Settings sections and save-view sub-tabs animate themselves —
/// their entrances live in their own state.)
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScreenKey {
    Welcome,
    Session,
    Tabs(Tab),
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

        // Restore the last selection from config, but only the bits
        // that still resolve against the current scanners.
        let mut restored = loadout::Loadout::default();
        // Restore the selected family (drives the picker even when no
        // owned-ROM game resolves under it); falls back to the family of
        // `last_game` for configs written before `last_family` existed.
        restored.family = config
            .last_family
            .as_deref()
            .and_then(game::family_static)
            .or_else(|| config.last_game.as_ref().and_then(|(f, _)| game::family_static(f)));
        if let Some((family, variant)) = config.last_game.as_ref() {
            if let Some(game) = tango_gamedb::find_by_family_and_variant(family, *variant) {
                if scanners.roms.read().contains_key(&game) {
                    restored.game = Some(game);
                    restored.family = Some(game.family_and_variant().0);
                    if let Some(rel) = config.last_save_per_game.get(&config::game_key(game)) {
                        let abs = config.data_relative_to_absolute(rel);
                        if scanners
                            .saves
                            .read()
                            .get(&game)
                            .map(|v| v.iter().any(|s| s.path == abs))
                            .unwrap_or(false)
                        {
                            restored.save = Some(abs);
                            // The patch overlay hangs off the save — restore
                            // whatever this save was last used with, if the
                            // patch still exists and supports the variant.
                            if let Some(Some((n, v))) = config.last_patch_per_save.get(rel) {
                                let patches = scanners.patches.read();
                                let ok = patches
                                    .get(n)
                                    .and_then(|p| p.versions.get(v))
                                    .map(|vm| vm.supported_games.contains(&game))
                                    .unwrap_or(false);
                                if ok {
                                    restored.patch = Some(n.clone());
                                    restored.patch_version = Some(v.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
        // The patch row's swap transition has to start in the
        // restored expanded state (a remembered patch keeps the
        // pickers up) — constructed at rest so launch doesn't play
        // a fold animation.
        restored.patch_row = anim::Transition::swap(restored.patch.is_some());
        let welcome = tabs::welcome::State::from_nickname(config.nickname.as_deref());

        // Spin up the SDL audio backend once at startup with the
        // LateBinder as the source. Sessions later bind their
        // MGBAStream into the binder and the SDL stream keeps going
        // across selections.
        let mut audio_binder = audio::LateBinder::new();
        audio_binder.set_volume(config.volume);
        let audio_backend = match audio::sdl::Backend::new(audio_binder.clone()) {
            Ok(b) => {
                use audio::Backend;
                audio_binder.set_sample_rate(b.sample_rate());
                log::info!("audio: sdl backend up at {} Hz", b.sample_rate());
                Some(b)
            }
            Err(e) => {
                log::warn!("audio: sdl init failed, running silent: {e:?}");
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

        let mut play = tabs::play::State::default();
        // CLI `Join <code>` (or Discord deep-link routed through
        // the same channel) lands here — prefill the link code so
        // the user can hit Fight straight away.
        let init_link_code = INIT_LINK_CODE.get().and_then(|c| c.clone());
        if let Some(code) = &init_link_code {
            play.link_code = code.clone();
        }

        let mut app = Self {
            config,
            tab: Tab::Play,
            welcome,
            settings: tabs::settings::State::default(),
            scanners,
            audio_binder,
            _audio_backend: audio_backend,
            loaded: None,
            loadout: restored,
            play,
            replays: ReplaysState::default(),
            patches: PatchesState::default(),
            session: session::State::new(),
            netplay: netplay::State::new(),
            discord: discord::Client::new(),
            session_started_at: None,
            patch_autoupdater,
            updater,
            rescans_in_flight: 0,
            // Start at rest (no launch animation) — progress 1.0
            // and not animating until first triggered.
            screen_enter: anim::Enter::default(),
            screen_enter_scope: EnterScope::Root { dy: ROOT_SLIDE },
            lobby_swap: anim::Transition::swap(false),
            lobby_exit_snapshot: None,
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
    /// selection change so the next launch restores it. The save is
    /// remembered per game, and the patch overlay per save — so every
    /// save carries the patch it was last used with, including the
    /// patch a template-created save was born under.
    fn persist_selection(&mut self) {
        self.config.last_family = self.loadout.family.map(|f| f.to_string());
        self.config.last_game = self
            .loadout
            .game
            .map(|g| (g.family_and_variant().0.to_string(), g.family_and_variant().1));
        if let (Some(g), Some(p)) = (self.loadout.game, self.loadout.save.as_ref()) {
            if let Some(rel) = self.config.data_relative_string(p) {
                self.config.last_save_per_game.insert(config::game_key(g), rel.clone());
                let overlay = match (&self.loadout.patch, &self.loadout.patch_version) {
                    (Some(n), Some(v)) => Some((n.clone(), v.clone())),
                    _ => None,
                };
                self.config.last_patch_per_save.insert(rel, overlay);
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
        let Some(game) = self.loadout.game else { return };
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

    /// Run a full `Scanners::rescan` on a tokio blocking worker so
    /// the disk walk + TOML parse for patches (the slowest of the
    /// four) doesn't stall iced's update loop. Returns a task that
    /// emits `Message::Rescanned(followup)` once the worker is
    /// done; the followup tells the handler which post-scan work to
    /// chain (refresh `self.loaded`, warm stats, auto-pick a save).
    ///
    /// Bumps `rescans_in_flight` synchronously so the very next
    /// `view` call sees the rescan as live and renders the per-tab
    /// Rescan button disabled — without this, the button would
    /// remain clickable until the spawned worker thread actually
    /// gets scheduled.
    fn rescan_off_thread(&mut self, followup: RescanFollowup) -> iced::Task<Message> {
        self.rescans_in_flight += 1;
        let scanners = self.scanners.clone();
        let config = self.config.clone();
        iced::Task::perform(
            async move {
                let _ = tokio::task::spawn_blocking(move || scanners.rescan(&config)).await;
            },
            move |()| Message::Rescanned(followup),
        )
    }

    /// Whether any rescan worker spawned by [`rescan_off_thread`] is
    /// still in flight. View functions read this to disable their
    /// Rescan buttons.
    pub fn is_rescanning(&self) -> bool {
        self.rescans_in_flight > 0
    }

    /// If a netplay state change just flipped the compat verdict to
    /// anything other than Compatible while we're still flagged
    /// ready, fire an Uncommit so the local commit doesn't outlive
    /// the agreement it was based on. Covers the cases the netplay
    /// handlers don't catch — peer changing their game/patch/
    /// match_type, or our own available_patches shrinking out from
    /// under a previously-valid commit.
    fn uncommit_if_incompat(&self) -> iced::Task<Message> {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        if !self.netplay.lobby.local_ready {
            return iced::Task::none();
        }
        let (Some(local), Some(remote)) = (self.netplay.lobby.local.as_ref(), self.netplay.lobby.remote.as_ref())
        else {
            return iced::Task::none();
        };
        let patches = self.scanners.patches.read();
        let verdict = netplay::compat::check(local, remote, &*patches);
        if matches!(verdict, netplay::compat::Verdict::Compatible) {
            return iced::Task::none();
        }
        iced::Task::done(Message::Netplay(netplay::Message::Uncommit))
    }

    /// Build a `protocol::Settings` packet from the App's current
    /// state: nickname from config, match_type defaults to (0, 0),
    /// game_info from the local loadout, and the available_games /
    /// available_patches lists from the scanners so the peer can see
    /// what we have locally.
    fn make_local_settings(&self) -> net::protocol::Settings {
        self.loadout
            .make_local_settings(&self.config, &self.netplay.lobby, &self.scanners)
    }

    fn loaded_key(&self) -> Option<(rom::GameRef, std::path::PathBuf, Option<(String, semver::Version)>)> {
        let game = self.loadout.game?;
        let save_path = self.loadout.save.clone()?;
        let patch = match (&self.loadout.patch, &self.loadout.patch_version) {
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
            self.loadout.save = None;
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
        // We just swapped in a freshly-built save, so any in-progress
        // edit (which lived in the previous in-memory save) is gone —
        // leave the global edit mode so the UI doesn't show stale state.
        // Dropping the whole EditState clears every editor's scratch at
        // once. The commit path takes the early-return above and never
        // reaches here, so this only fires on a real selection change.
        self.play.save_view.clear_editing();
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
    /// Emitted by the `window::frames()` subscription while a UI
    /// animation is mid-flight. Carries no state change — its
    /// only job is to drive another update → view pass so the
    /// animation can sample a fresh `Instant`.
    AnimTick,
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
    /// Exit the application. Fired by the top bar's close button,
    /// which only renders in fullscreen — there's no OS title-bar X
    /// to reach for there (iced's fullscreen is borderless).
    Quit,
    /// Fired when a backgrounded `Scanners::rescan` task completes.
    /// `followup` tells the handler which post-scan work to do —
    /// most paths just want `Refresh` (re-validate `self.loaded`),
    /// the replays-tab rescan also warms the stats cache, and the
    /// save-delete handler asks for a fresh "first save" pick now
    /// that the scan results are in.
    Rescanned(RescanFollowup),
}

/// Per-call-site cue for `Message::Rescanned`. Lets one handler
/// arm cover every rescan we kick off without dispatching a
/// distinct Message variant per call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescanFollowup {
    /// Just re-validate `self.loaded` against the fresh scan.
    Refresh,
    /// Refresh + warm the replays-tab stats cache (used after the
    /// Replays-tab Rescan button).
    RefreshAndReplayStats,
    /// Refresh + if `local_save` is `None`, auto-pick the first
    /// remaining save for the local game. Used by the save-delete
    /// handler so the picker doesn't strand on an empty selection.
    RefreshAndPickFirstSave,
    /// Drop `self.loaded` first so `refresh_loaded` rebuilds it
    /// from scratch (bypassing the same-key dedupe). Used after a
    /// single-player session writes back to its SRAM — the save
    /// path didn't change but the bytes did.
    ForceRebuildLoaded,
}

impl App {
    pub fn title(&self) -> String {
        t!(&self.config.language, "window-title")
    }

    /// What `view` is fundamentally showing right now. A change
    /// across an `update` dispatch means the screen was swapped
    /// wholesale — the trigger for [`App::screen_enter`].
    fn screen_key(&self) -> ScreenKey {
        if self.config.nickname.is_none() {
            ScreenKey::Welcome
        } else if self.session.is_active() {
            ScreenKey::Session
        } else {
            ScreenKey::Tabs(self.tab)
        }
    }

    /// Whether the Play tab's bottom band is the lobby (a netplay
    /// attempt is in flight, failed-but-not-dismissed, or handing
    /// off) rather than the link-code strip. Drives
    /// [`App::lobby_swap`] and the nav badge on the Play tab.
    /// `Failed` counts: the lobby stays up as a sticky failure
    /// banner until the user cancels it.
    fn lobby_on_screen(&self) -> bool {
        matches!(
            self.netplay.phase,
            netplay::Phase::Connecting { .. }
                | netplay::Phase::Negotiating { .. }
                | netplay::Phase::Lobby { .. }
                | netplay::Phase::Failed { .. }
        ) || self.netplay.handoff_pending()
    }

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        let screen_before = self.screen_key();
        let family_before = self.loadout.family;
        let selection_before = (self.loadout.game, self.loadout.save.clone());
        // Candidate snapshot for the lobby's exit animation — taken
        // before dispatch (the handler about to run may reset the
        // phase/lobby), kept only if the lobby actually left.
        let lobby_live = self
            .lobby_on_screen()
            .then(|| (self.netplay.phase.clone(), self.netplay.lobby.clone()));
        let task = self.update_inner(message);
        let now = iced::time::Instant::now();
        let screen_after = self.screen_key();
        if screen_before != screen_after {
            self.screen_enter.start(now);
            self.screen_enter_scope = match (screen_before, screen_after) {
                (ScreenKey::Tabs(t1), ScreenKey::Tabs(t2)) => {
                    // The nav strip lays the tabs out in declaration
                    // order, so the discriminants double as nav
                    // positions: moving right brings the new pane in
                    // from the right, moving left from the left.
                    let dx = if (t2 as u8) > (t1 as u8) {
                        PANE_SLIDE
                    } else {
                        -PANE_SLIDE
                    };
                    EnterScope::Body { dx }
                }
                // Closing a session descends — the menu comes back
                // in from above, mirroring the rise that brought
                // the session up.
                (ScreenKey::Session, _) => EnterScope::Root { dy: -ROOT_SLIDE },
                _ => EnterScope::Root { dy: ROOT_SLIDE },
            };
        }
        // The Play tab's band swap follows the netplay phase: the
        // view morphs the link-code strip into the lobby (and back)
        // off this transition. When the lobby leaves, freeze its last
        // live state for the exit half to render from.
        let lobby_after = self.lobby_on_screen();
        if let (Some(snap), false) = (lobby_live, lobby_after) {
            self.lobby_exit_snapshot = Some(snap);
            // A Fight-generated code never touched the input on the way in
            // (it debuted in the lobby band) — drop it in now that the
            // strip is coming back, so a retry re-hosts the same code.
            self.play.restore_generated_link_code();
        }
        self.lobby_swap.set(lobby_after, now);
        // Effect handlers in this module mutate `loadout.patch`
        // directly (e.g. save creation dropping an unsupported
        // patch) — re-sync the patch row's transition here too so
        // those paths animate like in-module ones.
        self.loadout.sync_patch_row(now);
        // A different family swaps the entire bottom of the tab —
        // rise the whole save-view pane in. A different game or save
        // within the family only re-renders the save's content — rise
        // just the panes under the save view's sub-tab strip, leaving
        // the strip itself planted. (Sub-tab switches slide the inner
        // panes horizontally instead; see save_view::State::apply.)
        if family_before != self.loadout.family {
            self.play.save_body_enter.start(now);
        } else if selection_before != (self.loadout.game, self.loadout.save.clone()) {
            self.play.save_view.enter_from = iced::Vector::new(0.0, 20.0);
            self.play.save_view.enter.start(now);
        }
        task
    }

    fn update_inner(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::NoOp => iced::Task::none(),
            Message::AnimTick => iced::Task::none(),
            // Same as the OS close button: config is persisted
            // incrementally (on every change + resize), so there's
            // no shutdown bookkeeping to flush here.
            Message::Quit => iced::exit(),
            Message::TabSelected(t) => {
                self.tab = t;
                iced::Task::none()
            }
            // Loadout strip interactions route to the shared
            // App-level Loadout — the tab never sees them.
            // Every dispatch below is followed by a Settings resend —
            // the netplay handler dedupes against the last-sent value
            // via `Settings: Eq`, so unchanged dispatches are free.
            Message::Play(tabs::play::Message::Loadout(m)) => {
                let task = self.update_loadout(m);
                iced::Task::batch([task, self.resend_settings_if_lobby()])
            }
            Message::Play(m) => {
                let task = self.update_play(m);
                iced::Task::batch([task, self.resend_settings_if_lobby()])
            }
            Message::Patches(m) => self.update_patches(m),
            Message::DiscordTick => {
                self.handle_discord_tick();
                iced::Task::none()
            }
            Message::Window(id, ev) => {
                match ev {
                    iced::window::Event::Resized(size) => {
                        // The Resized size could be either a user-driven
                        // resize or the result of maximize/unmaximize.
                        // We need is_maximized to decide whether to keep
                        // it as the restore size, so query it and finish
                        // the bookkeeping in WindowMaximizedQueried.
                        return iced::window::is_maximized(id)
                            .map(move |maximized| Message::WindowMaximizedQueried { size, maximized });
                    }
                    iced::window::Event::Moved(point) => {
                        // Entering fullscreen parks the window at its
                        // monitor's origin and fires Moved — so the
                        // persisted value at quit time identifies the
                        // fullscreen monitor for the next launch (see
                        // Config::last_window_position).
                        self.config.last_window_position = Some((point.x, point.y));
                        self.persist_config();
                    }
                    _ => {}
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
            Message::Replays(m) => self.update_replays(m),
            Message::Settings(m) => self.update_settings(m).map(Message::Settings),
            Message::Welcome(m) => self.update_welcome(m),
            Message::Session(m) => {
                // In-match frame-delay slider: persist the new value to config so
                // the choice sticks for the next match (session.update applies it
                // to the live session). Mirrors the lobby slider's persistence.
                if let session::Message::SetFrameDelay(d) = &m {
                    self.config.frame_delay = *d;
                    self.persist_config();
                }
                // The active session may have mutated the user's
                // save file on disk (single-player writes via
                // mgba's RW VFile). When the session ends, drop it
                // first so mgba's thread joins + flushes its file
                // handle, then re-scan saves + force a Loaded
                // rebuild so the play tab's save view reflects the
                // fresh on-disk SRAM. Detected by the active-slot
                // transition (not the Close message) because Esc
                // closes SP sessions inside the session update.
                let was_sp = matches!(self.session.active, Some(ActiveSession::SinglePlayer(_)));
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
                // Rescan + reload run off-thread; the Rescanned
                // followup forces a `loaded` rebuild past the
                // same-key dedupe so the play tab's save view
                // reflects the fresh on-disk SRAM.
                let sp_rescan = if was_sp && self.session.active.is_none() {
                    self.rescan_off_thread(RescanFollowup::ForceRebuildLoaded)
                } else {
                    iced::Task::none()
                };
                // PvP sessions write a `.tangoreplay` next to
                // the saves dir on match end; once the session
                // clears we want the new file to show up in the
                // Replays tab without a manual rescan. The
                // `RefreshAndReplayStats` followup also warms the
                // stats sidebar with the just-landed match.
                let pvp_closed = was_pvp && self.session.active.is_none();
                let pvp_rescan = if pvp_closed {
                    self.rescan_off_thread(RescanFollowup::RefreshAndReplayStats)
                } else {
                    iced::Task::none()
                };
                iced::Task::batch([task, sp_rescan, pvp_rescan])
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
                let local_game = self.loadout.game;
                let local_patch = self.loadout.patch.clone().zip(self.loadout.patch_version.clone());
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
                    |result| Message::PvpSessionBuilt(std::sync::Arc::new(std::sync::Mutex::new(Some(result)))),
                )
            }
            Message::Netplay(m) => {
                // Always resend after a netplay message too: this
                // covers the Negotiating → Lobby transition (first
                // announce) and lobby-state mutations like
                // SetMatchType / SetFrameDelay. The dedupe inside
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
                let resend = self.resend_settings_if_lobby();
                let uncommit = self.uncommit_if_incompat();
                iced::Task::batch([task, resend, uncommit, attention])
            }
            Message::PvpSessionBuilt(slot) => {
                let Some(result) = slot.lock().unwrap().take() else {
                    return iced::Task::none();
                };
                match result {
                    Ok(session) => {
                        let has_opponent_panel = session.opponent_loaded.is_some();
                        self.session.active = Some(ActiveSession::PvP(session));
                        self.session.show_opponent_panel = has_opponent_panel;
                        self.session.wake_controls();
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
            Message::Rescanned(followup) => {
                self.rescans_in_flight = self.rescans_in_flight.saturating_sub(1);
                match followup {
                    RescanFollowup::Refresh => {
                        self.refresh_loaded();
                        iced::Task::none()
                    }
                    RescanFollowup::RefreshAndReplayStats => {
                        self.refresh_loaded();
                        self.refresh_replay_stats().map(Message::Replays)
                    }
                    RescanFollowup::RefreshAndPickFirstSave => {
                        // Land on the next available save anywhere in the
                        // family (a sibling color variant is fine), not just
                        // the deleted save's own game, and fix the loadout's
                        // game to whatever that save resolves to.
                        if self.loadout.save.is_none() {
                            if let Some(family) = self.loadout.family {
                                if let Some((game, path)) = loadout::first_available_family_save(&self.scanners, family)
                                {
                                    self.loadout.select_save(game, path, &self.config, &self.scanners);
                                }
                            }
                        }
                        self.refresh_loaded();
                        iced::Task::none()
                    }
                    RescanFollowup::ForceRebuildLoaded => {
                        self.loaded = None;
                        self.refresh_loaded();
                        iced::Task::none()
                    }
                }
            }
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let mut subs = vec![
            session::subscription(&self.session).map(Message::Session),
            netplay::subscription(&self.netplay).map(Message::Netplay),
            // 1 Hz Discord refresh — cheap (compares activity for
            // equality before re-sending) and gives us the join-
            // secret pickup loop too.
            iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::DiscordTick),
            // Window events drive the geometry-persistence loop.
            iced::window::events().map(|(id, ev)| Message::Window(id, ev)),
        ];
        // Per-frame redraw driver, alive only while something is
        // actually moving: any registered animation mid-flight
        // (screen entrances, overlay transitions, pane enters —
        // they all `kick` the shared registry when they start) or
        // the Play tab's pulsing connection-status line. The menu
        // UI otherwise redraws on events only, so dropping this
        // when idle is what keeps animations from costing 60 fps
        // forever.
        let waiting_pulse_on_screen = !self.session.is_active()
            && self.tab == Tab::Play
            && match &self.netplay.phase {
                netplay::Phase::Connecting { .. } | netplay::Phase::Negotiating { .. } => true,
                // The lobby's "waiting for opponent data" handshake
                // line pulses too, until both sides' settings land.
                netplay::Phase::Lobby { .. } => {
                    self.netplay.lobby.local.is_none() || self.netplay.lobby.remote.is_none()
                }
                _ => false,
            };
        if anim::any_active() || waiting_pulse_on_screen {
            subs.push(iced::window::frames().map(|_| Message::AnimTick));
        }
        iced::Subscription::batch(subs)
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
        let game_info = self.loadout.game.map(|g| {
            let patch = self
                .loadout
                .patch
                .as_ref()
                .zip(self.loadout.patch_version.as_ref())
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

    /// Apply a loadout-strip message (from either tab) to the shared
    /// App-level [`loadout::Loadout`] and run the selection-change
    /// follow-ups. The caller batches a lobby settings-resend after
    /// this, so a mid-lobby save/patch switch reaches the peer.
    fn update_loadout(&mut self, msg: loadout::Message) -> iced::Task<Message> {
        let Some(effect) = self.loadout.update(msg, &self.scanners, &self.config) else {
            return iced::Task::none();
        };
        match effect {
            loadout::Effect::SelectionChanged => {
                self.refresh_loaded();
                self.persist_selection();
                // Game might have just changed — if so, the lobby
                // picker should show this game's default match
                // type (Triple where supported) instead of the
                // last game's pick.
                self.apply_default_match_type();
                iced::Task::none()
            }
            loadout::Effect::Rescan => self.rescan_off_thread(RescanFollowup::Refresh),
        }
    }

    fn update_play(&mut self, msg: tabs::play::Message) -> iced::Task<Message> {
        let Some(effect) = self
            .play
            .update(msg, &self.scanners, &self.config, self.loaded.as_ref(), &self.loadout)
        else {
            return iced::Task::none();
        };
        use tabs::play::Effect as E;
        match effect {
            E::SetFrameDelay(d) => {
                // Lobby slider. Persisted to config; it's this side's local
                // frame delay (snapshotted into the match at start, not
                // negotiated with the peer), so there's no live match to push it
                // to here.
                self.config.frame_delay = d;
                self.persist_config();
                iced::Task::none()
            }
            E::Connect { ident, copy_code } => {
                let msg = match ident {
                    netplay::LinkIdent::Matchmaking(link_code) => netplay::Message::Connect {
                        link_code,
                        endpoint: self.config.matchmaking_endpoint.clone(),
                        use_relay: self.config.relay_mode.use_relay(),
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
                match copy_code {
                    // Fight auto-generated this code — put it straight on
                    // the clipboard so the host can paste it to their
                    // opponent right away.
                    Some(code) => iced::Task::batch([iced::clipboard::write(code), task]),
                    None => task,
                }
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
                    if let Some(g) = self.loadout.game {
                        let (fam, var) = g.family_and_variant();
                        self.netplay.lobby.default_mt_for_game = Some((fam.to_string(), var));
                    }
                }
                self.netplay.update(m).map(Message::Netplay)
            }
            E::ReadyWithSave => {
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
                        self.session.wake_controls();
                    }
                    Err(e) => {
                        log::warn!("singleplayer start failed: {e}");
                        self.play.last_error = Some(format!("{e}"));
                    }
                }
                iced::Task::none()
            }
            E::SaveDuplicate { new_stem } => {
                if let Some(src) = self.loadout.save.clone() {
                    match duplicate_save(&src, &new_stem) {
                        Ok(dst) => {
                            log::info!("duplicated save: {} → {}", src.display(), dst.display());
                            self.loadout.save = Some(dst);
                            self.persist_selection();
                            return self.rescan_off_thread(RescanFollowup::Refresh);
                        }
                        Err(e) => log::error!("duplicate save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::SaveRename { new_stem } => {
                if let Some(src) = self.loadout.save.clone() {
                    match rename_save(&src, &new_stem) {
                        Ok(dst) => {
                            log::info!("renamed save: {} → {}", src.display(), dst.display());
                            self.loadout.save = Some(dst);
                            self.persist_selection();
                            return self.rescan_off_thread(RescanFollowup::Refresh);
                        }
                        Err(e) => log::error!("rename save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::SaveDelete => {
                if let Some(src) = self.loadout.save.clone() {
                    if let Err(e) = std::fs::remove_file(&src) {
                        log::error!("delete save: {e}");
                    } else {
                        log::info!("deleted save: {}", src.display());
                    }
                    // Clear the selection now so the picker shows
                    // "no save" while the rescan is in flight;
                    // PickFirstSave restores the first remaining
                    // entry once the scan finishes.
                    self.loadout.save = None;
                    self.persist_selection();
                    return self.rescan_off_thread(RescanFollowup::RefreshAndPickFirstSave);
                }
                iced::Task::none()
            }
            E::SaveNew { name, template, game } => {
                // The new save is created for `game` (the variant the
                // user picked), which may differ from the currently
                // selected one — so adopt it as the loadout's game too,
                // keeping game/save consistent for `refresh_loaded`.
                if let Some(template) = tabs::play::creation_template(game, &template, &self.loadout, &self.scanners) {
                    match create_new_save(&self.config.saves_path(), &name, template.as_ref()) {
                        Ok(dst) => {
                            log::info!(
                                "created new save for {:?}: {}",
                                game.family_and_variant(),
                                dst.display()
                            );
                            // Templates are only offered for patch-supported
                            // variants, so the patch normally still applies;
                            // drop it only if it somehow doesn't support the
                            // created variant.
                            if !loadout::patch_supports(&self.loadout, &self.scanners, game) {
                                self.loadout.patch = None;
                                self.loadout.patch_version = None;
                            }
                            self.loadout.game = Some(game);
                            self.loadout.family = Some(game.family_and_variant().0);
                            self.loadout.save = Some(dst);
                            // Records the save→patch association too — a
                            // template-created save is born remembering the
                            // patch it was created under.
                            self.persist_selection();
                            return self.rescan_off_thread(RescanFollowup::Refresh);
                        }
                        Err(e) => log::error!("create save: {e}"),
                    }
                }
                iced::Task::none()
            }
            E::EditChips(edit) => {
                // Stage one edit into the in-memory loaded save. The UI
                // reads `loaded.save` directly, so the change shows
                // immediately; nothing is written to disk until Save.
                if let Some(loaded) = self.loaded.as_mut() {
                    crate::save_edit::apply_chip_edit(loaded, edit);
                }
                iced::Task::none()
            }
            E::EditNavicust(edit) => {
                // Stage one navicust edit into the in-memory loaded save;
                // the UI reads `loaded.save` directly so it shows live.
                if let Some(loaded) = self.loaded.as_mut() {
                    crate::save_edit::apply_navicust_edit(loaded, edit);
                }
                iced::Task::none()
            }
            E::EditPatchCard56s(edit) => {
                // Stage one BN5/BN6 patch-card edit into the in-memory loaded
                // save; the UI reads `loaded.save` directly so it shows live.
                if let Some(loaded) = self.loaded.as_mut() {
                    crate::save_edit::apply_patch_card56_edit(loaded, edit);
                }
                iced::Task::none()
            }
            E::EditPatchCard4s(edit) => {
                // Stage one BN4 patch-card edit into the in-memory loaded save;
                // the UI reads `loaded.save` directly so it shows live.
                if let Some(loaded) = self.loaded.as_mut() {
                    crate::save_edit::apply_patch_card4_edit(loaded, edit);
                }
                iced::Task::none()
            }
            E::EditAutoBattleData(edit) => {
                // Stage one auto-battle-data edit into the in-memory loaded
                // save; the UI reads `loaded.save` directly so it shows live.
                if let Some(loaded) = self.loaded.as_mut() {
                    crate::save_edit::apply_auto_battle_data_edit(loaded, edit);
                }
                iced::Task::none()
            }
            E::SaveEditCommit => {
                // `Some(sram)` once the edited save is written; the SRAM is
                // reused below to refresh a live netplay commitment.
                let saved_sram = if let Some(loaded) = self.loaded.as_mut() {
                    if loaded.save_path.as_os_str().is_empty() {
                        None
                    } else {
                        // Every staged edit already keeps its view's derived
                        // caches in sync as it's applied — the anti-cheat
                        // folder/patch-card mirror (chips, patch cards) and
                        // the materialized WRAM caches (navicust, auto-battle
                        // data). So commit only has to recompute the whole-SRAM
                        // checksum and write once.
                        loaded.save.rebuild_checksum();
                        // Refresh the baked Navi-view image from the updated
                        // save (commit keeps the in-memory Loaded, so without
                        // this the read-only grid lags until reselection).
                        loaded.rebuild_navicust_render();
                        let sram = loaded.save.to_sram_dump();
                        let path = loaded.save_path.clone();
                        match std::fs::write(&path, &sram) {
                            Ok(()) => {
                                log::info!("saved edited save: {}", path.display());
                                Some(sram)
                            }
                            Err(e) => {
                                log::error!("save edited save: {e}");
                                None
                            }
                        }
                    }
                } else {
                    None
                };
                let Some(sram) = saved_sram else {
                    return iced::Task::none();
                };
                // If we're in a lobby and already committed (Ready), the saved
                // edits changed the save our commitment was made over — re-commit
                // so the opponent gets the new commitment (and chunks) instead of
                // a hash of our pre-edit save.
                let recommit =
                    if matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) && self.netplay.lobby.local_ready {
                        self.netplay
                            .update(netplay::Message::Commit { save_sram: sram })
                            .map(Message::Netplay)
                    } else {
                        iced::Task::none()
                    };
                // Reconcile the scanner cache with the new on-disk bytes (the
                // in-memory loaded is already current, so refresh_loaded will
                // early-return and keep it).
                let rescan = self.rescan_off_thread(RescanFollowup::Refresh);
                iced::Task::batch([rescan, recommit])
            }
            E::SaveEditCancel => {
                // Staged edits live only in the in-memory loaded save;
                // the on-disk file and the scanner cache still hold the
                // original. Drop and rebuild loaded to revert every tab.
                self.loaded = None;
                self.refresh_loaded();
                iced::Task::none()
            }
            E::SaveViewTask(t) => t.map(Message::Play),
        }
    }

    fn update_patches(&mut self, msg: tabs::patches::Message) -> iced::Task<Message> {
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
            E::Rescan => self.rescan_off_thread(RescanFollowup::Refresh),
            E::UpdateRescan => self.rescan_off_thread(RescanFollowup::Refresh),
            E::StartUpdate { url, root } => iced::Task::perform(
                async move { patch::update(url, root).await.map_err(|e| e.to_string()) },
                tabs::patches::Message::UpdateFinished,
            )
            .map(Message::Patches),
            E::ToggleFavorite(name) => {
                if !self.config.favorite_patches.remove(&name) {
                    self.config.favorite_patches.insert(name);
                }
                self.persist_config();
                iced::Task::none()
            }
        }
    }

    fn update_replays(&mut self, msg: tabs::replays::Message) -> iced::Task<Message> {
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
                        self.session.wake_controls();
                    }
                    Err(e) => log::warn!("failed to play replay {}: {e}", p.display()),
                }
                iced::Task::none()
            }
            // User triggered a full rescan — re-validate the
            // stats cache and warm it for any new replays
            // (handled in the Rescanned handler via the
            // `RefreshAndReplayStats` followup).
            E::Rescan => self.rescan_off_thread(RescanFollowup::RefreshAndReplayStats),
            E::CopyText(s) => iced::clipboard::write(s),
            E::CopyImage(img) => {
                copy_image_to_clipboard(img);
                iced::Task::none()
            }
            E::OpenExportSaveDialog {
                replay: replay_path,
                lossless,
            } => {
                // Lossless export muxes libx264rgb + flac, which .mkv holds
                // natively; scaled export targets the more portable .mp4.
                let ext = if lossless { "mkv" } else { "mp4" };
                let filter_name = if lossless { "Matroska" } else { "MP4" };
                let stem = replay_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "replay".to_string());
                let default_name = format!("{stem}.{ext}");
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
                            .add_filter(filter_name, &[ext])
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
                .map(Message::Replays)
            }
            E::StartExport {
                replay,
                output,
                settings,
                rounds,
            } => self
                .spawn_replay_export(replay, output, settings, rounds)
                .map(Message::Replays),
            E::SaveViewTask(t) => t.map(Message::Replays),
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
            // The export re-simulates both sides (the local-perspective
            // core plus the opponent shadow) from the recorded inputs, so
            // each side's ROM must be the exact patched ROM that was used
            // when the match was recorded — otherwise the re-sim desyncs.
            // Mirror `session::build_playback`'s `resolve_rom`: apply the
            // side's patch from disk before handing the bytes to export.
            // (Without this a cross-patch replay renders desynced garbage
            // or stalls partway, while playback — which does patch — is
            // fine.)
            let patches_path = self.config.patches_path();
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
                let rom = if let Some(patch_info) = gi.patch.as_ref() {
                    let v = semver::Version::parse(&patch_info.version)?;
                    patch::apply_patch_from_disk(&rom, entry, &patches_path, &patch_info.name, &v)?
                } else {
                    rom
                };
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
        let done_arc: std::sync::Arc<std::sync::Mutex<Option<Result<std::path::PathBuf, String>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
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
                *done_arc_thread.lock().unwrap() = Some(result);
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
                            let r = done.lock().unwrap().take().unwrap_or_else(|| {
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
            C::RelayMode(m) => self.config.relay_mode = m,
            C::FrameDelay(v) => {
                self.config.frame_delay =
                    v.clamp(tango_pvp::battle::MIN_FRAME_DELAY, tango_pvp::battle::MAX_FRAME_DELAY)
            }
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
                return iced::window::latest().and_then(move |id| iced::window::set_mode(id, mode));
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
            // Sampled by spawn_pvp at match start; nothing live to poke.
            C::DisableBgmInPvp(b) => self.config.disable_bgm_in_pvp = b,
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

    fn update_welcome(&mut self, msg: tabs::welcome::Message) -> iced::Task<Message> {
        use tabs::welcome::Message as M;
        match msg {
            M::NicknameChanged(s) => {
                self.welcome.nickname_draft = s;
                iced::Task::none()
            }
            M::Continue => {
                if let Some(nickname) = self.welcome.finalize_nickname() {
                    self.config.nickname = Some(nickname);
                    self.persist_config();
                }
                iced::Task::none()
            }
            M::LanguageSelected(l) => {
                self.config.language = l;
                self.persist_config();
                iced::Task::none()
            }
            M::OpenRomsFolder => {
                let p = self.config.roms_path();
                let _ = std::fs::create_dir_all(&p);
                if let Err(e) = open::that(&p) {
                    log::error!("open roms folder: {e}");
                }
                iced::Task::none()
            }
            M::RescanRoms => self.rescan_off_thread(RescanFollowup::Refresh),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let lang = &self.config.language;

        // Live entrance glide, `Some(progress)` while mid-flight.
        // Sampled once here; the branches below wrap whatever they
        // return (whole window or just the tab body, per
        // `screen_enter_scope`).
        let now = iced::time::Instant::now();
        let enter = self.screen_enter.progress(now);

        // First-run gate: no main UI until the user picks a nickname.
        // Sits on the same cyberworld backdrop as the main shell so
        // the first thing a new user sees is already the PET screen.
        if self.config.nickname.is_none() {
            let roms_count = self.scanners.roms.read().len();
            let welcome = tabs::welcome::view(
                lang,
                &self.welcome,
                roms_count,
                &self.config.roms_path(),
                self.is_rescanning(),
            )
            .map(Message::Welcome);
            return entered(
                iced::widget::stack![widgets::cyber_backdrop(), welcome]
                    .width(Fill)
                    .height(Fill)
                    .into(),
                enter,
                ROOT_SLIDE,
            );
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
                crate::video::effects::effect_for(&self.config.video_filter),
            )
            .map(Message::Session);
            // In-session settings modal: floats centered over the
            // running session with a dimmed click-to-dismiss
            // backdrop. The emulator keeps running underneath.
            // Rendered while the open/close transition is in
            // flight too, so the panel eases in and out.
            let composed: Element<'_, Message> = if self.session.settings_anim.visible(now) {
                let progress = self.session.settings_anim.progress(now);
                let body = tabs::settings::view(lang, &self.config, &self.settings, self.updater.status_blocking())
                    .map(Message::Settings);
                // Top header row carrying the X close button. The
                // close is the only affordance for dismissing the
                // modal — the backdrop is inert. Inline (not a
                // floating overlay) so the body lays out beneath.
                // Same chrome as the fullscreen top bar's app-close
                // X — both are window-dismissal affordances, so
                // they share the quiet-at-rest / red-on-hover look.
                let close_btn = widgets::icon_button_styled(
                    lucide_icons::Icon::X,
                    t!(lang, "playback-close"),
                    Some(Message::Session(session::Message::CloseSettings)),
                    [4.0, 8.0],
                    widgets::window_close,
                );
                let heading = iced::widget::text(t!(lang, "tab-settings")).size(crate::style::TEXT_HEADING);
                let header = iced::widget::container(
                    row![heading, iced::widget::space::horizontal(), close_btn]
                        .padding(iced::Padding {
                            top: 8.0,
                            right: 8.0,
                            bottom: 0.0,
                            left: 14.0,
                        })
                        .align_y(iced::Alignment::Center),
                )
                .width(Fill);
                let modal_panel = iced::widget::container(column![header, body].spacing(0).width(Fill).height(Fill))
                    .width(iced::Length::Fixed(820.0))
                    .height(iced::Length::Fixed(560.0))
                    .style(widgets::panel);
                // Wrap the panel in a mouse_area so clicks on
                // its inert regions (background, headings) get
                // swallowed instead of falling through to the
                // dismiss-on-press backdrop layer below.
                let modal_panel_swallow =
                    mouse_area(anim::pop(modal_panel, progress, 12.0)).on_press(|_| Message::NoOp);
                let placement = iced::widget::container(modal_panel_swallow)
                    .width(Fill)
                    .height(Fill)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center);
                // Backdrop — dim wash that also dismisses the
                // modal on click. Captures the press so it
                // doesn't reach the session HUD beneath. The dim
                // fades with the panel, and the dismiss handler is
                // only armed while the modal is actually open so a
                // click mid-fade-out can't re-fire the close.
                let mut backdrop = mouse_area(
                    iced::widget::container(iced::widget::Space::new().width(Fill).height(Fill))
                        .width(Fill)
                        .height(Fill)
                        .style(anim::backdrop_style(0.45 * progress)),
                );
                if self.session.settings_anim.shown() {
                    backdrop = backdrop.on_press(|_| Message::Session(session::Message::CloseSettings));
                }
                iced::widget::stack![
                    Element::from(session_view),
                    Element::from(backdrop),
                    Element::from(placement),
                ]
                .into()
            } else {
                session_view
            };
            // Session entry rises into place; the scope's dy also
            // covers the way back out (the menu descends — see the
            // screen-swap match in `update`).
            let composed = match (enter, self.screen_enter_scope) {
                (Some(p), EnterScope::Root { dy }) => entered(composed, Some(p), dy),
                _ => composed,
            };
            return crate::input_capture::InputCapture::new(composed, |input| {
                // Esc is reserved as the in-session escape/menu key —
                // it never reaches the joyflag pipeline so the user
                // can't accidentally hide it behind a mapping.
                let is_escape = |k: &iced::keyboard::key::Physical| {
                    matches!(
                        k,
                        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape)
                    )
                };
                let ev = match input {
                    crate::input_capture::Input::Keyboard(kb) => match kb {
                        iced::keyboard::Event::KeyPressed { physical_key, .. } if is_escape(physical_key) => {
                            return Some(Message::Session(session::Message::EscPressed));
                        }
                        iced::keyboard::Event::KeyReleased { physical_key, .. } if is_escape(physical_key) => {
                            return None;
                        }
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
                        crate::gamepad::GamepadEvent::ButtonDown(b) => Some(session::InputEvent::Button {
                            button: crate::input::GamepadButton::from_sdl3(b),
                            pressed: true,
                        }),
                        crate::gamepad::GamepadEvent::ButtonUp(b) => Some(session::InputEvent::Button {
                            button: crate::input::GamepadButton::from_sdl3(b),
                            pressed: false,
                        }),
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

        let rescanning = self.is_rescanning();
        let body: Element<'_, Message> = match self.tab {
            Tab::Play => self
                .play
                .view(
                    lang,
                    &self.scanners,
                    &self.loadout,
                    self.loaded.as_ref(),
                    self.config.streamer_mode,
                    &self.config,
                    &self.netplay.phase,
                    &self.netplay.lobby,
                    self.netplay.handoff_pending(),
                    rescanning,
                    &self.lobby_swap,
                    self.lobby_exit_snapshot.as_ref(),
                )
                .map(Message::Play),
            Tab::Replays => self
                .replays
                .view(lang, &self.scanners, &self.config, &self.netplay.phase, rescanning)
                .map(Message::Replays),
            Tab::Patches => self
                .patches
                .view(lang, &self.scanners, &self.config, rescanning)
                .map(Message::Patches),
            Tab::Settings => tabs::settings::view(lang, &self.config, &self.settings, self.updater.status_blocking())
                .map(Message::Settings),
        };

        // Body content rides on the drawn cyberworld backdrop (the
        // Legacy Collection's ring-and-hex PET screen). The content
        // container itself paints no background, and the backdrop
        // sits in a layer underneath — so tab switches slide just
        // the content sideways while the cyberworld stays fixed
        // (the top bar stays put too); welcome/session swaps glide
        // the whole window up.
        let mut body_content: Element<'_, Message> = container(body)
            .width(Fill)
            .height(Fill)
            .style(widgets::body_surface)
            .into();
        if let (Some(p), EnterScope::Body { dx }) = (enter, self.screen_enter_scope) {
            body_content = anim::slide_in(body_content, p, iced::Vector::new(dx, 0.0));
        }
        let body_surface: Element<'_, Message> = iced::widget::stack![widgets::cyber_backdrop(), body_content]
            .width(Fill)
            .height(Fill)
            .into();
        // While a lobby is live and the user is on another tab, the
        // Play tab's nav pill carries a small attention dot so the
        // open lobby isn't forgotten behind a tab switch.
        let lobby_badge = self.lobby_on_screen() && self.tab != Tab::Play;
        let root: Element<'_, Message> = column![
            top_bar(lang, self.tab, lobby_badge, self.config.fullscreen),
            widgets::hud_scanline_top(),
            body_surface,
        ]
        .spacing(0)
        .width(Fill)
        .height(Fill)
        .into();
        match (enter, self.screen_enter_scope) {
            (Some(p), EnterScope::Root { dy }) => entered(root, Some(p), dy),
            _ => root,
        }
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

/// Apply the whole-window entrance glide to `el` while one is live;
/// pass-through otherwise. `dy` is the starting offset (positive =
/// rise in from below). Drawing-only — layout and hit-testing stay
/// at the rest position throughout.
fn entered(el: Element<'_, Message>, progress: Option<f32>, dy: f32) -> Element<'_, Message> {
    match progress {
        Some(p) => anim::slide_in(el, p, iced::Vector::new(0.0, dy)),
        None => el,
    }
}

fn top_bar(lang: &LanguageIdentifier, active: Tab, lobby_badge: bool, fullscreen: bool) -> Element<'_, Message> {
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
    let mut bar = row![
        iced::widget::container(
            Image::new(LOGO.clone())
                .width(iced::Length::Fixed(28.0))
                .height(iced::Length::Fixed(28.0))
                .content_fit(iced::ContentFit::Contain),
        )
        .padding([2, 8]),
        widgets::nav_tab_button_badged(
            Icon::Gamepad,
            t!(lang, "tab-play"),
            Message::TabSelected(Tab::Play),
            Tab::Play == active,
            lobby_badge,
        ),
        tab(Icon::Film, t!(lang, "tab-replays"), Tab::Replays),
        horizontal_space(),
        // Decorative hexagon burst — the Legacy Collection's
        // header motif, trailing off ahead of the utility tabs.
        // Sized just shy of the chips so it fills the band.
        widgets::hex_chain(32.0),
        // Patches + Settings = low-emphasis utility tabs.
        // Patch management is an occasional maintenance chore,
        // not a destination, so it doesn't get equal billing
        // with Play/Replays — icon-only on the right, with the
        // label exposed as a hover tooltip.
        widgets::nav_icon_tab_button(
            Icon::Puzzle,
            t!(lang, "tab-patches"),
            Message::TabSelected(Tab::Patches),
            Tab::Patches == active,
        ),
        widgets::nav_icon_tab_button(
            Icon::Settings,
            t!(lang, "tab-settings"),
            Message::TabSelected(Tab::Settings),
            Tab::Settings == active,
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if fullscreen {
        // Fullscreen is borderless — no OS title bar, so no native
        // X. Stand in for it at the same screen corner, in the
        // titlebar-close mood (quiet at rest, red on hover).
        bar = bar.push(widgets::icon_button_styled(
            Icon::X,
            t!(lang, "window-quit"),
            Some(Message::Quit),
            [8.0, 12.0],
            widgets::window_close,
        ));
    }
    container(bar.padding([10, 8]))
        .width(Fill)
        .style(widgets::hud_bar)
        .into()
}
