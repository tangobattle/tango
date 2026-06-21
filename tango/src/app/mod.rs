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
    anim, audio, config, discord, game, i18n, identity, input, loadout, lobby, net, netplay, patch, replays, rom, save,
    selection, session, tabs, updater, widgets,
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

/// Per-tab `update_*` message handlers (the bulk of the update logic),
/// split out of this file to keep `App` from being one giant module.
mod update;

/// Push an RGBA image to the OS clipboard. iced's clipboard API
/// only handles text, so we drop down to `arboard` on a tokio
/// background task — both because it can block briefly and
/// because arboard's Clipboard handle isn't Send-safe to keep on
/// the UI thread.
/// Build a `net::protocol::Settings` from a lobby `MatchProposal`, to seed the
/// PvP session (a lobby match does no p2p Settings exchange).
fn settings_from_proposal(p: &tango_lobby::MatchProposal, nickname: String) -> crate::net::protocol::Settings {
    let game_info = p.game_info.as_ref().map(|g| crate::net::protocol::GameInfo {
        family_and_variant: (g.family.clone(), g.variant as u8),
        patch: g.patch.as_ref().and_then(|pt| {
            semver::Version::parse(&pt.version)
                .ok()
                .map(|version| crate::net::protocol::PatchInfo {
                    name: pt.name.clone(),
                    version,
                })
        }),
    });
    crate::net::protocol::Settings {
        nickname,
        match_type: p
            .match_type
            .as_ref()
            .map(|m| (m.mode as u8, m.subtype as u8))
            .unwrap_or((0, 0)),
        game_info,
        blind_setup: p.blind_setup,
    }
}

