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

use crate::library::{game, patch, replays, rom, save};
use crate::netplay::identity;
use crate::platform::{audio, input, sdl_init};
use crate::ui::theme::theme_for;
use crate::ui::{anim, widgets};
use crate::{config, discord, i18n, loadout, netplay, selection, session, tabs, updater, INIT_LINK_CODE};
use i18n::t;
use iced::widget::container;
use iced::widget::space::horizontal as horizontal_space;
use iced::{Alignment, Element, Fill, Theme};
use sweeten::widget::{column, row};
use tabs::patches::PatchesState;
use tabs::play::{create_new_save, duplicate_save, rename_save};
use tabs::replays::ReplaysState;
use unic_langid::LanguageIdentifier;

/// Per-tab `update_*` message handlers (the bulk of the update logic),
/// split out of this file to keep `App` from being one giant module.
mod update;

/// Bundle of decoded-replay state the export task needs.
/// Pulled together synchronously in `start_replay_export` so the
/// spawned future doesn't have to touch `&self`.
struct ExportPrep {
    games: [crate::library::rom::GameRef; 2],
    roms: [Vec<u8>; 2],
    replay: tango_replay::Replay,
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

    /// Rescan all four collections. Each scanner is gated on a stat
    /// fingerprint of the tree it reads, so the automatic triggers
    /// (tab entry, session close) skip the full read-and-parse
    /// unless files actually changed — switching tabs costs four
    /// metadata walks, not a re-read of every ROM and save on disk.
    fn rescan(&self, config: &config::Config) {
        let roms_path = config.roms_path();
        let saves_path = config.saves_path();
        let patches_path = config.patches_path();
        let replays_path = config.replays_path();
        self.roms
            .rescan_if_changed(&rom::scan_roots(&roms_path), || Some(rom::scan_roms(&roms_path)));
        self.saves.rescan_if_changed(std::slice::from_ref(&saves_path), || {
            Some(save::scan_saves(&saves_path))
        });
        self.patches
            .rescan_if_changed(&patch::scan_roots(&patches_path), || patch::scan(&patches_path).ok());
        self.replays.rescan_if_changed(std::slice::from_ref(&replays_path), || {
            Some(replays::scan_replays(&replays_path))
        });
    }
}

pub struct App {
    config: config::Config,
    /// Background thread that owns the actual config-file writes; see
    /// [`config::Writer`]. `persist_config` queues snapshots on it.
    config_writer: config::Writer,
    tab: Tab,
    scanners: Scanners,
    /// Cloned into every session so they can bind their MGBAStream
    /// without owning the audio backend. The sdl Backend lives in
    /// `_audio_backend` so the underlying stream keeps playing.
    audio_binder: audio::LateBinder,
    /// Kept alive for the program's lifetime; dropping it would tear
    /// down the SDL audio stream and the app would go silent.
    /// Rebuilt by [`Self::reopen_audio_backend`] when the playback
    /// device topology changes under us.
    _audio_backend: Option<audio::sdl::Backend>,
    /// Pins SDL's audio subsystem (and with it the OS device-
    /// notification machinery that keeps the device list current) for
    /// the app's lifetime, and serves the 1 Hz topology poll. The
    /// backend holds its own subsystem handle, but if the backend
    /// failed to open — or dies and fails to reopen — this keeps
    /// device enumeration working so a later hotplug can still bring
    /// audio up.
    audio_subsystem: Option<sdl3::AudioSubsystem>,
    /// Last playback-device topology snapshot, diffed on the 1 Hz
    /// [`Message::AudioWatchTick`]; any difference triggers
    /// [`Self::reopen_audio_backend`].
    audio_device_ids: Vec<sdl3::audio::AudioDeviceID>,

    /// Owned game+save+assets for the current selection. Rebuilt only
    /// when game or save changes; per-frame view() borrows it.
    loaded: Option<selection::Loaded>,

