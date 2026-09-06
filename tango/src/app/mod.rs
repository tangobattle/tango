//! Desktop application state, initialization, and subscriptions.
//!
//! [`message`] defines the event vocabulary and navigation destinations.
//! [`dispatch`] routes events and coordinates screen transitions;
//! [`update`] handles tab effects, and [`view`] renders the shell.
//! The host wires these methods into `iced::application` in `main.rs`.

use crate::library::{autoupdate, game, patch, replays, rom, Scanners};
use crate::platform::{audio, input};
use crate::ui::anim;
use crate::{config, discord, i18n, loadout, netplay, selection, session, tabs, updater, INIT_LINK_CODE};
use i18n::t;
use tabs::patches::PatchesState;
use tabs::play::{create_new_save, duplicate_save, rename_save};
use tabs::replays::ReplaysState;

/// Backstop for orderly process exit. The session's `Goodbye` send is capped
/// at one second; leave a little scheduling slack, but never let a wedged or
/// panicked supervisor strand a window the user already asked to close.
const PVP_EXIT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

mod dispatch;
mod message;
mod update;
mod view;

pub use message::{Message, RescanFollowup, Tab};

/// Bundle of decoded-replay state the export task needs.
/// Pulled together synchronously in `start_replay_render` so the
/// spawned future doesn't have to touch `&self`.
struct ExportPrep {
    games: [crate::library::rom::GameRef; 2],
    roms: [Vec<u8>; 2],
    replay: tango_replay::Replay,
}

pub struct App {
    config: config::Config,
    /// Background thread that owns the actual config-file writes; see
    /// [`config::Writer`]. `persist_config` queues snapshots on it.
    config_writer: config::Writer,
    tab: Tab,
    scanners: Scanners,
    /// Cloned into every session so they can bind their MGBAStream
    /// without owning the audio backend. The CPAL Backend lives in
    /// `_audio_backend` so the underlying stream keeps playing.
    audio_binder: audio::LateBinder,
    /// Owns the CPAL stream and its recovery supervisor. The backend
    /// internally follows default-device/config changes, reconnects
    /// after stream failures, and retries while no device is present.
    _audio_backend: Option<audio::cpal::Backend>,

