//! Desktop application state, initialization, and subscriptions.
//!
//! [`message`] defines the event vocabulary and navigation destinations.
//! [`dispatch`] routes events and coordinates screen transitions;
//! feature modules own their actions and effects; [`view`] renders the shell.
//! The host wires these methods into `iced::application` in `main.rs`.

use crate::library::{autoupdate, Catalog};
use crate::platform::audio;
use crate::ui::anim;
use crate::{config, discord, i18n, netplay, selection, session, tabs, updater, INIT_LINK_CODE};
use i18n::t;
use tabs::patches::PatchesState;
use tabs::replays::ReplaysState;

mod desktop;
mod dispatch;
mod downloads;
mod library;
mod lobby;
mod message;
mod patches;
mod play;
mod replay;
mod replay_controller;
mod sessions;
mod settings;
mod view;

pub use message::{Message, RescanFollowup, Tab};

pub struct App {
    config: config::Config,
    /// Background thread that owns the actual config-file writes; see
    /// [`config::Writer`]. `persist_config` queues snapshots on it.
    config_writer: config::Writer,
    tab: Tab,
    scanners: Catalog,
    /// Used to bind each installed session's audio stream
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
    loadout: tango_library::loadout::Selection,
    play: tabs::play::State,
    replays: ReplaysState,
    replay_controller: replay_controller::Controller,
    patches: PatchesState,
    settings: tabs::settings::State,
    welcome: tabs::welcome::State,
    netplay: netplay::State,

    /// Owns the active runtime and its presentation, including post-match results.
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
    downloads: downloads::Coordinator,
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

impl App {
    pub fn new() -> (Self, iced::Task<Message>) {
        let config = config::Config::load_or_create();

        let scanners = Catalog::new();
        // The scan itself runs off-thread from the task below: reading
        // and parsing the library is unbounded work (a big replay or ROM
        // collection, a slow disk), and running it here would hold the
        // window closed for its whole duration — iced doesn't open one
        // until this returns. The selection restore that needs the
        // results moves with it, into `RescanFollowup::Boot`.
        // Only the family half is restorable before the scan (the
        // catalog is still empty); `RescanFollowup::Boot` restores the
        // rest once it lands.
        let mut restored = tango_library::loadout::Selection::default();
        restored.restore(&config, &scanners);
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
            patches: PatchesState::default(),
            session: session::State::new(),
            netplay: netplay::State::new(),
            discord: discord::Client::new(),
            session_started_at: None,
            patch_autoupdater,
            downloads: downloads::Coordinator::default(),
            replay_controller: replay_controller::Controller::default(),
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

    /// Queue `self.config` for persistence on the background writer —
    /// the render thread never blocks on the disk. Write failures are
    /// logged by the writer thread.
    fn persist_config(&self) {
        self.config_writer.write(self.config.clone());
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