    /// The local loadout (family / game / save + patch overlay) —
    /// App-level so the lobby settings-resend sees every change the
    /// Play tab's selector makes.
    loadout: loadout::Loadout,
    play: tabs::play::State,
    replays: ReplaysState,
    /// In-flight replay-analysis workers ([`Effect::AnalyzeReplay`]),
    /// keyed by replay path: the flag stops the blocking simulation,
    /// the handle aborts its progress stream. Removed when the worker
    /// completes naturally, or cancelled by `replay_stats_takeover`
    /// when a playback session's prefetcher takes the same work over.
    replay_analysis_jobs: std::collections::HashMap<
        std::path::PathBuf,
        (std::sync::Arc<std::sync::atomic::AtomicBool>, iced::task::Handle),
    >,
    patches: PatchesState,
    settings: tabs::settings::State,
    welcome: tabs::welcome::State,
    netplay: netplay::State,
    /// Persistent self-signed client identity, loaded once at startup
    /// (see [`crate::netplay::identity`]). Cloned into each matchmaking
    /// `Connect` so the lobby websocket presents it as the mTLS client
    /// certificate. `None` if it couldn't be loaded/created — netplay
    /// then dials without a client cert.
    identity: Option<tango_signaling::ClientIdentity>,

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
    /// Background loop that re-fetches the patch index every 15 min
    /// (a conditional GET of metadata, not the packages) and refreshes
    /// the patches scanner in place.
    patch_autoupdater: patch::Autoupdater,
    /// A replay whose playback is waiting on a patch download. Set by
    /// `watch_replay`, resumed once the install rescan lands.
    pending_watch: Option<std::path::PathBuf>,
    /// In-flight and failed patch downloads. App-level because the
    /// patches tab, the lobby, replay playback and the play tab's picker
    /// all start them, and two tabs render them.
    downloads: patch::Downloads,
    /// Self-updater. Polls GitHub every 30 min, streams the
    /// platform installer into the cache dir, and on the
    /// `finish_update` call (or next launch) hands off to the
    /// installer. UI lives in Settings → About; toggle is in
    /// Settings → Network.
    updater: updater::Updater,
    /// Number of in-flight `rescan_off_thread` tasks. Gates the
    /// automatic rescan trigger (tab entry) so it doesn't stack
    /// workers, and the welcome screen's rescan button. A counter
    /// (not a bool) because rescans can overlap
    /// (e.g. the patch autoupdater fires its own rescan separately
    /// from an automatic one).
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
    lobby_exit_snapshot: Option<(netplay::Phase, netplay::LobbyState, netplay::ReadyView)>,
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
    /// The post-match results card (no session active, but
    /// `session.results` is set).
    Results,
    Tabs(Tab),
}

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

/// Open a path in the OS file manager / default handler, logging on failure.
/// Shared by the per-tab `OpenPath` effects.
fn open_path(path: impl AsRef<std::path::Path>) -> iced::Task<Message> {
    let path = path.as_ref();
    if let Err(e) = open::that(path) {
        log::error!("open {}: {e}", path.display());
    }
    iced::Task::none()
}

impl App {
    /// The analysis rounds to draw as a playback session's hover strip:
    /// the Replays tab's already-cooked chart when it has one (that
    /// covers an in-flight analysis too — the tab seeds a planned empty
    /// frame and re-cooks on every progress message), else cooked fresh
    /// from the stats sidecar, else empty (no stats — no strip). `s` is
    /// the replay's session: the fresh cook reconstructs the planned
    /// round spans from its boundary map, keeping the strip on the
    /// scrubber's exact tick scale even when the tab never cooked this
    /// replay (e.g. watched straight from the results screen).
    fn replay_chart_for(&self, path: &std::path::Path, s: &session::replay::ReplaySession) -> session::ReplayChart {
        if let Some(c) = self.replays.hp_charts.get(path) {
            return session::ReplayChart {
                path: path.to_path_buf(),
                rounds: c.rounds.clone(),
            };
        }
        let rounds = replays::load_match_stats(&self.config.cache_path(), &self.config.replays_path(), path)
            .map(|stats| widgets::cook_hp_rounds(&stats, [None, None], Some(&planned_spans(s))).0)
            .unwrap_or_default();
        session::ReplayChart {
            path: path.to_path_buf(),
            rounds,
        }
    }

    /// Stats duty for a playback session about to start on `path`. With
    /// no readable stats sidecar, the session's prefetcher — which runs
    /// the very simulation the analysis needs anyway — takes the
    /// analysis over: any in-flight tab worker for this replay is
    /// cancelled (its simulation stops, its progress stream aborts
    /// mid-air so the tab's pending marker survives the handover), and
    /// the returned job + progress-stream task plug the prefetcher into
    /// the tab's usual `HpStatsPartial`/`HpStatsLoaded` pipeline. With
    /// a sidecar on disk there is nothing to compute — `(None, none)`.
    fn replay_stats_takeover(
        &mut self,
        path: &std::path::Path,
    ) -> (Option<session::replay::PrefetchStatsJob>, iced::Task<Message>) {
        if replays::load_match_stats(&self.config.cache_path(), &self.config.replays_path(), path).is_some() {
            return (None, iced::Task::none());
        }
        if let Some((cancel, handle)) = self.replay_analysis_jobs.remove(path) {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            handle.abort();
        }
        // Marked pending so a tab focus during playback doesn't spawn a
        // duplicate worker; the prefetch stream's completion clears it.
        self.replays.hp_pending.insert(path.to_path_buf());

        let (partial_tx, partial_rx) = futures::channel::mpsc::unbounded::<tango_match::analysis::MatchStats>();
        let done: std::sync::Arc<std::sync::Mutex<Option<tango_match::analysis::MatchStats>>> = Default::default();
        let job = session::replay::PrefetchStatsJob {
            partial_tx,
            done: done.clone(),
            stats_file: replays::stats_path(&self.config.cache_path(), &self.config.replays_path(), path),
        };
        use futures::StreamExt;
        let progress_path = path.to_path_buf();
        let path = path.to_path_buf();
        let stream = partial_rx
            .map(move |partial| tabs::replays::Message::HpStatsPartial(progress_path.clone(), partial))
            .chain(futures::stream::once(async move {
                tabs::replays::Message::HpStatsLoaded(path, done.lock().unwrap().take())
            }));
        (Some(job), iced::Task::stream(stream).map(Message::Replays))
    }
}