/// Format lobby `IceServer`s into the inline-credential URL strings
/// libdatachannel expects (TURN-over-TCP is dropped — libdatachannel rejects it).
fn ice_to_strings(servers: &[tango_lobby::IceServer], use_relay: Option<bool>) -> Vec<String> {
    servers
        .iter()
        .flat_map(|s| {
            let username = s.username.clone();
            let credential = s.credential.clone();
            s.urls
                .iter()
                .filter_map(move |url| {
                    let colon = url.find(':')?;
                    let (proto, rest) = (&url[..colon], &url[colon + 1..]);
                    // libdatachannel rejects TURN-over-TCP; "Never relay" drops
                    // the TURN servers entirely.
                    if url.ends_with("?transport=tcp") {
                        return None;
                    }
                    if use_relay == Some(false) && (proto == "turn" || proto == "turns") {
                        return None;
                    }
                    Some(match (&username, &credential) {
                        (Some(u), Some(c)) => format!("{proto}:{u}:{c}@{rest}"),
                        _ => format!("{proto}:{rest}"),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Normalize a typed direct-connect target into a `host:port`. Mirrors the
/// `/connect` parser's heuristic: append the default port unless the input
/// already carries one (a colon that isn't just an IPv6 group, i.e. not ending
/// in `]`). Returns empty for blank input so the caller can bail.
fn direct_dial_addr(input: &str) -> String {
    let arg = input.trim();
    if arg.is_empty() {
        return String::new();
    }
    if arg.contains(':') && !arg.ends_with(']') {
        arg.to_string()
    } else {
        format!("{arg}:{}", crate::net::DEFAULT_LOCAL_PORT)
    }
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

    /// Presence connection to the lobby server (roster + challenges). Runs
    /// alongside the existing link-code netplay path; a failure to reach it is
    /// non-fatal.
    lobby: lobby::State,

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
            if let Some(game) = crate::game::find_by_family_and_variant(family, *variant) {
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

        let play = tabs::play::State::default();

        // Persistent self-signed identity, loaded once and handed to the lobby
        // client to present as its mTLS client certificate.
        let lobby = lobby::State::new(config.lobby_endpoint.clone(), identity::load(), config.lobby_status);

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
            lobby,
            discord: discord::Client::new(),
            session_started_at: None,
            patch_autoupdater,
            updater,
            rescans_in_flight: 0,
            // Start at rest (no launch animation) — progress 1.0
            // and not animating until first triggered.
            screen_enter: anim::Enter::default(),
            screen_enter_scope: EnterScope::Root { dy: ROOT_SLIDE },
        };
        app.refresh_loaded();
        // Seed the proposal's default match type for the restored game (the
        // SelectionChanged path that normally drives this doesn't fire for the
        // startup restore), so a fresh launch defaults to Triple where supported.
        app.apply_default_match_type();
        let stats_task = app.kick_replay_stats_loader().map(Message::Replays);
        let lobby_task = app.lobby.connect().map(Message::Lobby);
        (app, iced::Task::batch([stats_task, lobby_task]))
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

    /// Route a lobby message: the App handles the actions that need its loadout
    /// + loaded save (issue/accept a challenge), the rest go to the lobby state.
    fn handle_lobby_message(&mut self, message: lobby::Message) -> iced::Task<Message> {
        match message {
            lobby::Message::IssueChallenge(peer) => self.issue_challenge(peer),
            lobby::Message::AcceptIncoming(peer) => self.accept_incoming(peer),
            lobby::Message::Event(event) => self.handle_lobby_event(event),
            // Local friend nicknames live in config, not the lobby state — a
            // non-empty name makes them a friend; empty clears it.
            lobby::Message::SetNickname { code, name } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    self.config.friends.remove(&code);
                } else {
                    self.config.friends.insert(code, name);
                }
                self.persist_config();
                iced::Task::none()
            }
            // Remember the user's presence so reopening Tango comes back the
            // same way; the lobby state derives the connect/disconnect from it.
            lobby::Message::SetSelfStatus(status) => {
                self.config.lobby_status = status;
                self.persist_config();
                self.lobby.set_self_status(status).map(Message::Lobby)
            }
            // The match settings feed `current_proposal`. Stash them on the
            // netplay lobby state the proposal builder reads, then re-send any
            // outstanding challenge so the peer sees the updated terms.
            lobby::Message::SetMatchType(mt) => {
                self.netplay.lobby.match_type = mt;
                self.refresh_outgoing_challenge()
            }
            lobby::Message::SetBlindSetup(v) => {
                self.netplay.lobby.blind_setup = v;
                // Remember it so the next challenge defaults to the same choice.
                self.config.last_blind_setup = v;
                self.persist_config();
                self.refresh_outgoing_challenge()
            }
            lobby::Message::CopyText { text, flash } => {
                crate::copy_feedback::flash(flash);
                iced::clipboard::write(text)
            }
            // Direct-connect (signaling-free): host on the default port, or dial
            // the typed address. Both reuse the challenge path's local
            // settings + reveal, then hand off to netplay's ConnectDirect.
            lobby::Message::DirectHost => {
                self.start_direct(netplay::DirectRole::Host {
                    port: crate::net::DEFAULT_LOCAL_PORT,
                })
            }
            lobby::Message::DirectJoin => {
                let addr = direct_dial_addr(&self.lobby.direct_addr);
                if addr.is_empty() {
                    return iced::Task::none();
                }
                self.start_direct(netplay::DirectRole::Connect { addr })
            }
            lobby::Message::CancelDirect => {
                // Abort the in-flight bring-up; the direct view drops back to its
                // host/join form (netplay returns to idle).
                self.netplay.cancel();
                self.lobby.direct_error = None;
                iced::Task::none()
            }
            other => self.lobby.update(other).map(Message::Lobby),
        }
    }

    /// Bring up a signaling-free direct link in the given role. Mirrors
    /// `issue_challenge`'s preconditions (a game + save must be loaded so we can
    /// build local settings + a reveal), leaves the direct-connect view, and
    /// dispatches netplay's `ConnectDirect` — the rest of the flow (handshake,
    /// match handoff, PvP spawn) is identical to the lobby path.
    fn start_direct(&mut self, role: netplay::DirectRole) -> iced::Task<Message> {
        let Some(reveal) = self.local_reveal() else {
            log::warn!("direct connect: need a game + save loaded");
            return iced::Task::none();
        };
        let local_settings = self.make_local_settings();
        let match_type = self.netplay.lobby.match_type;
        self.lobby.menu_open = false;
        self.lobby.direct_error = None;
        // Keep the direct-connect view open — while netplay is connecting it
        // renders as the "waiting for a peer" screen (no full-screen takeover
        // until the match actually starts; see `netplay_takes_over_screen`).
        // Compatibility is gated on the resulting PreMatchData at handoff.
        self.netplay
            .connect_direct(role, local_settings, reveal.compressed, match_type)
            .map(Message::Netplay)
    }

    /// Incoming challengers whose proposed setup is netplay-incompatible with
    /// our current loadout — accepting is blocked for these (the sidebar shows
    /// the mismatch). Computed per-frame for the sidebar.
    fn incompatible_challengers(&self) -> std::collections::BTreeSet<tango_lobby::FriendCode> {
        if !self.lobby.has_incoming() {
            return std::collections::BTreeSet::new();
        }
        let local = self.make_local_settings();
        let patches = self.scanners.patches.read();
        self.lobby
            .incoming_iter()
            .filter_map(|(fc, inc)| {
                let remote = settings_from_proposal(&inc.proposal, String::new());
                // Accepting adopts the challenger's match type, so only a
                // game/patch mismatch should block accept — line the match types
                // up before the check so a mere match-type difference doesn't.
                let mut local = local.clone();
                local.match_type = remote.match_type;
                match netplay::compat::check(&local, &remote, &*patches) {
                    netplay::compat::Verdict::Compatible => None,
                    _ => Some(*fc),
                }
            })
            .collect()
    }

    /// Drive the lobby's match-relevant events: relay SDP into an in-flight
    /// bring-up, and on accept/confirm kick off the WebRTC match (which goes
    /// straight to PvP, no netplay lobby screen).
    fn handle_lobby_event(&mut self, event: tango_lobby::Event) -> iced::Task<Message> {
        use tango_lobby::Event;
        if let Event::RtcOffer { sdp, .. } | Event::RtcAnswer { sdp, .. } = &event {
            self.netplay.feed_lobby_sdp(sdp.clone());
        }
        let start = matches!(
            event,
            Event::ChallengeAccepted { .. } | Event::ChallengeConfirmed { .. }
        );
        // A fresh incoming challenge: flash + bounce the taskbar so you notice
        // even when Tango isn't focused (no-op when already focused, per iced).
        let incoming = matches!(event, Event::ChallengeIncoming { .. });
        // Bookkeeping first, so my_match gets the peer commitment + ICE servers.
        let lobby_task = self.lobby.update(lobby::Message::Event(event)).map(Message::Lobby);
        let net_task = if start {
            self.start_lobby_match()
        } else {
            iced::Task::none()
        };
        let attention = if incoming {
            iced::window::latest()
                .and_then(|id| iced::window::request_user_attention(id, Some(iced::window::UserAttention::Critical)))
        } else {
            iced::Task::none()
        };
        iced::Task::batch([lobby_task, net_task, attention])
    }

    /// Pull the ready match out of the lobby state and bring up the WebRTC match.
    fn start_lobby_match(&mut self) -> iced::Task<Message> {
        let Some(start) = self.lobby.take_match_start() else {
            return iced::Task::none();
        };
        let Some(handle) = self.lobby.handle() else {
            return iced::Task::none();
        };
        let role = if start.is_offerer {
            crate::net::lobby_rtc::LobbyRole::Offerer
        } else {
            crate::net::lobby_rtc::LobbyRole::Answerer
        };
        let use_relay = self.config.relay_mode.use_relay();
        let ice_servers = ice_to_strings(&start.ice_servers, use_relay);
        let match_type = start
            .local_proposal
            .match_type
            .as_ref()
            .map(|m| (m.mode as u8, m.subtype as u8))
            .unwrap_or((0, 0));
        let local_settings =
            settings_from_proposal(&start.local_proposal, self.config.nickname.clone().unwrap_or_default());
        let remote_settings = settings_from_proposal(&start.peer_proposal, String::new());
        self.netplay
            .connect_lobby_match(
                role,
                ice_servers,
                use_relay,
                handle,
                start.peer,
                start.local_compressed,
                start.peer_commitment,
                local_settings,
                remote_settings,
                match_type,
            )
            .map(Message::Netplay)
    }

    /// Whether we can issue / accept a challenge right now (a game + save loaded).
    fn can_challenge(&self) -> bool {
        self.loadout.game.is_some() && self.loaded.is_some()
    }

    /// The match proposal for our current loadout (with our picked match type),
    /// or `None` if no game is picked.
    fn current_proposal(&self) -> Option<tango_lobby::MatchProposal> {
        self.proposal_with_match_type(self.netplay.lobby.match_type)
    }

    /// Build a match proposal for the current loadout with an explicit match
    /// type. Accepting an incoming challenge reuses this with the *challenger's*
    /// match type (you accept on their terms), so a match-type difference never
    /// has to block accepting — we just adopt theirs.
    fn proposal_with_match_type(&self, match_type: (u8, u8)) -> Option<tango_lobby::MatchProposal> {
        let game = self.loadout.game?;
        let (family, variant) = game.family_and_variant();
        let patch = match (&self.loadout.patch, &self.loadout.patch_version) {
            (Some(name), Some(version)) => Some(tango_lobby::proto::lobby::game_info::Patch {
                name: name.clone(),
                version: version.to_string(),
            }),
            _ => None,
        };
        Some(tango_lobby::MatchProposal {
            match_type: Some(tango_lobby::MatchType {
                mode: match_type.0 as u32,
                subtype: match_type.1 as u32,
            }),
            game_info: Some(tango_lobby::GameInfo {
                family: family.to_string(),
                variant: variant as u32,
                patch,
            }),
            blind_setup: self.netplay.lobby.blind_setup,
        })
    }

    /// Build a commitment + reveal from the loaded save's SRAM.
    fn local_reveal(&self) -> Option<crate::net::protocol::LocalReveal> {
        let loaded = self.loaded.as_ref()?;
        let sram = loaded.save.to_sram_dump();
        match crate::net::protocol::build_commitment(sram) {
            Ok(reveal) => Some(reveal),
            Err(e) => {
                log::warn!("lobby challenge: build commitment failed: {e:#}");
                None
            }
        }
    }

    fn issue_challenge(&mut self, peer: tango_lobby::FriendCode) -> iced::Task<Message> {
        let (Some(proposal), Some(reveal)) = (self.current_proposal(), self.local_reveal()) else {
            log::warn!("lobby challenge: need a game + save loaded");
            return iced::Task::none();
        };
        self.lobby.start_outgoing(peer, proposal, reveal);
        iced::Task::none()
    }

    /// If we have an outstanding outgoing challenge, re-send it with the
    /// current loadout + match settings so the peer always sees what we'd
    /// actually play. A no-op when nothing is pending. Called when the
    /// selection or proposed match type / blind setup changes.
    fn refresh_outgoing_challenge(&mut self) -> iced::Task<Message> {
        match self.lobby.outgoing_peer() {
            Some(peer) => self.issue_challenge(peer),
            None => iced::Task::none(),
        }
    }

    fn accept_incoming(&mut self, peer: tango_lobby::FriendCode) -> iced::Task<Message> {
        let Some(incoming) = self.lobby.incoming_get(&peer).cloned() else {
            return iced::Task::none();
        };
        // Accept on their terms: adopt the challenger's match type so the two
        // sides always agree, rather than forcing the user to match it by hand.
        let their_match_type = incoming
            .proposal
            .match_type
            .as_ref()
            .map(|m| (m.mode as u8, m.subtype as u8))
            .unwrap_or(self.netplay.lobby.match_type);
        let (Some(proposal), Some(reveal)) = (self.proposal_with_match_type(their_match_type), self.local_reveal())
        else {
            log::warn!("lobby accept: need a game + save loaded");
            return iced::Task::none();
        };
        self.lobby.accept_incoming(peer, incoming, proposal, reveal);
        iced::Task::none()
    }

    /// Tear down and re-dial the presence connection against the current
    /// `config.lobby_endpoint` — called when the endpoint changes in Settings.
    /// A fresh `lobby::State` resets the roster + transient view state, which a
    /// reconnect would clear regardless.
    fn restart_lobby(&mut self) -> iced::Task<Message> {
        self.lobby = lobby::State::new(self.config.lobby_endpoint.clone(), identity::load(), self.config.lobby_status);
        self.lobby.connect().map(Message::Lobby)
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

    /// Default the sidebar proposal's match type when the selected game
    /// changes:
    ///   - Game just changed: pick Triple (mode=1) if the game supports it,
    ///     else Single — keyed off `default_mt_for_game` so it fires once per
    ///     game and a user's explicit pick for the same game sticks.
    ///   - Same game, current value invalid for it: same fallback (paranoia).
    ///   - Same game, valid value: leave alone.
    ///
    /// Called on selection change; `current_proposal` reads the result.
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


    /// Build a `protocol::Settings` packet from the App's current
    /// state: nickname from config, match_type defaults to (0, 0),
    /// game_info from the local loadout, and the available_games /
    /// available_patches lists from the scanners so the peer can see
    /// what we have locally.
    fn make_local_settings(&self) -> net::protocol::Settings {
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
    Lobby(lobby::Message),
    /// Carries the freshly-constructed PvP session back into the
    /// App after the async build task in `spawn_pvp` resolves.
    /// `Slot` because PvpSession isn't Clone.
    PvpSessionBuilt(netplay::Slot<anyhow::Result<session::pvp::PvpSession>>),
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
        } else if self.netplay_takes_over_screen() {
            // The session screen comes up the moment a (lobby) match starts
            // coming up — its backdrop renders first and the live session fills
            // in once `spawn_pvp` lands, so it's one screen, not a separate
            // connecting one. A direct link instead waits on the Play tab.
            ScreenKey::Session
        } else {
            ScreenKey::Tabs(self.tab)
        }
    }

    /// Whether the full-screen session view should take over the window. The
    /// lobby path merges connecting into the session screen (a peer who already
    /// accepted connects in a beat), but a *direct* link can wait indefinitely
    /// for someone to dial in — so it stays on the Play tab and waits in the
    /// sidebar's direct-connect screen until the match actually hands off.
    fn netplay_takes_over_screen(&self) -> bool {
        if self.session.is_active() || self.netplay.handoff_pending() {
            return true;
        }
        matches!(
            self.netplay.phase,
            netplay::Phase::Connecting {
                ident: netplay::LinkIdent::Lobby,
                ..
            }
        )
    }

    /// Whether a netplay attempt is in flight (connecting, failed-but-not-
    /// dismissed, or handing off) — drives the nav badge on the Play tab so an
    /// in-progress match is visible from other tabs.
    fn netplay_active(&self) -> bool {
        matches!(
            self.netplay.phase,
            netplay::Phase::Connecting { .. } | netplay::Phase::Failed { .. }
        ) || self.netplay.handoff_pending()
    }

    /// Whether a match is tying us up: a live PvP session, or a bring-up still
    /// in flight. The true→false edge (match ended / failed / cancelled, by any
    /// path) is where [`update`] reports us idle to the lobby.
    fn match_occupied(&self) -> bool {
        matches!(self.session.active, Some(ActiveSession::PvP(_)))
            || matches!(self.netplay.phase, netplay::Phase::Connecting { .. })
    }

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        let screen_before = self.screen_key();
        let family_before = self.loadout.family;
        let selection_before = (self.loadout.game, self.loadout.save.clone());
        let occupied_before = self.match_occupied();
        let task = self.update_inner(message);
        // Whenever a match stops occupying us — clean close, failed bring-up, or
        // a cancelled connect, by whatever path — tell the lobby we're idle once
        // here, so the roster (and our own busy dot) always clears rather than
        // only on the clean-close path.
        if occupied_before && !self.match_occupied() {
            self.lobby.report_idle();
        }
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
            // Loadout strip interactions route to the shared App-level Loadout
            // — the tab never sees them.
            Message::Play(tabs::play::Message::Loadout(m)) => self.update_loadout(m),
            Message::Play(m) => self.update_play(m),
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
            Message::Settings(m) => {
                // The endpoint field commits on Enter — re-dial the presence
                // connection against the (already-persisted) new endpoint.
                let reconnect = matches!(m, tabs::settings::Message::LobbyEndpointSubmitted);
                let settings_task = self.update_settings(m).map(Message::Settings);
                if reconnect {
                    iced::Task::batch([settings_task, self.restart_lobby()])
                } else {
                    settings_task
                }
            }
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
                // (Clearing the lobby "now playing" on match end is handled
                // centrally in `update` via the match-occupied transition.)
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
                // A direct link had no lobby to gate compatibility up front, so
                // check it here on the peer's settings before committing to the
                // spawn. Both sides run the same check, so both bail symmetrically
                // on a mismatch; the direct view shows the localized reason.
                let direct = matches!(
                    self.netplay.phase,
                    netplay::Phase::Connecting {
                        ident: netplay::LinkIdent::Direct(..),
                        ..
                    }
                );
                let Some(pre_match) = self.netplay.take_pre_match() else {
                    return iced::Task::none();
                };
                if direct {
                    let verdict = {
                        let patches = self.scanners.patches.read();
                        netplay::compat::check(&pre_match.local_settings, &pre_match.remote_settings, &patches)
                    };
                    if verdict != netplay::compat::Verdict::Compatible {
                        self.netplay.cancel();
                        self.lobby.direct_error = Some(verdict);
                        return iced::Task::none();
                    }
                }
                // The peer connected and the match is starting — the direct
                // connect screen has done its job; the session takes over now.
                self.lobby.direct_connect = false;
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
            Message::Lobby(m) => self.handle_lobby_message(m),
            Message::Netplay(m) => self.netplay.update(m).map(Message::Netplay),
            Message::PvpSessionBuilt(slot) => {
                let Some(result) = slot.lock().unwrap().take() else {
                    return iced::Task::none();
                };
                match result {
                    Ok(session) => {
                        self.session.active = Some(ActiveSession::PvP(session));
                        // A direct match isn't brokered by the lobby, so the
                        // server can't auto-derive our presence — report it now
                        // that the match is truly live (lobby matches are already
                        // marked server-side on accept). `report_idle` clears it
                        // on match end. `phase` is still Connecting until the
                        // `finish_handoff` below, so the ident is still readable.
                        let direct = matches!(
                            self.netplay.phase,
                            netplay::Phase::Connecting {
                                ident: netplay::LinkIdent::Direct(..),
                                ..
                            }
                        );
                        if direct {
                            if let Some(proposal) = self.current_proposal() {
                                self.lobby.report_busy(proposal);
                            }
                        }
                        // Both setup drawers start closed — the edge
                        // handles are the invitation; a pane that
                        // barges in over the match start isn't.
                        self.session.opponent_panel.close();
                        self.session.self_panel.close();
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
            lobby::subscription(&self.lobby).map(Message::Lobby),
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
            && matches!(self.netplay.phase, netplay::Phase::Connecting { .. });
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
            netplay::Phase::Connecting { ident, .. } => discord::make_looking_activity(ident, lang, game_info),
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
            return entered(
                iced::widget::stack![widgets::cyber_backdrop(), welcome]
                    .width(Fill)
                    .height(Fill)
                    .into(),
                enter,
                ROOT_SLIDE,
            );
        }

        // The session screen also covers the match bring-up: while a match is
        // coming up there's no `active` session yet, so the session view renders
        // just its backdrop + a "setting up" line, and the live session (with
        // emulation) fills in the moment `spawn_pvp` lands.
        if self.netplay_takes_over_screen() {
            // Deliver keyboard + gamepad input through the
            // synchronous widget path so each event reaches
            // `program.update()` on the same winit iteration it
            // arrived in. Going through subscriptions would
            // round-trip through an `mpsc::try_send` and cost ~1
            // winit iteration of input lag per event.
            let session_view = session::view::view(
                lang,
                &self.session,
                self.config.fractional_scaling,
                self.config.hide_emulator_border,
                crate::video::effects::effect_for(&self.config.video_filter),
                self.loadout.game.and_then(game::from_gamedb_entry),
            )
            .map(Message::Session);
            // In-session settings modal: floats centered over the
            // running session with a dimmed click-to-dismiss
            // backdrop. The emulator keeps running underneath.
            // Rendered while the open/close transition is in
            // flight too, so the panel eases in and out.
            let composed: Element<'_, Message> = if self.session.settings.visible(now) {
                let progress = self.session.settings.progress(now);
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
                if self.session.settings.shown() {
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
                        &self.netplay.phase,
                        self.netplay.handoff_pending(),
                        rescanning,
                    )
                    .map(Message::Play);
                // The presence roster rides on the right as a full-height pane,
                // composed here so the Play tab proper stays unaware of it.
                let incompatible = self.incompatible_challengers();
                let ctx = lobby::view::Ctx {
                    lang,
                    state: &self.lobby,
                    friends: &self.config.friends,
                    streamer_mode: self.config.streamer_mode,
                    can_challenge: self.can_challenge(),
                    netplay_idle: matches!(self.netplay.phase, netplay::Phase::Idle),
                    direct_connecting: match self.netplay.phase {
                        netplay::Phase::Connecting {
                            ident: netplay::LinkIdent::Direct(..),
                            waiting_for_opponent,
                        } => Some(waiting_for_opponent),
                        _ => None,
                    },
                    local_game: self.loadout.game,
                    match_type: self.netplay.lobby.match_type,
                    blind_setup: self.netplay.lobby.blind_setup,
                };
                row![
                    container(main).width(Fill).height(Fill),
                    lobby::view::sidebar(&ctx, &incompatible).map(Message::Lobby),
                ]
                .into()
            }
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
        let lobby_badge = self.netplay_active() && self.tab != Tab::Play;
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