    /// Owned game+save+assets for the current selection. Rebuilt only
    /// when game or save changes; per-frame view() borrows it. This is
    /// also what booting and readying up run on — a netplay-only game
    /// loads through the shared empty editor like everyone else.
    loaded: Option<selection::LoadedSave>,

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
    patch_autoupdater: autoupdate::Autoupdater,
    /// A replay whose playback is waiting on a patch download. Set by
    /// `watch_replay`, resumed once the install rescan lands.
    pending_watch: Option<std::path::PathBuf>,
    /// Playback speed to apply to the next queued replay as it installs,
    /// carried off the session it replaces. Set only on a queue handoff —
    /// watching a replay from the tab still starts at realtime — and
    /// consumed by `watch_replay`, so it survives the deferred install when
    /// the next replay has to fetch a patch first.
    queue_carry_speed: Option<f32>,
    /// Whether the active replay was running (rather than paused) the last
    /// time the frame handler looked. The queue advances on the edge into
    /// end-of-stream, and only when playback ran into it: scrubbing pauses
    /// first, so dragging the playhead onto the final tick reads as parking
    /// there rather than as the replay having run out.
    replay_was_playing: bool,
    /// In-flight and failed patch downloads. App-level because the
    /// patches tab, the lobby, replay playback and the play tab's picker
    /// all start them, and every one of those surfaces renders them.
    downloads: patch::Downloads,
    /// Cancel handles for the in-flight ones, keyed the same way. The
    /// download loop checks its token once per chunk and tidies up its
    /// own partial file, so cancelling leaves nothing behind.
    download_cancels: std::collections::HashMap<patch::VersionKey, tokio_util::sync::CancellationToken>,
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
    /// False until the startup scan's first stage lands (roms, saves,
    /// patches — everything the play tab is built from). Deliberately
    /// not `rescans_in_flight > 0`: this one means "the library has
    /// never been read", which is what tells an empty scanner apart
    /// from an empty library. A later background rescan must not flip
    /// a settled empty state back to "scanning…".
    library_scanned: bool,
    /// Same, for the startup scan's second stage — the replay index,
    /// which only the replays tab waits on.
    replays_scanned: bool,
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
    /// Set while an application quit waits for PvP's bounded `Goodbye` send.
    /// Repeated close requests during that short window are ignored.
    exit_pending: bool,
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

fn copy_html_to_clipboard(text: String, html: String) {
    tokio::task::spawn_blocking(move || match arboard::Clipboard::new() {
        Ok(mut cb) => {
            if let Err(e) = cb.set_html(html.as_str(), Some(text.as_str())) {
                log::warn!("clipboard set_html failed: {e}");
                // Keep the established plain-text copy useful even on a
                // clipboard backend that cannot publish HTML.
                if let Err(e) = cb.set_text(text) {
                    log::warn!("clipboard set_text fallback failed: {e}");
                }
            }
        }
        Err(e) => log::warn!("clipboard open failed: {e}"),
    });
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
    /// Hand a played-out replay session over to the next queued replay.
    ///
    /// Called on every frame message, so it is mostly a pair of atomic loads.
    /// It fires only on the transition into end-of-stream while playback was
    /// actually running: `replay_was_playing` is what separates "ran out" from
    /// "the user dragged the playhead to the last tick", since scrubbing
    /// pauses before it seeks. With nothing queued the finished replay just
    /// stays parked on its final frame, as it always has.
    fn advance_replay_queue(&mut self) -> iced::Task<Message> {
        let Some(s) = self.session.active_as::<session::replay::ReplaySession>() else {
            self.replay_was_playing = false;
            return iced::Task::none();
        };
        let was_playing = std::mem::replace(&mut self.replay_was_playing, !s.is_paused());
        // A pending seek means the playhead is on its way somewhere else —
        // the tick it reads right now says nothing about the stream ending.
        let ran_out = was_playing && s.pending_seek_target().is_none() && s.current_tick() >= s.total_ticks();
        if !ran_out || self.replays.queue.is_empty() {
            return iced::Task::none();
        }
        let next = self.replays.queue.remove(0);
        // Whatever speed they were watching at carries to the next one —
        // someone skimming a queue at 4× means it for the queue, not for the
        // one replay.
        self.queue_carry_speed = Some(s.speed());
        // Close before building: dropping the old session by overwriting the
        // slot would leave its drive thread unjoined and its audio still
        // bound. `watch_replay` handles the rest, including parking on a
        // patch download if the next replay needs one it hasn't got.
        self.session.close_session();
        self.replay_was_playing = false;
        self.watch_replay(next)
    }

    /// Stats duty for a playback session about to start on `path`. With
    /// no readable stats sidecar, the session's prefetcher — which runs
    /// the very simulation the analysis needs anyway — takes the
    /// analysis over: any in-flight tab worker for this replay is
    /// cancelled (its simulation stops, its progress stream aborts
    /// mid-air so the tab's pending marker survives the handover), and
    /// the returned job + progress-stream task plug the prefetcher into
    /// the tab's usual `HpStatsPartial`/`HpStatsLoaded` pipeline. With a
    /// sidecar on disk there is nothing to compute — just the round
    /// boundaries out of it, so the scrub bar has its marks from the
    /// first frame rather than waiting out a pass to rediscover them.
    fn replay_stats_takeover(&mut self, path: &std::path::Path) -> ReplayStatsDuty {
        if let Some(stats) = replays::load_match_stats(&self.config.cache_path(), &self.config.replays_path(), path) {
            return ReplayStatsDuty {
                job: None,
                round_boundaries: stats.round_marks(),
                task: iced::Task::none(),
            };
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
        ReplayStatsDuty {
            job: Some(job),
            // Nothing analyzed yet, so nothing to hand the scrub bar;
            // the prefetcher publishes each boundary as it reaches it.
            round_boundaries: vec![],
            task: iced::Task::stream(stream).map(Message::Replays),
        }
    }
}

/// What [`App::replay_stats_takeover`] settled for a playback session:
/// whether its prefetcher owes anyone an analysis, what it can be told
/// up front about where the rounds are, and the task wiring its progress
/// back into the replays tab.
struct ReplayStatsDuty {
    job: Option<session::replay::PrefetchStatsJob>,
    round_boundaries: Vec<u32>,
    task: iced::Task<Message>,
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
        // The scan itself runs off-thread from the task below: reading
        // and parsing the library is unbounded work (a big replay or ROM
        // collection, a slow disk), and running it here would hold the
        // window closed for its whole duration — iced doesn't open one
        // until this returns. The selection restore that needs the
        // results moves with it, into `RescanFollowup::Boot`.
        let restored = loadout::Loadout {
            // Restore the selected family (drives the picker even when no
            // owned-ROM game resolves under it); falls back to the family of
            // `last_game` for configs written before `last_family` existed.
            // Static lookups, so this half needs no scan.
            family: config
                .last_family
                .as_deref()
                .and_then(game::family_static)
                .or_else(|| config.last_game.as_ref().and_then(|(f, _)| game::family_static(f))),
            ..Default::default()
        };
        let welcome = tabs::welcome::State::from_nickname(config.nickname.as_deref());

        // Spin up the CPAL audio backend once at startup with the
        // LateBinder as the source. Sessions later bind their
        // MGBAStream into the binder and the CPAL stream keeps going
        // across selections.
        let audio_binder = audio::LateBinder::new();
        audio_binder.set_volume(config.volume);
        let audio_backend = match audio::cpal::Backend::new(audio_binder.clone()) {
            Ok(b) => Some(b),
            Err(e) => {
                log::warn!("audio: cpal supervisor failed to start, running silent: {e:?}");
                None
            }
        };

        let mut patch_autoupdater = autoupdate::Autoupdater::new(
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

        let mut app = Self {
            config,
            config_writer: config::Writer::new(),
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
            replay_analysis_jobs: Default::default(),
            queue_carry_speed: None,
            replay_was_playing: false,
            patches: PatchesState::default(),
            session: session::State::new(),
            netplay: netplay::State::new(),
            discord: discord::Client::new(),
            session_started_at: None,
            patch_autoupdater,
            pending_watch: None,
            downloads: patch::Downloads::new(),
            download_cancels: Default::default(),
            updater,
            rescans_in_flight: 0,
            library_scanned: false,
            replays_scanned: false,
            // Start at rest (no launch animation) — progress 1.0
            // and not animating until first triggered.
            screen_enter: anim::Enter::default(),
            screen_enter_scope: EnterScope::Root { dy: ROOT_SLIDE },
            lobby_swap: anim::Transition::swap(false),
            lobby_exit_snapshot: None,
            exit_pending: false,
        };
        app.refresh_loaded();
        let scan = app.boot_scan();
        (app, scan)
    }

    /// The startup scan, in two stages: first everything the play tab
    /// is built from, then the replay index. They're separate because
    /// they're waited on separately — the play tab has no reason to sit
    /// behind a replay collection that can run to thousands of files,
    /// and the replays tab is the only screen that does.
    ///
    /// Each stage lands as its own `Rescanned` message, so the counter
    /// takes two.
    fn boot_scan(&mut self) -> iced::Task<Message> {
        self.rescans_in_flight += 2;
        let scanners = self.scanners.clone();
        let config = self.config.clone();
        // One enumeration feeds both stages: it covers all four roots
        // and costs metadata only, so the first stage hands the replays
        // listing to the second rather than walking that tree twice.
        iced::Task::perform(
            async move {
                let listings = Scanners::list(&config).await;
                let replays = listings.replays.clone();
                let _ = tokio::task::spawn_blocking(move || scanners.rescan_library(&config, &listings)).await;
                replays
            },
            |listing| listing,
        )
        .then({
            let scanners = self.scanners.clone();
            move |listing| {
                let scanners = scanners.clone();
                // The play tab is live from here; the replay index
                // lands whenever it lands.
                iced::Task::done(Message::Rescanned(RescanFollowup::Boot)).chain(iced::Task::perform(
                    async move {
                        let _ = tokio::task::spawn_blocking(move || scanners.rescan_replays(&listing)).await;
                    },
                    |()| Message::Rescanned(RescanFollowup::BootReplays),
                ))
            }
        })
    }

    /// Resolve the saved selection against the scanners, now that the
    /// startup scan has filled them. Only the family half is restorable
    /// without a scan (see [`App::new`]); the game, its save and that
    /// save's patch overlay each have to still exist to come back.
    fn restore_selection(&mut self) {
        let Some((family, variant)) = self.config.last_game.as_ref() else {
            return;
        };
        let Some(game) = crate::library::game::find_by_family_and_variant(family, *variant) else {
            return;
        };
        if !self.scanners.roms.read().contains_key(&game) {
            return;
        }
        self.loadout.game = Some(game);
        self.loadout.family = Some(game.family_and_variant().0);
        let Some(rel) = self.config.last_save_per_family.get(game.family_and_variant().0) else {
            return;
        };
        let abs = self.config.data_relative_to_absolute(rel);
        if !self
            .scanners
            .saves
            .read()
            .get(&game)
            .map(|v| v.iter().any(|s| s.path == abs))
            .unwrap_or(false)
        {
            return;
        }
        self.loadout.save = Some(abs);
        // The patch overlay hangs off the save — restore whatever this
        // save was last used with, if the patch still exists and
        // supports the variant.
        if let Some(Some((n, v))) = self.config.last_patch_per_save.get(rel) {
            if self.scanners.patches.read().supported_games(n, v).contains(&game) {
                self.loadout.patch = Some(n.clone());
                self.loadout.patch_version = Some(v.clone());
            }
        }
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
        // The round count isn't in the recording — only a telemetry
        // analysis knows it — so it comes from this replay's cached one
        // where there is one, and the caption goes without until then.
        let cache_path = self.config.cache_path();
        let replays_path = self.config.replays_path();
        let stream = futures::stream::iter(paths)
            .then(move |path| {
                let (cache_path, replays_path) = (cache_path.clone(), replays_path.clone());
                async move {
                    let p = path.clone();
                    let stats = tokio::task::spawn_blocking(move || {
                        let mut stats = replays::compute_stats(crate::library::storage(), &p).ok()?;
                        stats.round_count =
                            replays::load_match_stats(&cache_path, &replays_path, &p).map(|s| s.rounds.len() as u32);
                        Some(stats)
                    })
                    .await
                    .ok()
                    .flatten();
                    (path, stats)
                }
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
    /// remembered per family, and the patch overlay per save — so every
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
                self.config
                    .last_save_per_family
                    .insert(g.family_and_variant().0.to_string(), rel.clone());
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
    /// Default match-type policy:
    ///   - Family JUST changed (or first selection in this lobby):
    ///     the mode this family was last picked in
    ///     ([`Config::last_match_type_per_family`]), or, failing that,
    ///     Triple (mode=1) if the game supports it, else Single.
    ///     Keyed off `default_mt_for_family` so it only fires once per
    ///     (lobby, family) pair.
    ///   - Same family, current value invalid for this game: same
    ///     fallback (paranoia — the versions of a family can differ).
    ///   - Same family, valid value: leave alone — sticky user pick.
    ///
    /// Called any time the current game or lobby state could have
    /// changed in a way that affects the right default: on Connect
    /// (cancel_and_renew wiped the lobby), on selection change,
    /// and defensively inside `resend_settings_if_lobby`.
    fn apply_default_match_type(&mut self) {
        let Some(game) = self.loadout.game else { return };
        let mt_table = game::from_gamedb_entry(game)
            .map(|g| g.family.match_types)
            .unwrap_or(&[]);
        let family = game.family_and_variant().0;
        let family_changed = self.netplay.lobby.default_mt_for_family.as_deref() != Some(family);
        let (mode, sub) = self.netplay.lobby.match_type;
        let current_valid =
            (mode as usize) < mt_table.len() && (sub as usize) < *mt_table.get(mode as usize).unwrap_or(&0);
        if family_changed || !current_valid {
            // What this family was last played in, if the game still
            // offers it — a remembered pick outranks the built-in
            // default, and a stale one (the table shrank under a patch)
            // falls through to it.
            let remembered = self
                .config
                .last_match_type_per_family
                .get(family)
                .copied()
                .filter(|&(mode, sub)| {
                    (mode as usize) < mt_table.len() && (sub as usize) < *mt_table.get(mode as usize).unwrap_or(&0)
                });
            let new_mt = remembered.unwrap_or_else(|| {
                if mt_table.get(1).copied().unwrap_or(0) > 0 {
                    (1, 0) // Triple
                } else {
                    (0, 0) // Single
                }
            });
            self.netplay.lobby.match_type = new_mt;
            self.netplay.lobby.default_mt_for_family = Some(family.to_string());
        }
    }

    /// Both sides have exchanged StartMatch: drain the lobby-side state
    /// into a `PreMatchData` and kick off async PvP setup. The lobby pump
    /// has been cancel-signaled; `spawn_pvp` polls the receiver-handoff
    /// slot until it releases ownership. On success we land back in
    /// `Message::PvpSessionBuilt`.
    fn start_pvp_handoff(&mut self) -> iced::Task<Message> {
        let Some(pre_match) = self.netplay.take_pre_match() else {
            return iced::Task::none();
        };
        // Stamp the attempt this build belongs to. Leaving the lobby
        // mid-build (always allowed) bumps the id via
        // `netplay.disconnect()`, so the PvpSessionBuilt handler can
        // tell an abandoned build from a live one.
        let attempt = self.netplay.session_id();
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
            move |result| Message::PvpSessionBuilt(attempt, std::sync::Arc::new(std::sync::Mutex::new(Some(result)))),
        )
    }

    /// Build the current Settings packet and push it to the peer — only
    /// meaningful while netplay is in Lobby phase; outside that this
    /// returns `Task::none()`. Wrapped in a helper because it has three
    /// callers: lobby entry, selection change, and match-type change.
    fn resend_settings_if_lobby(&mut self) -> iced::Task<Message> {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) {
            return iced::Task::none();
        }
        self.apply_default_match_type();
        let settings = self.make_local_settings();
        self.netplay.send_local_settings(settings);
        iced::Task::none()
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
                // Enumerate here (cheap, metadata only), then read and
                // parse on a blocking worker so the walk-and-parse of
                // every ROM and save doesn't stall iced's update loop.
                let listings = Scanners::list(&config).await;
                let _ = tokio::task::spawn_blocking(move || scanners.rescan(&config, &listings)).await;
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
    fn uncommit_if_incompat(&mut self) {
        if !matches!(self.netplay.phase, netplay::Phase::Lobby { .. }) || !self.netplay.local_ready() {
            return;
        }
        // Scoped so the scanner read guards (and the borrows of
        // `netplay.lobby`) are released before the uncommit.
        let compatible = {
            let (Some(local), Some(remote)) = (self.netplay.lobby.local.as_ref(), self.netplay.lobby.remote.as_ref())
            else {
                return;
            };
            let roms = self.scanners.roms.read();
            let patches = self.scanners.patches.read();
            matches!(
                netplay::compat::check(local, remote, &roms, &patches),
                netplay::compat::Verdict::Compatible
            )
        };
        if !compatible {
            self.netplay.uncommit();
        }
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
        // The view state rides inside the LoadedSave, so swapping in a
        // freshly-built one drops any in-progress edit with the save it
        // was staged against — nothing to reset by hand.
        // A disk load carries no session payload — the editor opens on
        // the game's own default and the file picker takes it from
        // there.
        self.loaded = Some(selection::build(game, rom, save_path, save, &patches_path, patch_meta));
    }
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
}

#[cfg(test)]
mod tests {
    use super::{RescanFollowup, Tab};

    #[test]
    fn clicking_play_forces_the_loaded_selection_to_be_rebuilt() {
        assert_eq!(Tab::Play.rescan_followup(), Some(RescanFollowup::ForceRebuildLoaded));
        assert_eq!(Tab::Patches.rescan_followup(), Some(RescanFollowup::Refresh));
        assert_eq!(Tab::Settings.rescan_followup(), None);
    }
}