/// The planned per-round tick spans (= the recorded round lengths) of a
/// playback session, reconstructed from its boundary map — the layout
/// frame `widgets::cook_hp_rounds` needs so a chart cooked for the
/// session's analysis strip shares the scrubber's exact tick scale.
fn planned_spans(s: &session::replay::ReplaySession) -> Vec<u32> {
    let boundaries = s.round_boundaries();
    std::iter::once(0)
        .chain(boundaries.iter().copied())
        .zip(boundaries.iter().copied().chain(std::iter::once(s.total_ticks())))
        .map(|(a, b)| b.saturating_sub(a))
        .collect()
}

/// Reveal a file in the OS file manager with the file itself selected,
/// rather than opening its containing folder anonymously. Shared by the
/// per-tab `RevealPath` effects (replays, saves).
fn reveal_path(path: impl AsRef<std::path::Path>) -> iced::Task<Message> {
    // opener::reveal blocks until the platform helper finishes; run it off
    // the update loop so a wedged file manager can't stall the UI.
    let path = path.as_ref().to_path_buf();
    std::thread::spawn(move || {
        if let Err(e) = opener::reveal(&path) {
            log::error!("reveal {}: {e}", path.display());
        }
    });
    iced::Task::none()
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
            scanners.patches.read().installed.len(),
            scanners.replays.read().len(),
        );

        // Restore the last selection from config, but only the bits
        // that still resolve against the current scanners.
        let mut restored = loadout::Loadout {
            // Restore the selected family (drives the picker even when no
            // owned-ROM game resolves under it); falls back to the family of
            // `last_game` for configs written before `last_family` existed.
            family: config
                .last_family
                .as_deref()
                .and_then(game::family_static)
                .or_else(|| config.last_game.as_ref().and_then(|(f, _)| game::family_static(f))),
            ..Default::default()
        };
        if let Some((family, variant)) = config.last_game.as_ref() {
            if let Some(game) = crate::library::game::find_by_family_and_variant(family, *variant) {
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
                                let ok = patches.supported_games(n, v).contains(&game);
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
        let welcome = tabs::welcome::State::from_nickname(config.nickname.as_deref());

        // Spin up the SDL audio backend once at startup with the
        // LateBinder as the source. Sessions later bind their
        // MGBAStream into the binder and the SDL stream keeps going
        // across selections.
        //
        // The subsystem pin must be taken before `Backend::new`, which
        // borrows the global `Sdl` itself — holding our guard across it
        // would deadlock `sdl_init`'s mutex. It outlives the backend so
        // the 1 Hz playback-device topology poll (see
        // `reopen_audio_backend`) works for the app's whole life.
        let audio_subsystem = {
            let sdl = sdl_init::sdl();
            sdl.and_then(|sdl| match sdl.audio() {
                Ok(a) => Some(a),
                Err(e) => {
                    log::warn!("audio: subsystem pin failed: {e}");
                    None
                }
            })
        };
        let mut audio_binder = audio::LateBinder::new();
        audio_binder.set_volume(config.volume);
        let audio_backend = match audio::sdl::Backend::new(audio_binder.clone()) {
            Ok(b) => {
                audio_binder.set_sample_rate(b.sample_rate());
                log::info!("audio: sdl backend up at {} Hz", b.sample_rate());
                Some(b)
            }
            Err(e) => {
                log::warn!("audio: sdl init failed, running silent: {e:?}");
                None
            }
        };
        let audio_device_ids = audio_subsystem
            .as_ref()
            .map(audio::sdl::playback_device_ids)
            .unwrap_or_default();

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
            play.adopt_link_code(code.clone());
        }

        let identity = identity::load();

        let mut app = Self {
            config,
            config_writer: config::Writer::new(),
            tab: Tab::Play,
            welcome,
            settings: tabs::settings::State::default(),
            scanners,
            audio_binder,
            _audio_backend: audio_backend,
            audio_subsystem,
            audio_device_ids,
            loaded: None,
            loadout: restored,
            play,
            replays: ReplaysState::default(),
            replay_analysis_jobs: Default::default(),
            patches: PatchesState::default(),
            session: session::State::new(),
            netplay: netplay::State::new(),
            identity,
            discord: discord::Client::new(),
            session_started_at: None,
            patch_autoupdater,
            pending_watch: None,
            downloads: patch::Downloads::new(),
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
        (app, iced::Task::batch([stats_task]))
    }

    /// Drops cached replay stats for paths that no longer exist in
    /// the latest scan, then kicks the worker for any newly-scanned
    /// paths that don't have stats yet. Returns tab-scoped Task —
    /// caller wraps with `.map(Message::Replays)` if at App level.
    fn refresh_replay_stats(&mut self) -> iced::Task<tabs::replays::Message> {
        let live: std::collections::HashSet<std::path::PathBuf> =
            self.scanners.replays.read().iter().map(|r| r.path.clone()).collect();
        self.replays.stats.retain(|p, _| live.contains(p));
        self.replays.hp_charts.retain(|p, _| live.contains(p));
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

    /// Queue `self.config` for persistence on the background writer —
    /// the render thread never blocks on the disk. Write failures are
    /// logged by the writer thread.
    fn persist_config(&self) {
        self.config_writer.write(self.config.clone());
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
    /// `view` call (and auto-rescan trigger) sees the rescan as
    /// live — without this, back-to-back triggers would stack
    /// workers until the first one actually gets scheduled.
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
    /// still in flight. Gates the automatic rescan triggers and the
    /// welcome screen's rescan button.
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
        if !self.netplay.local_ready() {
            return iced::Task::none();
        }
        let (Some(local), Some(remote)) = (self.netplay.lobby.local.as_ref(), self.netplay.lobby.remote.as_ref())
        else {
            return iced::Task::none();
        };
        let roms = self.scanners.roms.read();
        let patches = self.scanners.patches.read();
        let verdict = netplay::compat::check(local, remote, &roms, &patches);
        if matches!(verdict, netplay::compat::Verdict::Compatible) {
            return iced::Task::none();
        }
        iced::Task::done(Message::Netplay(netplay::Message::Uncommit))
    }

    /// Fetch a patch the lobby needs but doesn't have.
    ///
    /// The compatibility check resolves the peer's patch from the repo
    /// index, so we know a matchup is playable before the package is on
    /// disk — and the only thing standing in the way is a download we
    /// can start ourselves. Idempotent: the tab tracks in-flight
    /// downloads, and this fires on every lobby state change.
    fn fetch_missing_patch(&mut self) -> iced::Task<Message> {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        let (Some(local), Some(remote)) = (self.netplay.lobby.local.as_ref(), self.netplay.lobby.remote.as_ref())
        else {
            return iced::Task::none();
        };
        let verdict = {
            let roms = self.scanners.roms.read();
            let patches = self.scanners.patches.read();
            netplay::compat::check(local, remote, &roms, &patches)
        };
        let Some((name, version)) = verdict.fetchable() else {
            return iced::Task::none();
        };
        let key = (name.to_owned(), version.clone());
        log::info!("lobby needs {} {}, fetching", key.0, key.1);
        self.install_patch(key)
    }

    /// Build a `protocol::Settings` packet from the App's current
    /// state: nickname from config, match_type defaults to (0, 0),
    /// game_info from the local loadout. (No available-games /
    /// available-patches lists cross the wire — possession of the
    /// peer's setup is checked locally by `compat::check`.)
    fn make_local_settings(&self) -> tango_net_protocol::control::Settings {
        self.loadout.make_local_settings(&self.config, &self.netplay.lobby)
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

    /// Recompute `self.loaded` from the loadout's game + save +
    /// patch[+version]. Cheap when nothing's changed; expensive when
    /// ROM/assets need a fresh parse (BPS + asset parsing + icon
    /// decode), which is why we don't call it from view().
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
            // it on disk and a rescan noticed). Drop the stale
            // selection so the picker stops showing a missing entry.
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
                .version(&name, &version)
                .map(|v| (name.clone(), version.clone(), v.clone()))
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
        // The commit path takes the early-return above and never
        // reaches here, so this only fires on a real selection change.
        self.play.reset_save_editing();
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
    /// Carries the freshly-constructed PvP session (plus its setup-pane
    /// presentation state and audio binding) back into the App after the
    /// async build task in `spawn_pvp` resolves. `Slot` because
    /// PvpSession isn't Clone.
    #[allow(clippy::type_complexity)]
    PvpSessionBuilt(
        netplay::Slot<anyhow::Result<(session::pvp::PvpSession, session::PvpPanes, Option<audio::Binding>)>>,
    ),
    /// 1 Hz tick: refresh Discord rich-presence + drain any
    /// Discord-initiated join secret into the play link-code
    /// field.
    DiscordTick,
    /// 1 Hz audio housekeeping tick: diff the playback-device
    /// topology and reopen the output stream on the current default
    /// device if it moved. Emulation is paced by that stream's
    /// callbacks, so this is also what unfreezes a running session
    /// whose endpoint died (e.g. Voicemeeter's virtual device
    /// dropping on an engine restart).
    AudioWatchTick,
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
    /// a rescan with the Replays tab on screen also warms the stats
    /// cache, and the save-delete handler asks for a fresh "first
    /// save" pick now that the scan results are in.
    Rescanned(RescanFollowup),
}

/// Per-call-site cue for `Message::Rescanned`. Lets one handler
/// arm cover every rescan we kick off without dispatching a
/// distinct Message variant per call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescanFollowup {
    /// A patch a replay was waiting on just installed — start playback
    /// now that the scan can see it.
    RetryPendingWatch,
    /// Just re-validate `self.loaded` against the fresh scan.
    Refresh,
    /// Refresh + warm the replays-tab stats cache (used when a
    /// rescan runs with the Replays tab on screen, and after a PvP
    /// session closes).
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
        } else if self.session.results.is_some() {
            ScreenKey::Results
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
        let lobby_live = self.lobby_on_screen().then(|| {
            (
                self.netplay.phase.clone(),
                self.netplay.lobby.clone(),
                self.netplay.ready_view(),
            )
        });
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
        // A different family swaps the entire bottom of the tab —
        // rise the whole save-view pane in. A different game or save
        // within the family only re-renders the save's content — rise
        // just the panes under the save view's sub-tab strip, leaving
        // the strip itself planted. (Sub-tab switches slide the inner
        // panes horizontally instead; see save_view::State::apply.)
        if family_before != self.loadout.family {
            self.play.animate_family_switch(now);
        } else if selection_before != (self.loadout.game, self.loadout.save.clone()) {
            self.play.animate_save_switch(now);
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
            Message::Quit => {
                // Complete any queued config write before the runtime
                // tears down (Drop also flushes, as the backstop for the
                // window-close exit path).
                self.config_writer.flush();
                iced::exit()
            }
            Message::TabSelected(t) => {
                let entered = self.tab != t;
                self.tab = t;
                // A tab switch unmounts the input settings pane's capture
                // wrapper, so key/button releases stop arriving — drop the
                // held set rather than show stale-lit binding chips on the
                // way back.
                self.settings.held = Default::default();
                // Entering a scanner-backed tab re-runs the scan in the
                // background — there are no Rescan buttons; this is how
                // new files on disk get noticed. Cheap when nothing
                // changed (stat-fingerprint gated, see Scanners::rescan).
                // Settings doesn't read the scanners, so skip it there.
                if entered && t != Tab::Settings && !self.is_rescanning() {
                    // Entering Replays also warms the stats cache, so
                    // newly-recorded replays get their stats line
                    // without a manual nudge.
                    return self.rescan_off_thread(if t == Tab::Replays {
                        RescanFollowup::RefreshAndReplayStats
                    } else {
                        RescanFollowup::Refresh
                    });
                }
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
            Message::AudioWatchTick => {
                // Pump (without draining — polling here would eat
                // gamepad events meant for `gamepad::pump`) so SDL
                // runs its deferred device bookkeeping: device
                // *removals* are processed as queued main-thread
                // callbacks inside SDL_PumpEvents, and the redraw-
                // driven pump may be silent exactly when the output
                // device just died. Additions land in the list
                // directly from SDL's notification thread.
                if let Some(mut pump) = sdl_init::event_pump() {
                    pump.pump_events();
                }
                if let Some(subsystem) = &self.audio_subsystem {
                    let ids = audio::sdl::playback_device_ids(subsystem);
                    if ids != self.audio_device_ids {
                        self.audio_device_ids = ids;
                        self.reopen_audio_backend();
                    }
                }
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
                        // Only remember position while fullscreen.
                        // Entering fullscreen parks the window at its
                        // monitor's origin and fires Moved (with
                        // fullscreen already set, see C::Fullscreen) —
                        // so the persisted value identifies the
                        // fullscreen monitor for the next launch.
                        // Windowed positions are deliberately not
                        // persisted: restoring an exact x/y is janky on
                        // multi-monitor setups (saved coords can land
                        // off-screen or on the wrong display).
                        if self.config.fullscreen {
                            self.config.last_window_position = Some((point.x, point.y));
                            self.persist_config();
                        }
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
                if let session::Message::Pvp(session::view::pvp::Message::SetFrameDelay(d)) = &m {
                    self.config.frame_delay = *d;
                    self.persist_config();
                }
                // Replay input display toggle: the flag lives in config
                // (so the choice sticks across replays); the session
                // handler itself is a no-op.
                if let session::Message::Replay(session::view::replay::Message::ToggleInputDisplay) = &m {
                    self.config.show_replay_inputs = !self.config.show_replay_inputs;
                    self.persist_config();
                }
                // Same deal for the opponent-screen PiP toggle — the session
                // handler flips the live session's state; this keeps the
                // choice sticking across replays.
                if let session::Message::Replay(session::view::replay::Message::TogglePip) = &m {
                    self.config.show_opponent_pip = !self.config.show_opponent_pip;
                    self.persist_config();
                }
                // The transport bar's Export-clip chip: everything
                // positional is captured NOW, while the session is alive —
                // the jump-start snapshot nearest the span start and the
                // session's round boundaries — then the save dialog runs
                // async and the pick flows through the replays tab's
                // export-job machinery (progress, cancel, and the panel all
                // live there). The session handler itself is a no-op.
                if let session::Message::Replay(session::view::replay::Message::ExportClip { start, end }) = &m {
                    let (start, end) = (*start, *end);
                    let Some(path) = self.session.replay_chart.as_ref().map(|c| c.path.clone()) else {
                        return iced::Task::none();
                    };
                    let (snapshot, round_marks) = self
                        .session
                        .active_as::<session::replay::ReplaySession>()
                        .map(|s| (s.clip_start_snapshot(start), s.round_boundaries()))
                        .unwrap_or_default();
                    let clip = crate::replay_export::Clip {
                        start,
                        end,
                        snapshot,
                        round_marks,
                    };
                    let lossless = self.replays.export_settings.scale == 0;
                    let replay_for_msg = path.clone();
                    return self.export_save_dialog(path, lossless, "-clip", move |output| {
                        tabs::replays::Message::Export(tabs::replays::ExportMessage::StartClip {
                            replay: replay_for_msg.clone(),
                            output,
                            clip: clip.clone(),
                        })
                    });
                }
                // The clip strip's cancel: forward to the replays tab's
                // own cancel handler — the job and its canceller live
                // there, whichever surface started the export.
                if let session::Message::Replay(session::view::replay::Message::CancelClipExport) = &m {
                    if let Some(path) = self.session.replay_chart.as_ref().map(|c| c.path.clone()) {
                        return self.update_replays(tabs::replays::Message::Export(
                            tabs::replays::ExportMessage::Cancel(path),
                        ));
                    }
                    return iced::Task::none();
                }
                // Results screen's Watch button: building a playback session
                // needs the scanners + config, so it's handled here (the
                // session module's handler is a no-op). The results stay set
                // underneath — closing the replay lands back on them. On
                // failure (e.g. the replay is still flushing or unreadable),
                // log and leave the results screen up.
                if let session::Message::Results(session::view::results::Message::WatchReplay) = &m {
                    if let Some(path) = self.session.results.as_ref().and_then(|r| r.replay_path.clone()) {
                        let (stats_job, stats_task) = self.replay_stats_takeover(&path);
                        match session::build_playback(
                            &self.scanners,
                            &self.config,
                            &self.audio_binder,
                            &path,
                            stats_job,
                        ) {
                            Ok((s, audio)) => {
                                self.session.replay_chart = Some(self.replay_chart_for(&path, &s));
                                self.session.active = Some(Box::new(s));
                                self.session.audio_binding = audio;
                                self.session.session_installed();
                            }
                            // The dropped job closes its stream, whose
                            // completion message clears the tab's pending
                            // marker — a later focus retries the analysis.
                            Err(e) => log::warn!("failed to play replay {}: {e}", path.display()),
                        }
                        return stats_task;
                    }
                    return iced::Task::none();
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
                let was_sp = self
                    .session
                    .active_as::<session::singleplayer::SinglePlayerSession>()
                    .is_some();
                // Snapshot "was PvP" before dispatch — PvP
                // sessions can auto-tear-down inside
                // `UpdateFramebuffer` (peer-end / disconnect /
                // grace timeout), not just from a Close message.
                // We trigger the replay rescan whenever a PvP
                // session was active before and isn't after.
                let was_pvp = self.session.active_as::<session::pvp::PvpSession>().is_some();
                let task = self.session.update(m, &self.config.input_mapping).map(Message::Session);
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
                let local_game = self.loadout.game;
                let local_patch = self.loadout.patch.clone().zip(self.loadout.patch_version.clone());
                iced::Task::perform(
                    async move {
                        let Some(local_game) = local_game else {
                            return Err(anyhow::anyhow!("no local game selected"));
                        };
                        session::spawn_pvp(scanners, config, audio_binder, local_game, local_patch, pre_match).await
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
                let fetch = self.fetch_missing_patch();
                iced::Task::batch([task, resend, uncommit, fetch, attention])
            }
            Message::PvpSessionBuilt(slot) => {
                let Some(result) = slot.lock().unwrap().take() else {
                    return iced::Task::none();
                };
                match result {
                    Ok((session, panes, audio)) => {
                        // Both setup drawers start closed — the edge
                        // handles are the invitation; a pane that
                        // barges in over the match start isn't.
                        // Except when the user opted in: slide the
                        // opponent's drawer open at match start if
                        // their setup is actually visible.
                        let auto_open = self.config.show_opponent_setup && panes.opponent_loaded.is_some();
                        self.session.active = Some(Box::new(session));
                        self.session.pvp_panes = Some(panes);
                        self.session.audio_binding = audio;
                        if auto_open {
                            self.session.opponent_panel.open();
                        } else {
                            self.session.opponent_panel.close();
                        }
                        self.session.self_panel.close();
                        self.session.session_installed();
                        // Drop the post-handoff lobby snapshot now
                        // that the PvP view is taking over the
                        // screen. take_pre_match deliberately left
                        // it in place so the bottom strip didn't
                        // flash blank while spawn_pvp ran.
                        self.netplay.finish_handoff();
                    }
                    Err(e) => {
                        // Surface the failure where every other netplay
                        // failure lands: the lobby band's sticky Failed
                        // status, which is still on screen (the handoff
                        // kept it up while the session was built).
                        log::error!("pvp session build failed: {e:#}");
                        self.netplay.fail_session_build(netplay::Error::Other(format!("{e:#}")));
                    }
                }
                iced::Task::none()
            }
            Message::Rescanned(followup) => {
                self.rescans_in_flight = self.rescans_in_flight.saturating_sub(1);
                match followup {
                    RescanFollowup::Refresh => {
                        self.refresh_loaded();
                        iced::Task::none()
                    }
                    RescanFollowup::RetryPendingWatch => {
                        self.refresh_loaded();
                        match self.pending_watch.take() {
                            Some(path) => self.watch_replay(path),
                            None => iced::Task::none(),
                        }
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
            // 1 Hz audio-device housekeeping — a device-list snapshot
            // compare unless a playback device actually came or went.
            // A poll rather than SDL's AudioDeviceAdded/Removed events
            // because those only flush from SDL's pending list when
            // the event pump runs, and our pump is redraw-driven —
            // possibly silent during the very freeze this recovers.
            iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::AudioWatchTick),
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
        // The input settings pane needs redraws too: its live binding
        // highlight polls the gamepad from the SDL pump, which only runs
        // on RedrawRequested — with no session (whose vblank notify
        // drives redraws) and no animation, a pad press would otherwise
        // sit unseen until some other event happened to redraw.
        let input_pane_on_screen = !self.session.is_active()
            && self.tab == Tab::Settings
            && self.settings.active_tab == tabs::settings::SettingsTab::Input;
        if anim::any_active() || waiting_pulse_on_screen || input_pane_on_screen {
            subs.push(iced::window::frames().map(|_| Message::AnimTick));
        }
        iced::Subscription::batch(subs)
    }

    /// A playback device came or went: reopen the SDL output stream so
    /// it re-acquires whatever the default device now is. SDL migrates
    /// a default-device stream across a default *change* on its own,
    /// but it can't resurrect a stream whose endpoint died (USB DAC
    /// unplugged, Voicemeeter's virtual device dropping on an engine
    /// restart) — and since emulation is paced by this stream's
    /// callbacks, a dead stream freezes any running core. The new
    /// backend is built before the old one drops so the audio
    /// subsystem never quits mid-swap; sessions stay bound through the
    /// LateBinder and simply start receiving `fill` calls again, which
    /// is also what wakes a core parked in mgba's audio sync.
    fn reopen_audio_backend(&mut self) {
        log::info!("audio: playback device topology changed, reopening output stream");
        match audio::sdl::Backend::new(self.audio_binder.clone()) {
            Ok(b) => {
                self.audio_binder.set_sample_rate(b.sample_rate());
                log::info!("audio: sdl backend reopened at {} Hz", b.sample_rate());
                self._audio_backend = Some(b);
            }
            // Keep whatever we had: the change may have been an
            // unrelated device vanishing mid-enumeration, and a stream
            // that does turn out dead is no worse held than dropped.
            // The next topology change (e.g. a device coming back)
            // retries; no per-tick retry, so a deviceless machine
            // doesn't log-spam.
            Err(e) => log::warn!("audio: reopen failed, keeping previous stream: {e:?}"),
        }
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
                self.play.adopt_link_code(secret);
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

        if let Some(active) = self.session.active.as_deref() {
            let start = self.session_started_at.unwrap_or_else(std::time::SystemTime::now);
            return if active.is::<session::replay::ReplaySession>() {
                discord::make_base_activity(None)
            } else if active.is::<session::singleplayer::SinglePlayerSession>() {
                discord::make_single_player_activity(start, lang, game_info)
            } else {
                discord::make_in_progress_activity(start, lang, game_info)
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
            return anim::slide_in_opt(
                iced::widget::stack![widgets::cyber_backdrop(), welcome]
                    .width(Fill)
                    .height(Fill),
                enter,
                iced::Vector::new(0.0, ROOT_SLIDE),
            );
        }

        if self.session.is_active() {
            // Deliver keyboard + gamepad input through the
            // synchronous widget path so each event reaches
            // `program.update()` on the same winit iteration it
            // arrived in. Going through subscriptions would
            // round-trip through an `mpsc::try_send` and cost ~1
            // winit iteration of input lag per event.
            // The watched replay's export job (whole-replay or clip
            // alike), digested for the transport bar's clip strip —
            // the job itself stays owned by the replays tab.
            let clip_job = self
                .session
                .replay_chart
                .as_ref()
                .and_then(|c| self.replays.job(&c.path))
                .map(|j| session::view::ClipJob {
                    completed: j.completed,
                    total: j.total,
                    result: j.result.as_ref().map(|r| match r {
                        Ok(_) => Ok(()),
                        Err(e) => Err(e.as_str()),
                    }),
                    cancelling: j.canceller.is_cancelled() && j.result.is_none(),
                });
            let session_view = session::view::view(
                lang,
                &self.session,
                self.config.fractional_scaling,
                self.config.hide_emulator_border,
                self.config.show_replay_inputs,
                clip_job,
                crate::platform::video::effects::effect_for(&self.config.video_filter),
            )
            .map(Message::Session);
            // In-session settings modal: floats centered over the
            // running session with a dimmed click-to-dismiss
            // backdrop. The emulator keeps running underneath.
            // Rendered while the open/close transition is in
            // flight too, so the panel eases in and out.
            let composed: Element<'_, Message> = if self.session.settings.visible(now) {
                let progress = self.session.settings.progress(now);
                // The session's own InputCapture wrapper + vblank pump
                // already track every key/button in `input_held`, so the
                // input pane's live binding highlight reads from that
                // instead of pumping its own.
                let body = tabs::settings::view(
                    lang,
                    &self.config,
                    &self.settings,
                    self.updater.status_blocking(),
                    Some(&self.session.input_held),
                )
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
                let heading = iced::widget::text(t!(lang, "tab-settings")).size(crate::ui::style::TEXT_HEADING);
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
                // Dim wash + click-swallow + centered placement come
                // from the shared scaffolding; the dismiss handler is
                // only armed while the modal is actually open so a
                // click mid-fade-out can't re-fire the close.
                let modal = widgets::modal_layer(
                    anim::pop(modal_panel, progress, 12.0),
                    0.45 * progress,
                    Message::NoOp,
                    self.session
                        .settings
                        .shown()
                        .then_some(Message::Session(session::Message::CloseSettings)),
                );
                iced::widget::stack![Element::from(session_view), modal].into()
            } else {
                session_view
            };
            // Session entry rises into place; the scope's dy also
            // covers the way back out (the menu descends — see the
            // screen-swap match in `update`).
            let composed = match (enter, self.screen_enter_scope) {
                (Some(p), EnterScope::Root { dy }) => anim::slide_in(composed, p, iced::Vector::new(0.0, dy)),
                _ => composed,
            };
            return crate::platform::input_capture::InputCapture::new(composed, |input| {
                // Esc is reserved as the in-session escape/menu key —
                // it never reaches the joyflag pipeline so the user
                // can't accidentally hide it behind a mapping. Both
                // edges are routed: press peels overlays and arms
                // hold-to-quit, release disarms it.
                let is_escape = |k: &iced::keyboard::key::Physical| {
                    matches!(
                        k,
                        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape)
                    )
                };
                if let crate::platform::input_capture::Input::Keyboard(kb) = &input {
                    match kb {
                        iced::keyboard::Event::KeyPressed { physical_key, .. } if is_escape(physical_key) => {
                            return Some(Message::Session(session::Message::EscPressed));
                        }
                        iced::keyboard::Event::KeyReleased { physical_key, .. } if is_escape(physical_key) => {
                            return Some(Message::Session(session::Message::EscReleased));
                        }
                        _ => {}
                    }
                }
                input.to_event().map(|ev| Message::Session(session::Message::Input(ev)))
            })
            .into();
        }

        // Post-match results: a full-screen moment between the session and
        // the tabs — same chrome-less cyberworld composition as the welcome
        // screen. The ScreenKey change animates the swap in both directions.
        if let Some(results) = self.session.results.as_ref() {
            let results_view =
                session::view::results_view(lang, results).map(|m| Message::Session(session::Message::Results(m)));
            let composed: Element<'_, Message> = iced::widget::stack![widgets::cyber_backdrop(), results_view]
                .width(Fill)
                .height(Fill)
                .into();
            let composed = match (enter, self.screen_enter_scope) {
                (Some(p), EnterScope::Root { dy }) => anim::slide_in(composed, p, iced::Vector::new(0.0, dy)),
                _ => composed,
            };
            // Esc dismisses — through the same synchronous capture wrapper
            // the session uses, so it works without any widget focused.
            return crate::platform::input_capture::InputCapture::new(composed, |input| {
                if let crate::platform::input_capture::Input::Keyboard(iced::keyboard::Event::KeyPressed {
                    physical_key,
                    ..
                }) = &input
                {
                    if matches!(
                        physical_key,
                        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape)
                    ) {
                        return Some(Message::Session(session::Message::Results(
                            session::view::results::Message::Dismiss,
                        )));
                    }
                }
                None
            })
            .into();
        }

        let body: Element<'_, Message> = match self.tab {
            Tab::Play => {
                let main = self
                    .play
                    .view(
                        lang,
                        &self.scanners,
                        &self.loadout,
                        self.loaded.as_ref(),
                        self.config.streamer_mode,
                        &self.config,
                        &self.downloads,
                        tabs::play::LobbyBandCtx {
                            phase: &self.netplay.phase,
                            lobby: &self.netplay.lobby,
                            ready: self.netplay.ready_view(),
                            handoff_pending: self.netplay.handoff_pending(),
                            swap: &self.lobby_swap,
                            exit_snapshot: self.lobby_exit_snapshot.as_ref(),
                        },
                    )
                    .map(Message::Play);
                container(main).width(Fill).height(Fill).into()
            }
            Tab::Replays => self
                .replays
                .view(lang, &self.scanners, &self.config, &self.netplay.phase)
                .map(Message::Replays),
            Tab::Patches => self
                .patches
                .view(lang, &self.scanners, &self.config, &self.downloads)
                .map(Message::Patches),
            Tab::Settings => {
                tabs::settings::view(lang, &self.config, &self.settings, self.updater.status_blocking(), None)
                    .map(Message::Settings)
            }
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
            (Some(p), EnterScope::Root { dy }) => anim::slide_in(root, p, iced::Vector::new(0.0, dy)),
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
        let raw: &'static [u8] = include_bytes!("../icon.png");
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
