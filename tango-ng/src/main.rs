//! tango-ng: the next-generation Tango frontend, built on Slint so the
//! same UI can target desktop and mobile. It reuses the workspace's
//! UI-agnostic backend crates (mgba, tango-gamesupport, tango-dataview);
//! modules copied from the `tango` crate say so in their headers.
//!
//! Verification modes (instead of the interactive UI):
//! - `tango-ng --smoke [out.png]`: headless — scan, boot the first game
//!   with a save (against a temp copy of the save), emulate ~5 real
//!   seconds with audio, dump the framebuffer.
//! - `tango-ng --smoke-pvp host|connect [out.png]`: headless two-instance
//!   PvP — run both roles concurrently against the same data dir; each
//!   side drives the direct /host·/connect path + the auto-lobby to a
//!   live match, emulates ~8 seconds, dumps the framebuffer.
//! - `tango-ng --smoke-pvp match <link_code> [out.png]`: same, but both
//!   instances rendezvous on the real matchmaking server with the given
//!   link code (set `TANGO_IDENTITY_DIR` per instance for distinct certs).
//! - `tango-ng --ui-shot [out_dir]`: open the real UI, wait for the scan,
//!   then snapshot the main screens (including the netplay lobby band)
//!   to PNGs and exit. Run with `SLINT_BACKEND=winit-software` for
//!   reliable snapshots.

mod audio;
mod bnlc;
mod config;
mod game;
mod i18n;
mod identity;
mod input;
mod loaded;
// The net + netplay layers are ported; the direct /host + /connect
// slice (Phase A) and matchmaking + the lobby band (Phase B) are live.
// dead_code stays allowed for the surface tango-ng doesn't consume yet
// (Discord join secrets, parts of the reconnect plumbing).
#[allow(dead_code, unused_imports)]
mod net;
#[allow(dead_code)]
mod netplay;
mod patch;
// PvP session (Phase A port). Parts of its surface (frame-delay
// slider, reconnect overlay, median latency) wait on the Phase B
// lobby/session UI.
#[allow(dead_code)]
mod pvp;
mod replays;
mod rom;
mod save;
mod session;

slint::include_modules!();

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

enum Event {
    ScanDone {
        roms: HashMap<rom::GameRef, Vec<u8>>,
        saves: HashMap<rom::GameRef, Vec<save::ScannedSave>>,
        replays: Vec<replays::ScannedReplay>,
        patches: patch::PatchMap,
    },
    ReplayStats {
        path: std::path::PathBuf,
        stats: replays::ReplayStats,
    },
    /// Netplay task results + lobby-loop observations, forwarded into
    /// `netplay::State::update` by the timer below (which intercepts
    /// `MatchHandoffReady` itself, mirroring tango's App).
    Netplay(netplay::Message),
    /// The async PvP build kicked off by `MatchHandoffReady` resolved
    /// (tango's `Message::PvpSessionBuilt`). Ok installs the session;
    /// Err surfaces in the status line. Either way the netplay lobby
    /// snapshot is cleared via `finish_handoff`.
    PvpBuilt(Box<anyhow::Result<pvp::PvpSession>>),
}

/// Push every localized UI string into the Slint `I18n` global (plus
/// the app-version line), from tango's Fluent bundles. Called once at
/// startup and again on every language change; the data-path handler
/// also re-calls it so the roms-path line under the no-ROMs empty
/// state stays current. House rule: every t! call takes a literal key.
///
/// Strings the .slint marks "no tango key" intentionally have no
/// counterpart here and stay English (tango-ng-only placeholders like
/// "No replays found").
fn apply_i18n(app: &AppWindow, config: &config::Config) {
    let lang = &config.language;
    let i18n = app.global::<I18n>();
    // top bar
    i18n.set_tab_play(t!(lang, "tab-play").into());
    i18n.set_tab_replays(t!(lang, "tab-replays").into());
    // play tab
    i18n.set_play_no_game(t!(lang, "play-no-game").into());
    i18n.set_play_no_patch(t!(lang, "play-no-patch").into());
    i18n.set_play_no_save(t!(lang, "play-no-save").into());
    i18n.set_play_play(t!(lang, "play-play").into());
    i18n.set_play_fight(t!(lang, "play-fight").into());
    i18n.set_play_link_code(t!(lang, "play-link-code").into());
    i18n.set_empty_no_roms_title(t!(lang, "empty-no-roms-title").into());
    // tango renders the body ("Drop your … .gba files into:") over the
    // roms path; fold the path into the same string here.
    i18n.set_empty_no_roms_body(format!("{}\n{}", t!(lang, "empty-no-roms-body"), config.roms_path().display()).into());
    i18n.set_empty_select_title(t!(lang, "play-no-selection").into());
    // save viewer
    i18n.set_navi_base_hp(t!(lang, "navi-base-hp").into());
    i18n.set_navi_buster_attack(t!(lang, "navi-buster-attack").into());
    i18n.set_navi_buster_rapid(t!(lang, "navi-buster-rapid").into());
    i18n.set_navi_buster_charge(t!(lang, "navi-buster-charge").into());
    i18n.set_folder_group(t!(lang, "folder-group").into());
    i18n.set_save_empty(t!(lang, "save-empty").into());
    // lobby band
    i18n.set_lobby_you(t!(lang, "play-you").into());
    i18n.set_lobby_opponent(t!(lang, "play-opponent").into());
    i18n.set_lobby_match_type(t!(lang, "lobby-match-type").into());
    i18n.set_lobby_frame_delay(t!(lang, "settings-netplay-frame-delay").into());
    i18n.set_lobby_blind(t!(lang, "lobby-blind-mine").into());
    i18n.set_lobby_ready(t!(lang, "lobby-ready").into());
    i18n.set_lobby_unready(t!(lang, "lobby-unready").into());
    i18n.set_lobby_starting(t!(lang, "lobby-match-starting").into());
    // replays tab
    i18n.set_replays_all_games(t!(lang, "replays-filter-all-games").into());
    i18n.set_replays_opponent_placeholder(t!(lang, "replays-filter-opponent-placeholder").into());
    i18n.set_replays_select_prompt(t!(lang, "replays-select-prompt").into());
    i18n.set_replays_watch(t!(lang, "replays-watch").into());
    // settings
    i18n.set_settings_general(t!(lang, "settings-section-general").into());
    i18n.set_settings_graphics(t!(lang, "settings-section-graphics").into());
    i18n.set_settings_audio(t!(lang, "settings-section-audio").into());
    i18n.set_settings_input(t!(lang, "settings-section-input").into());
    i18n.set_settings_netplay(t!(lang, "settings-section-netplay").into());
    i18n.set_settings_about(t!(lang, "settings-section-about").into());
    i18n.set_settings_nickname(t!(lang, "settings-nickname").into());
    i18n.set_settings_language(t!(lang, "settings-language").into());
    i18n.set_settings_theme(t!(lang, "settings-theme").into());
    i18n.set_settings_theme_dark(t!(lang, "settings-theme-dark").into());
    i18n.set_settings_theme_light(t!(lang, "settings-theme-light").into());
    // tango-ng's "(Enter to apply + rescan)" hint has no key; the
    // localized label is just "Data folder".
    i18n.set_settings_data_folder(t!(lang, "settings-data-folder").into());
    i18n.set_settings_volume(t!(lang, "settings-volume").into());
    i18n.set_settings_fractional(t!(lang, "settings-fractional-scaling").into());
    // about
    app.set_app_version(t!(lang, "updater-current-version", version = format!("v{}", env!("CARGO_PKG_VERSION"))).into());
}

struct State {
    config: config::Config,
    audio_binder: audio::LateBinder,
    roms: HashMap<rom::GameRef, Vec<u8>>,
    saves: HashMap<rom::GameRef, Vec<save::ScannedSave>>,
    /// Games shown in the list, parallel to the `games` model rows.
    game_rows: Vec<rom::GameRef>,
    /// Saves shown for the selected game, parallel to the `saves` model rows.
    save_rows: Vec<save::ScannedSave>,
    /// All scanned replays (newest first).
    replay_rows: Vec<replays::ScannedReplay>,
    /// Indices into `replay_rows` currently shown, parallel to the
    /// `replays` model rows (the filters narrow this).
    replay_filtered: Vec<usize>,
    /// Family ids behind the game-filter picker (model index i+1 —
    /// index 0 is "All games").
    replay_filter_families: Vec<String>,
    /// Lowercased opponent substring filter.
    replay_filter_opponent: String,
    /// Replay shown in the detail pane + its lines (stats append lazily).
    replay_detail_path: Option<std::path::PathBuf>,
    replay_detail_lines: Vec<SharedString>,
    patches: patch::PatchMap,
    /// Patch names shown in the patch picker (model index i+1 — index 0
    /// is the "No patch" sentinel).
    patch_rows: Vec<String>,
    /// Versions shown in the version picker, newest first.
    version_rows: Vec<semver::Version>,
    /// The save viewer's parsed save + ROM assets + baked sprite
    /// images for the current (game, patch, save) selection. Rebuilt
    /// by [`refresh_loaded`]; `None` while nothing is selected.
    loaded: Option<loaded::Loaded>,
    session: Option<ActiveSession>,
    joyflags: u32,
    /// Replay speed selected in the transport (fast-forward restores it).
    replay_speed: f32,
    /// Whether playback was running when a scrub drag started (the drag
    /// pauses; commit resumes iff this was set).
    scrub_was_playing: bool,
    /// Whether this drag has already blitted a keyframe preview — from
    /// then on nearest-keyframe previews are forced (the live frame is
    /// no longer in the buffer).
    scrub_forced: bool,
    /// Netplay connection state machine, driven by the link-code bar
    /// (matchmaking codes, `/host` + `/connect`) and the timer's event
    /// fold. The lobby band renders off it via `refresh_lobby_ui`.
    netplay: netplay::State,
    /// The persistent client identity presented to the matchmaking
    /// server as an mTLS client cert, loaded once at startup and
    /// threaded into each `Message::Connect`. `None` = dial without one.
    identity: Option<tango_signaling::ClientIdentity>,
    /// `(mode, subtype)` rows behind the lobby's match-type picker,
    /// parallel to the `lobby-match-types` model.
    lobby_mt_rows: Vec<(u8, u8)>,
    /// The lobby band's last-pushed property values — `refresh_lobby_ui`
    /// diffs against this so idle ticks don't invalidate Slint
    /// properties (or fight the two-way-bound controls mid-drag).
    lobby_ui: LobbySnapshot,
}

/// At most one session is active at a time.
enum ActiveSession {
    SinglePlayer(session::SinglePlayerSession),
    Replay(session::ReplaySession),
    Pvp(Box<pvp::PvpSession>),
}

impl ActiveSession {
    fn frame_dirty(&self) -> bool {
        match self {
            ActiveSession::SinglePlayer(s) => s.frame_dirty(),
            ActiveSession::Replay(s) => s.frame_dirty(),
            ActiveSession::Pvp(s) => s.frame_dirty(),
        }
    }

    fn read_frame(&self, dst_rgba: &mut [u8]) {
        match self {
            ActiveSession::SinglePlayer(s) => s.read_frame(dst_rgba),
            ActiveSession::Replay(s) => s.read_frame(dst_rgba),
            ActiveSession::Pvp(s) => s.read_frame(dst_rgba),
        }
    }
}

/// Resolve one replay side's ROM: registry lookup by (family, variant),
/// raw bytes from the scan, BPS reapplied if the side was patched.
fn resolve_replay_rom(
    roms: &HashMap<rom::GameRef, Vec<u8>>,
    patches_path: &std::path::Path,
    side: Option<&tango_pvp::replay::metadata::Side>,
) -> anyhow::Result<(rom::GameRef, Vec<u8>)> {
    let gi = side
        .and_then(|s| s.game_info.as_ref())
        .ok_or_else(|| anyhow::anyhow!("replay side has no game info"))?;
    let variant = u8::try_from(gi.rom_variant)?;
    let game = game::find_by_family_and_variant(&gi.rom_family, variant)
        .ok_or_else(|| anyhow::anyhow!("unknown game {}/{}", gi.rom_family, gi.rom_variant))?;
    let rom = roms
        .get(&game)
        .ok_or_else(|| anyhow::anyhow!("no ROM scanned for {}/{}", gi.rom_family, gi.rom_variant))?;
    let rom = if let Some(patch_info) = gi.patch.as_ref() {
        let version = semver::Version::parse(&patch_info.version)?;
        patch::apply_patch_from_disk(rom, game, patches_path, &patch_info.name, &version)?
    } else {
        rom.clone()
    };
    Ok((game, rom))
}

/// Build a [`session::ReplaySession`] for a replay file.
fn start_replay(
    path: &std::path::Path,
    roms: &HashMap<rom::GameRef, Vec<u8>>,
    patches_path: &std::path::Path,
    audio_binder: &audio::LateBinder,
) -> anyhow::Result<session::ReplaySession> {
    let f = std::fs::File::open(path)?;
    let replay = tango_pvp::replay::Replay::decode(f)?;
    let (local_game, local_rom) = resolve_replay_rom(roms, patches_path, replay.metadata.local_side.as_ref())?;
    let (remote_game, remote_rom) = resolve_replay_rom(roms, patches_path, replay.metadata.remote_side.as_ref())?;
    session::ReplaySession::new(replay, local_game, local_rom, remote_game, remote_rom, audio_binder)
}

/// Rebuild the games + replays models (and clear selections) from the
/// scanned state — after a scan lands or the display language changes.
fn refresh_models(app: &AppWindow, st: &mut State) {
    let lang = st.config.language.clone();
    let mut game_rows: Vec<rom::GameRef> = st.roms.keys().copied().collect();
    game_rows.sort_by_key(|g| game::display_name(&lang, *g));
    let rows: Vec<SharedString> = game_rows
        .iter()
        .map(|g| game::display_name(&lang, *g).into())
        .collect();
    st.game_rows = game_rows;
    st.save_rows.clear();
    st.patch_rows.clear();
    st.version_rows.clear();
    st.loaded = None;

    // Family filter options, from the families actually seen in replays.
    let mut families: Vec<String> = st
        .replay_rows
        .iter()
        .filter_map(|r| {
            r.metadata
                .local_side
                .as_ref()
                .and_then(|s| s.game_info.as_ref())
                .map(|gi| gi.rom_family.clone())
        })
        .collect();
    families.sort();
    families.dedup();
    let mut filter_model: Vec<SharedString> = vec![SharedString::from(t!(&lang, "replays-filter-all-games"))];
    filter_model.extend(
        families
            .iter()
            .map(|f| SharedString::from(game::family_display_name(&lang, f))),
    );
    st.replay_filter_families = families;
    app.set_replay_game_filters(ModelRc::new(VecModel::from(filter_model)));
    app.set_selected_replay_filter(0);

    // tango-ng-only scan summary — no tango key; stays English.
    app.set_status(
        format!(
            "{} games · {} saves · {} replays",
            st.game_rows.len(),
            st.saves.values().map(|v| v.len()).sum::<usize>(),
            st.replay_rows.len()
        )
        .into(),
    );
    app.set_selected_game(-1);
    app.set_selected_save(-1);
    app.set_selected_patch(0);
    app.set_selected_version(-1);
    app.set_games(ModelRc::new(VecModel::from(rows)));
    app.set_saves(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
    app.set_patches(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
    app.set_versions(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
    push_save_view(app, st);
    apply_replay_filter(app, st, None);
}

/// Rebuild the shown-replay indices + model from the current filters.
/// `family` is the family-id filter (None = all).
fn apply_replay_filter(app: &AppWindow, st: &mut State, family: Option<&str>) {
    let lang = st.config.language.clone();
    let opponent = st.replay_filter_opponent.clone();
    st.replay_filtered = st
        .replay_rows
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            let family_ok = family.is_none_or(|f| {
                r.metadata
                    .local_side
                    .as_ref()
                    .and_then(|s| s.game_info.as_ref())
                    .is_some_and(|gi| gi.rom_family == f)
            });
            let opponent_ok = opponent.is_empty()
                || r.metadata
                    .remote_side
                    .as_ref()
                    .is_some_and(|s| s.nickname.to_lowercase().contains(&opponent));
            family_ok && opponent_ok
        })
        .map(|(i, _)| i)
        .collect();

    let rows: Vec<ReplayRow> = st
        .replay_filtered
        .iter()
        .map(|&i| replay_row(&lang, &st.replay_rows[i]))
        .collect();
    app.set_selected_replay(-1);
    app.set_replays(ModelRc::new(VecModel::from(rows)));
}

/// One replay-list row's display strings: "timestamp" over
/// "game @ code · p1 vs p2", like the tango replay list.
fn replay_row(lang: &unic_langid::LanguageIdentifier, replay: &replays::ScannedReplay) -> ReplayRow {
    let md = &replay.metadata;
    let game_name = md
        .local_side
        .as_ref()
        .and_then(|s| s.game_info.as_ref())
        .map(|gi| {
            u8::try_from(gi.rom_variant)
                .ok()
                .and_then(|v| game::find_by_family_and_variant(&gi.rom_family, v))
                .map(|g| game::display_name(lang, g))
                .unwrap_or_else(|| gi.rom_family.clone())
        })
        .unwrap_or_else(|| "?".to_string());
    let local = md.local_side.as_ref().map(|s| s.nickname.as_str()).unwrap_or("?");
    let remote = md.remote_side.as_ref().map(|s| s.nickname.as_str()).unwrap_or("?");
    ReplayRow {
        line1: replays::format_ts(md.ts, "%Y-%m-%d %H:%M:%S").into(),
        line2: format!("{} @ {}  ·  {} vs {}", game_name, md.link_code, local, remote).into(),
    }
}

/// Recognise the direct link-code commands the user can type in place
/// of a matchmaking code (copied from `tango/src/tabs/play/mod.rs`):
///
/// - `/host` — listen on [`net::DEFAULT_LOCAL_PORT`]
/// - `/host <port>` — listen on the given port
/// - `/connect <addr>` — dial `<addr>`, appending the default port if
///   the user didn't specify one
fn parse_direct_command(input: &str) -> Option<netplay::DirectRole> {
    // The leading slash is the disambiguator — without it, any
    // input is a matchmaking link code (which can legitimately
    // contain letters, digits, and the random-code separators).
    if !input.starts_with('/') {
        return None;
    }
    let mut parts = input.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().map(str::trim).unwrap_or("");
    match cmd {
        "/host" => {
            let port = if arg.is_empty() {
                net::DEFAULT_LOCAL_PORT
            } else {
                arg.parse::<u16>().ok()?
            };
            Some(netplay::DirectRole::Host { port })
        }
        "/connect" => {
            if arg.is_empty() {
                return None;
            }
            // Heuristic: if the user gave no colon (bare IP) or
            // their input ends with the IPv6 closing bracket
            // without a trailing colon, append the default port.
            // We deliberately don't try to validate the address
            // itself — the connect error surfaces well.
            let addr = if arg.contains(':') && !arg.ends_with(']') {
                arg.to_string()
            } else {
                format!("{arg}:{}", net::DEFAULT_LOCAL_PORT)
            };
            Some(netplay::DirectRole::Connect { addr })
        }
        _ => None,
    }
}

/// One-line description of the netplay lifecycle, used by the
/// `--smoke-pvp` driver's progress prints (the GUI's lobby band derives
/// its own, richer status via `compute_lobby_snapshot`).
fn netplay_status_text(netplay: &netplay::State) -> String {
    if netplay.handoff_pending() {
        return "Starting match…".to_string();
    }
    match &netplay.phase {
        netplay::Phase::Idle => String::new(),
        netplay::Phase::Connecting {
            waiting_for_opponent: false,
            ..
        } => "Connecting…".to_string(),
        netplay::Phase::Connecting {
            waiting_for_opponent: true,
            ..
        } => "Waiting for opponent…".to_string(),
        netplay::Phase::Negotiating { .. } => "Negotiating…".to_string(),
        netplay::Phase::Lobby { .. } => match netplay.lobby.remote.as_ref() {
            None => "Lobby — waiting for opponent…".to_string(),
            Some(remote) if netplay.lobby.local_ready && netplay.lobby.remote_ready => {
                format!("Lobby — starting vs {}…", remote.nickname)
            }
            Some(remote) => format!("Lobby — vs {}", remote.nickname),
        },
        netplay::Phase::Failed { error } => format!("Failed: {error}"),
    }
}

/// Headless auto-lobby for the `--smoke-pvp` driver (the GUI has the
/// real lobby band): it advertises the given selection as its Settings
/// the moment the phase reaches Lobby (the dedupe inside
/// `SendLocalSettings` makes repeats no-ops, and a material change
/// still auto-uncommits, like tango's resend pass), then auto-Readies
/// (`Commit` with the selected save's SRAM) once both sides' settings
/// are in and the compat verdict is green (tango's Ready→Commit,
/// app/update.rs:100-110). Match type is pinned to (0, 0) — the lobby
/// default — so both sides agree without a picker; blind setup stays
/// off.
fn drive_auto_lobby(
    netplay: &mut netplay::State,
    patches: &patch::PatchMap,
    nickname: &str,
    game: rom::GameRef,
    game_patch: Option<(String, semver::Version)>,
    save: &save::ScannedSave,
) {
    if !matches!(netplay.phase, netplay::Phase::Lobby { .. }) {
        return;
    }
    let (family, variant) = game.family_and_variant();
    let settings = net::protocol::Settings {
        nickname: nickname.to_string(),
        match_type: (0, 0),
        game_info: Some(net::protocol::GameInfo {
            family_and_variant: (family.to_string(), variant),
            patch: game_patch.map(|(name, version)| net::protocol::PatchInfo { name, version }),
        }),
        blind_setup: false,
    };
    netplay.update(netplay::Message::SendLocalSettings(Box::new(settings)));
    if netplay.lobby.local_ready {
        return;
    }
    let (Some(local), Some(remote)) = (netplay.lobby.local.as_ref(), netplay.lobby.remote.as_ref()) else {
        return;
    };
    if !matches!(
        netplay::compat::check(local, remote, patches),
        netplay::compat::Verdict::Compatible
    ) {
        return;
    }
    netplay.update(netplay::Message::Commit {
        save_sram: save.save.to_sram_dump(),
    });
}

/// The Play tab's current (game, patch, save-row) pick, or `None` when
/// incomplete. Mirrors `on_play_clicked`'s reads; the netplay slice
/// uses it for the lobby Settings and the PvP ROM resolve.
fn selected_loadout(app: &AppWindow, st: &State) -> Option<(rom::GameRef, Option<(String, semver::Version)>, usize)> {
    let game = st.game_rows.get(app.get_selected_game() as usize).copied()?;
    let save_index = app.get_selected_save();
    if save_index < 0 || save_index as usize >= st.save_rows.len() {
        return None;
    }
    let patch = if app.get_selected_patch() > 0 {
        // A patch is picked but its version row is missing → incomplete.
        Some(selected_patch(app, st)?)
    } else {
        None
    };
    Some((game, patch, save_index as usize))
}

/// The patch pickers' current (name, version), or `None` when "No
/// patch" is selected or the version row is missing.
fn selected_patch(app: &AppWindow, st: &State) -> Option<(String, semver::Version)> {
    let patch_index = app.get_selected_patch();
    if patch_index <= 0 {
        return None;
    }
    Some((
        st.patch_rows.get(patch_index as usize - 1)?.clone(),
        st.version_rows.get(app.get_selected_version() as usize)?.clone(),
    ))
}

/// The Play tab's currently-picked game, if any (no save required —
/// the lobby advertises the game as soon as it's selected, like tango).
fn selected_game(app: &AppWindow, st: &State) -> Option<rom::GameRef> {
    st.game_rows.get(app.get_selected_game() as usize).copied()
}

/// Rebuild the save viewer's [`loaded::Loaded`] for the current (game,
/// patch, save) selection — or clear it when the pick is incomplete —
/// then push the derived view models. Runs on every save / patch /
/// version selection change; rescans and language changes land here
/// via [`refresh_models`]'s cleared selection.
fn refresh_loaded(app: &AppWindow, st: &mut State) {
    st.loaded = None;
    let save_index = app.get_selected_save();
    if let (Some(game), Some(save)) = (
        selected_game(app, st),
        usize::try_from(save_index).ok().and_then(|i| st.save_rows.get(i)),
    ) {
        if let Some(rom) = st.roms.get(&game) {
            // Bake from what Play would boot: the selected patch when
            // the pick is complete. (Patch chosen but no version row —
            // possible only when the patch has no versions for this
            // game — renders unpatched, exactly what Play would refuse
            // to start.)
            let patch = selected_patch(app, st);
            st.loaded = Some(loaded::Loaded::build(
                game,
                rom,
                save.save.clone_box(),
                &st.config.patches_path(),
                patch,
            ));
        }
    }
    push_save_view(app, st);
}

/// Push the save viewer's models — navi header, section tabs, folder
/// rows — from `st.loaded`, or clear the whole viewer when it's gone.
fn push_save_view(app: &AppWindow, st: &State) {
    let Some(l) = st.loaded.as_ref() else {
        app.set_save_loaded(false);
        app.set_navi_header(NaviHeader::default());
        app.set_save_tabs(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
        app.set_folder_chips(ModelRc::new(VecModel::from(Vec::<ChipRow>::new())));
        return;
    };
    let lang = &st.config.language;
    app.set_navi_header(loaded::navi_header(l));
    // Section gating like tango's available_tabs — each tab exists iff
    // its view does. Only Folder is ported so far; the others append
    // here as they land.
    let mut tabs: Vec<SharedString> = Vec::new();
    if l.save.view_chips().is_some() {
        tabs.push(t!(lang, "save-tab-folder").into());
    }
    app.set_save_tabs(ModelRc::new(VecModel::from(tabs)));
    app.set_save_active_tab(0);
    app.set_folder_has_mb(l.assets.chips_have_mb());
    app.set_folder_chips(ModelRc::new(VecModel::from(loaded::folder_rows(
        l,
        app.get_folder_grouped(),
    ))));
    app.set_save_loaded(true);
}

/// Single source of truth for the local side's `protocol::Settings`
/// (tango's `Loadout::make_local_settings`, loadout.rs:217): built from
/// the current selection + lobby-local state. Also the "You" card
/// fallback before `netplay.lobby.local` has round-tripped.
fn make_local_settings(app: &AppWindow, st: &State) -> net::protocol::Settings {
    net::protocol::Settings {
        nickname: st.config.nickname.clone().unwrap_or_default(),
        match_type: st.netplay.lobby.match_type,
        game_info: selected_game(app, st).map(|game| {
            let (family, variant) = game.family_and_variant();
            net::protocol::GameInfo {
                family_and_variant: (family.to_string(), variant),
                patch: selected_patch(app, st).map(|(name, version)| net::protocol::PatchInfo { name, version }),
            }
        }),
        blind_setup: st.netplay.lobby.blind_setup,
    }
}

/// tango's `App::apply_default_match_type` (app/mod.rs:511): when the
/// game changed since the last default was applied (or the current pick
/// is invalid for it), default to Triple (mode 1) where the game
/// supports it, else Single. Keyed off `default_mt_for_game` so an
/// explicit user pick for the SAME game sticks.
fn apply_default_match_type(netplay: &mut netplay::State, game: rom::GameRef) {
    let mt_table = game.match_types;
    let game_key = {
        let (family, variant) = game.family_and_variant();
        (family.to_string(), variant)
    };
    let game_changed = netplay.lobby.default_mt_for_game.as_ref() != Some(&game_key);
    let (mode, sub) = netplay.lobby.match_type;
    let current_valid = (mode as usize) < mt_table.len() && (sub as usize) < *mt_table.get(mode as usize).unwrap_or(&0);
    if game_changed || !current_valid {
        netplay.lobby.match_type = if mt_table.get(1).copied().unwrap_or(0) > 0 {
            (1, 0) // Triple
        } else {
            (0, 0) // Single
        };
        netplay.lobby.default_mt_for_game = Some(game_key);
    }
}

/// tango's `App::resend_settings_if_lobby` (app/mod.rs:537): no-op
/// outside Lobby phase; otherwise re-apply the default-match-type
/// policy and push the current selection's Settings — the dedupe
/// inside `SendLocalSettings` makes repeats no-ops, and a material
/// change auto-uncommits. Called once per timer tick while netplay is
/// live, which covers both lobby entry and any mid-lobby selection
/// change (the save/version pickers have no change callbacks to hook).
fn resend_settings_if_lobby(app: &AppWindow, st: &mut State) {
    if !matches!(st.netplay.phase, netplay::Phase::Lobby { .. }) {
        return;
    }
    if let Some(game) = selected_game(app, st) {
        apply_default_match_type(&mut st.netplay, game);
    }
    let settings = make_local_settings(app, st);
    st.netplay.update(netplay::Message::SendLocalSettings(Box::new(settings)));
}

/// The lobby's compat verdict, once both sides' settings are in.
fn lobby_verdict(st: &State) -> Option<netplay::compat::Verdict> {
    let local = st.netplay.lobby.local.as_ref()?;
    let remote = st.netplay.lobby.remote.as_ref()?;
    Some(netplay::compat::check(local, remote, &st.patches))
}

/// A lobby side card's caption line: "<game> · <patch> · <match-type>"
/// (tango's lobby.rs `side_card_subline`).
fn side_card_subline(lang: &unic_langid::LanguageIdentifier, settings: &net::protocol::Settings) -> String {
    let Some(gi) = settings.game_info.as_ref() else {
        return t!(lang, "lobby-no-game");
    };
    let mut subline = game::family_display_name(lang, &gi.family_and_variant.0);
    if let Some(p) = gi.patch.as_ref() {
        subline.push_str(&format!(" · {} v{}", p.name, p.version));
    }
    subline.push_str(&format!(
        " · {}",
        game::match_type_name(lang, &gi.family_and_variant.0, settings.match_type.0, settings.match_type.1)
    ));
    subline
}

/// Map netplay's error sentinels to user copy (tango's `play-status-*`
/// Fluent keys).
fn lobby_failed_text(lang: &unic_langid::LanguageIdentifier, error: &str) -> String {
    match error {
        "peer-disconnected" => t!(lang, "play-status-peer-disconnected"),
        "negotiate-expected-hello" => t!(lang, "play-status-negotiate-expected-hello"),
        "negotiate-version-too-old" => t!(lang, "play-status-negotiate-version-too-old"),
        "negotiate-version-too-new" => t!(lang, "play-status-negotiate-version-too-new"),
        other if other.starts_with("negotiate-other: ") => t!(
            lang,
            "play-status-negotiate-failed",
            error = other.trim_start_matches("negotiate-other: ")
        ),
        other => t!(lang, "play-status-failed", error = other),
    }
}

/// Everything the lobby band renders. Computed fresh each tick by
/// [`compute_lobby_snapshot`] and diffed field-by-field against the
/// previous push in [`refresh_lobby_ui`].
#[derive(Clone, PartialEq)]
struct LobbySnapshot {
    visible: bool,
    failed: bool,
    inert: bool,
    status: String,
    /// 0 = in-flight (muted), 1 = good (green), 2 = bad (red).
    tone: i32,
    conn_detail: String,
    local_nickname: String,
    local_subline: String,
    local_ready: bool,
    remote_nickname: String,
    remote_subline: String,
    remote_ready: bool,
    mt_labels: Vec<String>,
    mt_selected: i32,
    /// Normalized slider position: (frame_delay - 2) / 8.
    frame_delay_norm: f32,
    frame_delay_label: String,
    blind: bool,
    /// 0 = Ready, 1 = Unready, 2 = Starting….
    ready_state: i32,
    ready_enabled: bool,
    suggest_enabled: bool,
}

impl Default for LobbySnapshot {
    /// Matches the `.slint` property defaults, so the first diff pass
    /// (and every idle tick) pushes nothing.
    fn default() -> Self {
        Self {
            visible: false,
            failed: false,
            inert: false,
            status: String::new(),
            tone: 0,
            conn_detail: String::new(),
            local_nickname: String::new(),
            local_subline: String::new(),
            local_ready: false,
            remote_nickname: "—".to_string(),
            remote_subline: "—".to_string(),
            remote_ready: false,
            mt_labels: Vec::new(),
            mt_selected: -1,
            frame_delay_norm: 0.0,
            frame_delay_label: "2".to_string(),
            blind: false,
            ready_state: 0,
            ready_enabled: false,
            suggest_enabled: false,
        }
    }
}

/// Derive this frame's lobby band content from netplay + config +
/// selection state — the Slint rebuild of tango's `Lobby::view`
/// (tabs/play/lobby.rs). Returns the snapshot plus the `(mode,
/// subtype)` rows behind the match-type model. Everything defaults
/// (band hidden) when netplay is Idle with no handoff pending.
fn compute_lobby_snapshot(app: &AppWindow, st: &State) -> (LobbySnapshot, Vec<(u8, u8)>) {
    let handoff = st.netplay.handoff_pending();
    if matches!(st.netplay.phase, netplay::Phase::Idle) && !handoff {
        return (LobbySnapshot::default(), Vec::new());
    }
    let lang = &st.config.language;
    let lobby = &st.netplay.lobby;
    let failed = matches!(st.netplay.phase, netplay::Phase::Failed { .. });
    // Controls refuse input without changing layout while the lobby is
    // dead or the match is spinning up (tango's `Lobby::inert`).
    let inert = failed || handoff;

    // Status line + tone: connection progress until the lobby is up,
    // then the compat verdict between the two settings (tango's
    // `Status` enum, kept as one derivation so the Ready gate below
    // can't drift from the text).
    let verdict = lobby_verdict(st);
    let (status, tone) = match &st.netplay.phase {
        netplay::Phase::Failed { error } => (lobby_failed_text(lang, error), 2),
        netplay::Phase::Connecting {
            ident,
            waiting_for_opponent: false,
        } => (
            // Direct `/connect` dials straight at the peer — the
            // matchmaking copy would be wrong.
            if matches!(ident, netplay::LinkIdent::Direct(netplay::DirectRole::Connect { .. })) {
                t!(lang, "play-status-direct-connecting")
            } else {
                t!(lang, "play-status-connecting")
            },
            0,
        ),
        netplay::Phase::Connecting {
            waiting_for_opponent: true,
            ..
        } => (t!(lang, "play-status-waiting-opponent"), 0),
        netplay::Phase::Negotiating { .. } => (t!(lang, "play-status-negotiating"), 0),
        netplay::Phase::Lobby { .. } | netplay::Phase::Idle => match &verdict {
            Some(netplay::compat::Verdict::Compatible) => (t!(lang, "lobby-compat-ok"), 1),
            Some(netplay::compat::Verdict::MissingGame) => (t!(lang, "lobby-compat-missing-game"), 2),
            Some(netplay::compat::Verdict::DifferentVersions) => (t!(lang, "lobby-compat-version-mismatch"), 2),
            Some(netplay::compat::Verdict::DifferentMatchTypes) => (t!(lang, "lobby-compat-match-mismatch"), 2),
            None => (t!(lang, "lobby-handshake"), 0),
        },
    };

    // The caption under the status: the *identifier* (link code /
    // direct target) while dialing, the *wire* (live ping) once
    // measured; nothing on a dead lobby (the status carries the
    // failure).
    let ident = match &st.netplay.phase {
        netplay::Phase::Connecting { ident, .. }
        | netplay::Phase::Negotiating { ident }
        | netplay::Phase::Lobby { ident } => Some(ident),
        _ => None,
    };
    let conn_detail = if failed {
        String::new()
    } else if let Some(d) = matches!(st.netplay.phase, netplay::Phase::Lobby { .. })
        .then(|| lobby.latency_counter.latest())
        .flatten()
    {
        let ms = d.as_millis() as i64;
        match lobby.connection_kind {
            Some(netplay::ConnectionKind::Direct) => t!(lang, "lobby-latency-direct", ms = ms),
            Some(netplay::ConnectionKind::Relayed) => t!(lang, "lobby-latency-relayed", ms = ms),
            None => t!(lang, "lobby-latency", ms = ms),
        }
    } else {
        match ident {
            Some(netplay::LinkIdent::Matchmaking(code)) => t!(lang, "lobby-link-code", code = code.clone()),
            Some(netplay::LinkIdent::Direct(netplay::DirectRole::Host { port })) => {
                // Stringified so Fluent can't locale-format the number.
                t!(lang, "lobby-direct-host", port = port.to_string())
            }
            Some(netplay::LinkIdent::Direct(netplay::DirectRole::Connect { addr })) => {
                t!(lang, "lobby-direct-connect", target = addr.clone())
            }
            None => String::new(),
        }
    };

    // Side cards. "You" falls back to Settings synthesized from the
    // current selection until `lobby.local` lands; the opponent shows
    // placeholders until their Settings arrive. Ready lights go dark on
    // failure (the lobby those flags belonged to is gone), but the
    // opponent's info stays — "who just left" is what you want to read
    // off a dead lobby.
    let local_settings = lobby.local.clone().unwrap_or_else(|| make_local_settings(app, st));
    let card_nickname = |nickname: &str| {
        if nickname.is_empty() {
            "—".to_string()
        } else {
            nickname.to_string()
        }
    };
    let (remote_nickname, remote_subline) = match lobby.remote.as_ref() {
        Some(s) => (card_nickname(&s.nickname), side_card_subline(lang, s)),
        None => ("—".to_string(), "—".to_string()),
    };

    // Match-type rows from the local game's mode/subtype table
    // (tango's `Lobby::match_type_picker`).
    let mut mt_rows = Vec::new();
    let mut mt_labels = Vec::new();
    if let Some(game) = selected_game(app, st) {
        let family = game.family_and_variant().0;
        for (mode, subtype_count) in game.match_types.iter().enumerate() {
            for sub in 0..*subtype_count {
                mt_rows.push((mode as u8, sub as u8));
                mt_labels.push(game::match_type_name(lang, family, mode as u8, sub as u8));
            }
        }
    }
    let mt_selected = mt_rows
        .iter()
        .position(|&mt| mt == lobby.match_type)
        .map(|i| i as i32)
        .unwrap_or(-1);

    let frame_delay = st
        .config
        .frame_delay
        .clamp(tango_pvp::battle::MIN_FRAME_DELAY, tango_pvp::battle::MAX_FRAME_DELAY);
    let frame_delay_norm = (frame_delay - tango_pvp::battle::MIN_FRAME_DELAY) as f32
        / (tango_pvp::battle::MAX_FRAME_DELAY - tango_pvp::battle::MIN_FRAME_DELAY) as f32;

    // Ready → Unready → Starting…, one button (tango's ready_button).
    let ready_state = if lobby.match_ready {
        2
    } else if lobby.local_ready {
        1
    } else {
        0
    };
    let has_save = {
        let save_index = app.get_selected_save();
        save_index >= 0 && (save_index as usize) < st.save_rows.len()
    };
    let compat_ok = matches!(verdict, Some(netplay::compat::Verdict::Compatible));
    let ready_enabled = !inert
        && match ready_state {
            0 => compat_ok && has_save,
            1 => true,
            _ => false,
        };

    let snap = LobbySnapshot {
        visible: true,
        failed,
        inert,
        status,
        tone,
        conn_detail,
        local_nickname: card_nickname(&local_settings.nickname),
        local_subline: side_card_subline(lang, &local_settings),
        local_ready: lobby.local_ready && !failed,
        remote_nickname,
        remote_subline,
        remote_ready: lobby.remote_ready && !failed,
        mt_labels,
        mt_selected,
        frame_delay_norm,
        frame_delay_label: frame_delay.to_string(),
        blind: lobby.blind_setup,
        ready_state,
        ready_enabled,
        // Suggest needs a real reading to take the median of — enabled
        // once the first Pong lands.
        suggest_enabled: !inert && lobby.latency_counter.latest().is_some(),
    };
    (snap, mt_rows)
}

/// Refresh the lobby band: compute this frame's snapshot and push only
/// the properties whose values actually changed since the last push.
fn refresh_lobby_ui(app: &AppWindow, st: &mut State) {
    let (snap, mt_rows) = compute_lobby_snapshot(app, st);
    st.lobby_mt_rows = mt_rows;
    let prev = &st.lobby_ui;
    macro_rules! push {
        ($field:ident, $setter:ident) => {
            if snap.$field != prev.$field {
                app.$setter(snap.$field.clone().into());
            }
        };
    }
    push!(visible, set_lobby_visible);
    push!(failed, set_lobby_failed);
    push!(inert, set_lobby_inert);
    push!(status, set_lobby_status);
    push!(tone, set_lobby_status_tone);
    push!(conn_detail, set_lobby_conn_detail);
    push!(local_nickname, set_lobby_local_nickname);
    push!(local_subline, set_lobby_local_subline);
    push!(local_ready, set_lobby_local_ready);
    push!(remote_nickname, set_lobby_remote_nickname);
    push!(remote_subline, set_lobby_remote_subline);
    push!(remote_ready, set_lobby_remote_ready);
    if snap.mt_labels != prev.mt_labels {
        app.set_lobby_match_types(ModelRc::new(VecModel::from(
            snap.mt_labels.iter().map(|s| SharedString::from(s.as_str())).collect::<Vec<_>>(),
        )));
    }
    push!(mt_selected, set_lobby_selected_match_type);
    push!(frame_delay_norm, set_lobby_frame_delay);
    push!(frame_delay_label, set_lobby_frame_delay_label);
    push!(blind, set_lobby_blind);
    push!(ready_state, set_lobby_ready_state);
    push!(ready_enabled, set_lobby_ready_enabled);
    push!(suggest_enabled, set_lobby_suggest_enabled);
    st.lobby_ui = snap;
}

/// `MatchHandoffReady`: drain the lobby state into a `PreMatchData`,
/// resolve both sides' ROMs, and kick off the async PvP build on the
/// runtime — its result lands back in the event loop as
/// `Event::PvpBuilt` (mirrors tango's app/mod.rs:1070-1106 handler).
/// A resolve failure after the drain can't be retried (the connection
/// handles are gone), so it surfaces in the status line and clears the
/// lobby snapshot right here.
fn start_pvp_handoff(app: &AppWindow, st: &mut State, rt: &tokio::runtime::Handle, tx: &std::sync::mpsc::Sender<Event>) {
    let Some(pre_match) = st.netplay.take_pre_match() else {
        return;
    };
    let resolved = selected_loadout(app, st)
        .ok_or_else(|| anyhow::anyhow!("no game/save selected"))
        .and_then(|(game, patch, _)| {
            pvp::resolve_pvp_roms(
                &st.roms,
                &st.config.patches_path(),
                game,
                patch,
                &pre_match.remote_settings,
            )
        });
    let roms = match resolved {
        Ok(roms) => roms,
        Err(e) => {
            log::error!("pvp rom resolve failed: {e:?}");
            app.set_status(format!("Failed to start match: {e}").into());
            st.netplay.finish_handoff();
            return;
        }
    };
    let frame_delay = st.config.frame_delay;
    let disable_bgm = st.config.disable_bgm_in_pvp;
    let replays_path = st.config.replays_path();
    let audio_binder = st.audio_binder.clone();
    let tx = tx.clone();
    rt.spawn(async move {
        let result = pvp::spawn_pvp(roms, pre_match, frame_delay, disable_bgm, replays_path, audio_binder).await;
        let _ = tx.send(Event::PvpBuilt(Box::new(result)));
    });
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("tango-ng {}", env!("CARGO_PKG_VERSION"));

    // The async (netplay) layer's runtime. Held for the program lifetime;
    // tasks get the Handle (threaded into netplay::State below) — the
    // emulator thread must never have the runtime *entered*
    // (PvpSender::send uses blocking_send).
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

    let config = config::Config::load();
    log::info!("data path: {}", config.data_path.display());

    let mut audio_binder = audio::LateBinder::new();
    // A dead audio device downgrades to silence rather than aborting; note
    // that with audio_sync on, emulation is paced by audio consumption, so
    // without a backend a session will stall once mgba's buffer fills.
    let _audio_backend = audio::backend::Backend::new(&mut audio_binder)
        .map_err(|e| log::warn!("audio disabled: {e:?}"))
        .ok();

    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--smoke") {
        let out = std::path::PathBuf::from(args.get(2).map(|s| s.as_str()).unwrap_or("tango-ng-smoke.png"));
        return smoke(&config, &audio_binder, &out);
    }
    if args.get(1).map(|s| s.as_str()) == Some("--smoke-replay") {
        let out = std::path::PathBuf::from(args.get(2).map(|s| s.as_str()).unwrap_or("tango-ng-replay.png"));
        return smoke_replay(&config, &audio_binder, &out);
    }
    if args.get(1).map(|s| s.as_str()) == Some("--smoke-pvp") {
        let role = args.get(2).cloned().unwrap_or_default();
        // `match` takes the shared link code as an extra argument, so
        // the output path shifts one slot right.
        let (link_code, out_arg) = if role == "match" {
            let code = args
                .get(3)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("--smoke-pvp match needs a <link_code>"))?;
            (Some(code), 4)
        } else {
            (None, 3)
        };
        let out = std::path::PathBuf::from(args.get(out_arg).map(|s| s.as_str()).unwrap_or("tango-ng-pvp.png"));
        return smoke_pvp(
            &config,
            &audio_binder,
            tokio_runtime.handle().clone(),
            &role,
            link_code.as_deref(),
            &out,
        );
    }
    let ui_shot_dir: Option<std::path::PathBuf> = (args.get(1).map(|s| s.as_str()) == Some("--ui-shot"))
        .then(|| std::path::PathBuf::from(args.get(2).map(|s| s.as_str()).unwrap_or(".")));

    let app = AppWindow::new()?;
    let shot_step = Rc::new(RefCell::new(0i32));

    // Main-loop event channel: background work (the scanner, replay
    // stats, netplay tasks + lobby loop) sends `Event`s; the 16ms timer
    // below drains them on the UI thread.
    let (tx, rx) = std::sync::mpsc::channel();

    let state = Rc::new(RefCell::new(State {
        config,
        audio_binder,
        roms: HashMap::new(),
        saves: HashMap::new(),
        game_rows: Vec::new(),
        save_rows: Vec::new(),
        replay_rows: Vec::new(),
        replay_filtered: Vec::new(),
        replay_filter_families: Vec::new(),
        replay_filter_opponent: String::new(),
        replay_detail_path: None,
        replay_detail_lines: Vec::new(),
        patches: patch::PatchMap::new(),
        patch_rows: Vec::new(),
        version_rows: Vec::new(),
        loaded: None,
        session: None,
        joyflags: 0,
        replay_speed: 1.0,
        scrub_was_playing: false,
        scrub_forced: false,
        netplay: netplay::State::new(tokio_runtime.handle().clone(), tx.clone()),
        // Loaded once here; every matchmaking Connect clones it.
        identity: identity::load(),
        lobby_mt_rows: Vec::new(),
        lobby_ui: LobbySnapshot::default(),
    }));

    // Background scan; results come back over the channel and are folded
    // into the UI by the frame timer below. Rc so the data-path setting
    // can retrigger it.
    let stats_tx = tx.clone();
    let pvp_tx = tx.clone();
    let spawn_scan: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let st = state.borrow();
            let roms_path = st.config.roms_path();
            let saves_path = st.config.saves_path();
            let replays_path = st.config.replays_path();
            let patches_path = st.config.patches_path();
            let tx = tx.clone();
            if let Some(app) = app_weak.upgrade() {
                app.set_status("Scanning…".into());
            }
            std::thread::spawn(move || {
                let roms = rom::scan_roms(&roms_path);
                let saves = save::scan_saves(&saves_path);
                let replays = replays::scan_replays(&replays_path);
                let patches = patch::scan(&patches_path);
                let _ = tx.send(Event::ScanDone {
                    roms,
                    saves,
                    replays,
                    patches,
                });
            });
        }
    });
    spawn_scan();

    // Seed the settings widgets + theme from config.
    {
        let st = state.borrow();
        // Language rows show each locale's endonym (its `LANGUAGE`
        // Fluent key — "日本語", not "ja-JP"), like tango's picker.
        app.set_languages(ModelRc::new(VecModel::from(
            i18n::SUPPORTED_LANGS
                .iter()
                .map(|id| SharedString::from(t_opt!(id, "LANGUAGE").unwrap_or_else(|| id.to_string())))
                .collect::<Vec<_>>(),
        )));
        apply_i18n(&app, &st.config);
        app.set_settings_nickname(st.config.nickname.as_deref().unwrap_or("").into());
        app.set_settings_data_path(st.config.data_path.display().to_string().into());
        app.set_settings_language(
            i18n::SUPPORTED_LANGS
                .iter()
                .position(|l| *l == st.config.language)
                .map(|i| i as i32)
                .unwrap_or(-1),
        );
        app.set_settings_theme(match st.config.theme {
            config::ThemeMode::Dark => 0,
            config::ThemeMode::Light => 1,
        });
        app.global::<Theme>()
            .set_light(st.config.theme == config::ThemeMode::Light);
        app.set_settings_volume(st.config.volume);
        app.set_settings_volume_label(format!("{}%", (st.config.volume * 100.0).round() as i32).into());
        app.set_fractional(st.config.fractional_scaling);
        st.audio_binder.set_volume(st.config.volume);
    }

    app.on_nickname_changed({
        let state = state.clone();
        move |text| {
            let mut st = state.borrow_mut();
            let text = text.trim();
            st.config.nickname = (!text.is_empty()).then(|| text.to_string());
            st.config.save();
        }
    });

    app.on_language_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let Some(lang) = i18n::SUPPORTED_LANGS.get(index as usize).cloned() else {
                return;
            };
            let mut st = state.borrow_mut();
            st.config.language = lang;
            st.config.save();
            // Relabel the chrome, then the models (game/replay rows and
            // their derived strings re-render in the new language). The
            // lobby band, if live, catches up on the next tick's
            // snapshot diff.
            apply_i18n(&app, &st.config);
            refresh_models(&app, &mut st);
        }
    });

    app.on_theme_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.config.theme = if index == 1 {
                config::ThemeMode::Light
            } else {
                config::ThemeMode::Dark
            };
            st.config.save();
            app.global::<Theme>()
                .set_light(st.config.theme == config::ThemeMode::Light);
        }
    });

    app.on_volume_changed({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |volume| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.config.volume = volume.clamp(0.0, 1.0);
            st.audio_binder.set_volume(st.config.volume);
            app.set_settings_volume_label(format!("{}%", (st.config.volume * 100.0).round() as i32).into());
            st.config.save();
        }
    });

    app.on_fractional_changed({
        let state = state.clone();
        move |fractional| {
            let mut st = state.borrow_mut();
            st.config.fractional_scaling = fractional;
            st.config.save();
        }
    });

    app.on_data_path_changed({
        let state = state.clone();
        let app_weak = app.as_weak();
        let spawn_scan = spawn_scan.clone();
        move |text| {
            {
                let mut st = state.borrow_mut();
                st.config.data_path = std::path::PathBuf::from(text.as_str());
                st.config.save();
                // The no-ROMs empty state renders the roms path — refresh it.
                if let Some(app) = app_weak.upgrade() {
                    apply_i18n(&app, &st.config);
                }
            }
            spawn_scan();
        }
    });

    app.on_game_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some(game) = st.game_rows.get(index as usize).copied() else {
                return;
            };
            st.save_rows = st.saves.get(&game).cloned().unwrap_or_default();
            let saves_path = st.config.saves_path();
            let rows: Vec<SharedString> = st
                .save_rows
                .iter()
                .map(|s| {
                    s.path
                        .strip_prefix(&saves_path)
                        .unwrap_or(&s.path)
                        .display()
                        .to_string()
                        .into()
                })
                .collect();
            app.set_saves(ModelRc::new(VecModel::from(rows)));

            // Patches supporting this game (any version).
            st.patch_rows = st
                .patches
                .iter()
                .filter(|(_, p)| p.versions.values().any(|v| v.supported_games.contains(&game)))
                .map(|(name, _)| name.clone())
                .collect();
            st.version_rows.clear();
            let mut patch_model: Vec<SharedString> =
                vec![SharedString::from(t!(&st.config.language, "play-no-patch"))];
            patch_model.extend(st.patch_rows.iter().map(|n| SharedString::from(n.as_str())));
            app.set_patches(ModelRc::new(VecModel::from(patch_model)));
            app.set_selected_patch(0);
            app.set_versions(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
            app.set_selected_version(-1);
            // The game switch reset the save pick (the Picker clears
            // `selected-save` before this callback) — drop the viewer.
            refresh_loaded(&app, &mut st);
        }
    });

    app.on_patch_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some(game) = st.game_rows.get(app.get_selected_game() as usize).copied() else {
                return;
            };
            if index <= 0 {
                st.version_rows.clear();
                app.set_versions(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
                app.set_selected_version(-1);
                refresh_loaded(&app, &mut st);
                return;
            }
            let Some(patch) = st
                .patch_rows
                .get(index as usize - 1)
                .and_then(|name| st.patches.get(name))
                .cloned()
            else {
                return;
            };
            st.version_rows = patch
                .versions
                .iter()
                .rev()
                .filter(|(_, v)| v.supported_games.contains(&game))
                .map(|(sv, _)| sv.clone())
                .collect();
            let model: Vec<SharedString> = st.version_rows.iter().map(|v| format!("v{v}").into()).collect();
            app.set_versions(ModelRc::new(VecModel::from(model)));
            app.set_selected_version(if st.version_rows.is_empty() { -1 } else { 0 });
            refresh_loaded(&app, &mut st);
        }
    });

    app.on_version_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |_index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            refresh_loaded(&app, &mut st);
        }
    });

    app.on_save_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |_index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            refresh_loaded(&app, &mut st);
        }
    });

    app.on_folder_grouped_toggled({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |grouped| {
            let Some(app) = app_weak.upgrade() else { return };
            // Only the folder model depends on grouping — no rebake.
            let st = state.borrow();
            if let Some(l) = st.loaded.as_ref() {
                app.set_folder_chips(ModelRc::new(VecModel::from(loaded::folder_rows(l, grouped))));
            }
        }
    });

    app.on_replay_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some(replay) = st
                .replay_filtered
                .get(index as usize)
                .and_then(|&ri| st.replay_rows.get(ri))
            else {
                return;
            };
            let md = &replay.metadata;
            let lang = &st.config.language;

            let mut lines: Vec<SharedString> = Vec::new();
            lines.push(replays::format_rel_path(&st.config.replays_path(), &replay.path).into());
            lines.push(replays::format_ts(md.ts, "%Y-%m-%d %H:%M:%S %z").into());
            if let Some(gi) = md.local_side.as_ref().and_then(|s| s.game_info.as_ref()) {
                let game_name = u8::try_from(gi.rom_variant)
                    .ok()
                    .and_then(|v| game::find_by_family_and_variant(&gi.rom_family, v))
                    .map(|g| game::display_name(lang, g))
                    .unwrap_or_else(|| gi.rom_family.clone());
                let game_line = match &gi.patch {
                    Some(patch) => format!("{} + {} v{}", game_name, patch.name, patch.version),
                    None => game_name,
                };
                lines.push(game_line.into());
                lines.push(
                    game::match_type_name(lang, &gi.rom_family, md.match_type as u8, md.match_subtype as u8).into(),
                );
            }
            let local = md.local_side.as_ref().map(|s| s.nickname.as_str()).unwrap_or("?");
            let remote = md.remote_side.as_ref().map(|s| s.nickname.as_str()).unwrap_or("?");
            lines.push(format!("{local} vs {remote}").into());

            app.set_replay_title(
                replay
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
                    .into(),
            );
            app.set_replay_detail(ModelRc::new(VecModel::from(lines.clone())));

            // Stats need a full decode — compute off-thread, folded into
            // the detail pane when they land (if still selected).
            let path = replay.path.clone();
            st.replay_detail_path = Some(path.clone());
            st.replay_detail_lines = lines;
            let tx = stats_tx.clone();
            std::thread::spawn(move || match replays::compute_stats(&path) {
                Ok(stats) => {
                    let _ = tx.send(Event::ReplayStats { path, stats });
                }
                Err(e) => log::warn!("{}: stats failed: {e}", path.display()),
            });
        }
    });

    app.on_replay_filter_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let family = if index <= 0 {
                None
            } else {
                st.replay_filter_families.get(index as usize - 1).cloned()
            };
            apply_replay_filter(&app, &mut st, family.as_deref());
        }
    });

    app.on_replay_opponent_edited({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |text| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.replay_filter_opponent = text.trim().to_lowercase();
            let index = app.get_selected_replay_filter();
            let family = if index <= 0 {
                None
            } else {
                st.replay_filter_families.get(index as usize - 1).cloned()
            };
            apply_replay_filter(&app, &mut st, family.as_deref());
        }
    });

    app.on_play_clicked({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let game_index = app.get_selected_game();
            let save_index = app.get_selected_save();
            let Some(game) = st.game_rows.get(game_index as usize).copied() else {
                return;
            };
            let Some(save) = st.save_rows.get(save_index as usize).cloned() else {
                return;
            };
            let Some(rom) = st.roms.get(&game) else {
                return;
            };
            // Apply the selected patch version, if any.
            let patch_index = app.get_selected_patch();
            let rom = if patch_index > 0 {
                let patched = st
                    .patch_rows
                    .get(patch_index as usize - 1)
                    .zip(st.version_rows.get(app.get_selected_version() as usize))
                    .ok_or_else(|| anyhow::anyhow!("no patch version selected"))
                    .and_then(|(name, version)| {
                        patch::apply_patch_from_disk(rom, game, &st.config.patches_path(), name, version)
                    });
                match patched {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("failed to apply patch: {e:?}");
                        app.set_status(format!("Failed to apply patch: {e}").into());
                        return;
                    }
                }
            } else {
                rom.clone()
            };
            match session::SinglePlayerSession::new(&rom, &save.path, &st.audio_binder) {
                Ok(session) => {
                    st.session = Some(ActiveSession::SinglePlayer(session));
                    st.joyflags = 0;
                    app.set_session_kind(0);
                    app.set_in_session(true);
                }
                Err(e) => {
                    log::error!("failed to start session: {e:?}");
                    app.set_status(format!("Failed to start: {e}").into());
                }
            }
        }
    });

    app.on_fight_clicked({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let text = app.get_link_code().trim().to_string();
            if text.is_empty() {
                return;
            }
            if text.starts_with('/') {
                // Direct link-code commands: /host [port] | /connect <addr>.
                match parse_direct_command(&text) {
                    Some(role) => st.netplay.update(netplay::Message::ConnectDirect { role }),
                    None => {
                        app.set_status(format!("Unrecognized command: {text}").into());
                        return;
                    }
                }
            } else {
                // Matchmaking link code, normalized the way tango's
                // input filter does as-you-type (tabs/play/mod.rs
                // LinkCodeChanged): ascii alphanumerics + '-' only,
                // lowercased (matchmaking is case-sensitive; this keeps
                // a code read aloud from missing its lobby), 100 max.
                let link_code: String = text
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .map(|c| c.to_ascii_lowercase())
                    .take(100)
                    .collect();
                if link_code.is_empty() {
                    app.set_status(format!("Not a valid link code: {text}").into());
                    return;
                }
                let msg = netplay::Message::Connect {
                    link_code,
                    endpoint: st.config.matchmaking_endpoint.clone(),
                    use_relay: st.config.relay_mode.use_relay(),
                    identity: st.identity.clone(),
                };
                st.netplay.update(msg);
            }
            // Connect wiped the lobby state — apply the default
            // match-type policy now so the picker reads right from the
            // first frame (tango app/update.rs:44-61).
            if let Some(game) = selected_game(&app, &st) {
                apply_default_match_type(&mut st.netplay, game);
            }
            refresh_lobby_ui(&app, &mut st);
        }
    });

    app.on_lobby_leave({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.netplay.update(netplay::Message::Disconnect);
            refresh_lobby_ui(&app, &mut st);
        }
    });

    app.on_lobby_ready({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            // Defense in depth behind the view-time gating (the button
            // disables itself): Lobby phase, a compatible verdict, and
            // a selected save are all required to commit.
            if !matches!(st.netplay.phase, netplay::Phase::Lobby { .. }) || st.netplay.handoff_pending() {
                return;
            }
            if !matches!(lobby_verdict(&st), Some(netplay::compat::Verdict::Compatible)) {
                return;
            }
            let Some(save_sram) = st
                .save_rows
                .get(app.get_selected_save() as usize)
                .map(|s| s.save.to_sram_dump())
            else {
                return;
            };
            st.netplay.update(netplay::Message::Commit { save_sram });
            refresh_lobby_ui(&app, &mut st);
        }
    });

    app.on_lobby_unready({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.netplay.update(netplay::Message::Uncommit);
            refresh_lobby_ui(&app, &mut st);
        }
    });

    app.on_match_type_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some(&mt) = st.lobby_mt_rows.get(index as usize) else {
                return;
            };
            st.netplay.update(netplay::Message::SetMatchType(mt));
            // Stamp the default-MT slot so the per-tick default pass
            // doesn't clobber an explicit pre-default pick (tango
            // app/update.rs:85-90).
            if let Some(game) = selected_game(&app, &st) {
                let (family, variant) = game.family_and_variant();
                st.netplay.lobby.default_mt_for_game = Some((family.to_string(), variant));
            }
            resend_settings_if_lobby(&app, &mut st);
            refresh_lobby_ui(&app, &mut st);
        }
    });

    app.on_frame_delay_changed({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |value| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            const MIN: u32 = tango_pvp::battle::MIN_FRAME_DELAY;
            const MAX: u32 = tango_pvp::battle::MAX_FRAME_DELAY;
            let frames = ((MIN as f32 + value.clamp(0.0, 1.0) * (MAX - MIN) as f32).round() as u32).clamp(MIN, MAX);
            // Persisted; it's this side's local frame delay
            // (snapshotted into the match at start, never negotiated),
            // so there's no live match to push it to here.
            if st.config.frame_delay != frames {
                st.config.frame_delay = frames;
                st.config.save();
            }
            // Snap the slider onto the frame it resolved to (a raw drag
            // value can rest between detents otherwise) + the readout.
            let norm = (frames - MIN) as f32 / (MAX - MIN) as f32;
            app.set_lobby_frame_delay(norm);
            app.set_lobby_frame_delay_label(frames.to_string().into());
            st.lobby_ui.frame_delay_norm = norm;
            st.lobby_ui.frame_delay_label = frames.to_string();
        }
    });

    app.on_suggest_delay({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            // Gated on the first Pong having landed; reads the median
            // window rather than the raw latest so the recommendation
            // doesn't chase a single spiky Pong (tango lobby.rs:434).
            if st.netplay.lobby.latency_counter.latest().is_none() {
                return;
            }
            let frames = tango_pvp::battle::suggest_frame_delay(st.netplay.lobby.latency_counter.median());
            if st.config.frame_delay != frames {
                st.config.frame_delay = frames;
                st.config.save();
            }
            refresh_lobby_ui(&app, &mut st);
        }
    });

    app.on_blind_toggled({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |blind| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.netplay.update(netplay::Message::SetBlindSetup(blind));
            // The flag rides the Settings packet — resend so the peer
            // sees it (flipping it on also drops their commit).
            resend_settings_if_lobby(&app, &mut st);
            refresh_lobby_ui(&app, &mut st);
        }
    });

    app.on_watch_clicked({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some(scanned) = st
                .replay_filtered
                .get(app.get_selected_replay() as usize)
                .and_then(|&ri| st.replay_rows.get(ri))
            else {
                return;
            };
            let start = start_replay(
                &scanned.path,
                &st.roms,
                &st.config.patches_path(),
                &st.audio_binder,
            );
            match start {
                Ok(session) => {
                    let marks: Vec<f32> = session
                        .round_boundaries()
                        .iter()
                        .map(|b| *b as f32 / session.total_ticks().max(1) as f32)
                        .collect();
                    app.set_replay_marks(ModelRc::new(VecModel::from(marks)));
                    st.session = Some(ActiveSession::Replay(session));
                    st.replay_speed = 1.0;
                    app.set_replay_speed(1);
                    app.set_replay_paused(false);
                    app.set_session_kind(1);
                    app.set_in_session(true);
                }
                Err(e) => {
                    log::error!("failed to start replay: {e:?}");
                    app.set_status(format!("Failed to start replay: {e}").into());
                }
            }
        }
    });

    app.on_toggle_pause({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
            if let Some(ActiveSession::Replay(session)) = &st.session {
                // Play at the end = rewind to the start and play.
                if session.is_complete() && session.is_paused() {
                    session.seek_to(0, true);
                    app.set_replay_paused(false);
                    return;
                }
                let paused = !session.is_paused();
                session.set_paused(paused);
                app.set_replay_paused(paused);
            }
        }
    });

    app.on_scrub_started({
        let state = state.clone();
        move || {
            let mut st = state.borrow_mut();
            let Some(ActiveSession::Replay(session)) = &st.session else {
                return;
            };
            let was_playing = (!session.is_paused() || session.seek_will_resume()) && !session.is_complete();
            session.set_paused(true);
            st.scrub_was_playing = was_playing;
            st.scrub_forced = false;
        }
    });

    app.on_scrub_moved({
        let state = state.clone();
        move |fraction| {
            let mut st = state.borrow_mut();
            let Some(ActiveSession::Replay(session)) = &st.session else {
                return;
            };
            let target = (fraction.clamp(0.0, 1.0) * session.total_ticks() as f32) as u32;
            let forced = st.scrub_forced;
            if session.scrub_preview(target, forced) {
                st.scrub_forced = true;
            }
        }
    });

    app.on_scrub_committed({
        let state = state.clone();
        move |fraction| {
            let mut st = state.borrow_mut();
            let resume = st.scrub_was_playing;
            st.scrub_was_playing = false;
            st.scrub_forced = false;
            let Some(ActiveSession::Replay(session)) = &st.session else {
                return;
            };
            let target = (fraction.clamp(0.0, 1.0) * session.total_ticks() as f32) as u32;
            session.seek_to(target, resume);
        }
    });

    app.on_speed_selected({
        let state = state.clone();
        move |index| {
            let mut st = state.borrow_mut();
            st.replay_speed = [0.5, 1.0, 2.0, 4.0].get(index as usize).copied().unwrap_or(1.0);
            if let Some(ActiveSession::Replay(session)) = &st.session {
                session.set_speed(st.replay_speed);
            }
        }
    });

    let end_session = {
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            // PvP: cancel the netcode before the drop so the match-run
            // task sends the peer its Closing marker (Drop also cancels
            // — this just makes the ordering explicit, mirroring tango's
            // session Close handler).
            if let Some(ActiveSession::Pvp(p)) = &st.session {
                p.request_close();
            }
            st.session = None;
            st.joyflags = 0;
            app.set_in_session(false);
            app.set_frame(Image::default());
        }
    };

    app.on_stop_clicked(end_session.clone());

    app.on_key_event({
        let state = state.clone();
        let app_weak = app.as_weak();
        let end_session = end_session.clone();
        move |text, pressed| {
            let mut st = state.borrow_mut();
            match input::classify(text.as_str()) {
                Some(input::KeyAction::Joyflag(flag)) => {
                    // PvP takes live input exactly like singleplayer — the
                    // netcode reads the joyflag atomic on the primary trap.
                    if matches!(
                        &st.session,
                        Some(ActiveSession::SinglePlayer(_) | ActiveSession::Pvp(_))
                    ) {
                        if pressed {
                            st.joyflags |= flag;
                        } else {
                            st.joyflags &= !flag;
                        }
                        match &st.session {
                            Some(ActiveSession::SinglePlayer(session)) => session.set_joyflags(st.joyflags),
                            Some(ActiveSession::Pvp(session)) => session.set_joyflags(st.joyflags),
                            _ => {}
                        }
                    } else if let Some(ActiveSession::Replay(session)) = &st.session {
                        // Replays take no input; space doubles as pause toggle
                        // (and play-at-end = rewind to the start).
                        if flag == mgba::input::keys::SELECT && pressed {
                            let paused = if session.is_complete() && session.is_paused() {
                                session.seek_to(0, true);
                                false
                            } else {
                                let paused = !session.is_paused();
                                session.set_paused(paused);
                                paused
                            };
                            if let Some(app) = app_weak.upgrade() {
                                app.set_replay_paused(paused);
                            }
                        }
                    }
                }
                Some(input::KeyAction::FastForward) => match &st.session {
                    Some(ActiveSession::SinglePlayer(session)) => {
                        session.set_speed(if pressed { 3.0 } else { 1.0 });
                    }
                    Some(ActiveSession::Replay(session)) => {
                        let speed = if pressed { 4.0 } else { st.replay_speed };
                        session.set_speed(speed);
                    }
                    // PvP is throttled by the netcode's clock-sync — no
                    // fast-forward.
                    Some(ActiveSession::Pvp(_)) | None => {}
                },
                Some(input::KeyAction::EndSession) => {
                    if pressed {
                        drop(st);
                        end_session();
                    }
                }
                None => {}
            }
        }
    });

    // Frame pump + event fold, ~60 Hz. Cheap when idle: a try_recv and a
    // dirty-flag check.
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), {
        let state = state.clone();
        let app_weak = app.as_weak();
        let rt = tokio_runtime.handle().clone();
        let end_session = end_session.clone();
        move || {
            let Some(app) = app_weak.upgrade() else { return };

            while let Ok(event) = rx.try_recv() {
                match event {
                    Event::ScanDone {
                        roms,
                        saves,
                        replays,
                        patches,
                    } => {
                        let mut st = state.borrow_mut();
                        st.roms = roms;
                        st.saves = saves;
                        st.replay_rows = replays;
                        st.patches = patches;
                        refresh_models(&app, &mut st);
                    }
                    Event::ReplayStats { path, stats } => {
                        let st = state.borrow();
                        if st.replay_detail_path.as_deref() == Some(path.as_path()) {
                            let lang = &st.config.language;
                            let mut lines = st.replay_detail_lines.clone();
                            lines.push(
                                format!(
                                    "{} · {}{}",
                                    t!(lang, "replays-round-count", count = stats.round_count),
                                    replays::format_duration(stats.tick_count),
                                    if stats.is_complete {
                                        String::new()
                                    } else {
                                        format!(" · {}", t!(lang, "replays-incomplete"))
                                    }
                                )
                                .into(),
                            );
                            app.set_replay_detail(ModelRc::new(VecModel::from(lines)));
                        }
                    }
                    Event::Netplay(msg) => {
                        // Forwarded into the netplay state machine, except
                        // MatchHandoffReady, which the app layer handles
                        // itself (drain + spawn_pvp — mirrors tango's App).
                        // update() may spawn tasks / send further Events
                        // (drained next tick) but never re-borrows this
                        // RefCell — netplay::State is self-contained. The
                        // lobby band refresh runs after the drain, below.
                        let mut st = state.borrow_mut();
                        if matches!(msg, netplay::Message::MatchHandoffReady) {
                            start_pvp_handoff(&app, &mut st, &rt, &pvp_tx);
                        } else {
                            st.netplay.update(msg);
                        }
                    }
                    Event::PvpBuilt(result) => {
                        let mut st = state.borrow_mut();
                        match *result {
                            Ok(session) => {
                                st.session = Some(ActiveSession::Pvp(Box::new(session)));
                                st.joyflags = 0;
                                app.set_session_kind(2);
                                app.set_in_session(true);
                            }
                            Err(e) => {
                                log::error!("pvp session build failed: {e:?}");
                                app.set_status(format!("Failed to start match: {e}").into());
                            }
                        }
                        // Clear the post-handoff lobby snapshot now that the
                        // PvP view (or the error) has taken over — tango does
                        // the same at app/mod.rs:1159-1164.
                        st.netplay.finish_handoff();
                    }
                }
            }

            // Lobby upkeep, once per tick: advertise the current
            // selection while in the lobby (the dedupe inside
            // SendLocalSettings makes repeats no-ops — this is how both
            // lobby entry and mid-lobby selection changes reach the
            // peer, tango's resend pass), then push the lobby band's
            // properties (diffed, so idle ticks are free).
            {
                let mut st = state.borrow_mut();
                resend_settings_if_lobby(&app, &mut st);
                refresh_lobby_ui(&app, &mut st);
            }

            let st = state.borrow();
            let mut pvp_ended = false;
            if let Some(session) = &st.session {
                if session.frame_dirty() {
                    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(
                        session::SCREEN_WIDTH,
                        session::SCREEN_HEIGHT,
                    );
                    session.read_frame(pixels.make_mut_bytes());
                    app.set_frame(Image::from_rgba8(pixels));
                }
                if let ActiveSession::Pvp(p) = session {
                    // Self-close once the match has wound down (completion +
                    // peer EndOfMatch / disconnect / grace — see
                    // PvpSession::is_ended); actual teardown happens below,
                    // after this borrow drops.
                    pvp_ended = p.is_ended();
                    let mut line = format!("PvP · P{} · {:.1} tps", p.local_player_index() + 1, p.tps());
                    if let Some(l) = p.latency_raw() {
                        line += &format!(" · {} ms", l.as_millis());
                    }
                    if let Some(rs) = p.round_stats() {
                        line += &format!(" · skew {:+} · lead {:+}", rs.skew, rs.lead);
                    }
                    app.set_status(line.into());
                }
                if let ActiveSession::Replay(replay) = session {
                    // Draw the playhead where an in-flight seek is headed
                    // instead of snapping back until the chase lands.
                    let tick = replay.pending_seek_target().unwrap_or_else(|| replay.current_tick());
                    app.set_replay_progress(
                        format!(
                            "{} / {}{}",
                            tick,
                            replay.total_ticks(),
                            if replay.is_complete() { " · end" } else { "" }
                        )
                        .into(),
                    );
                    app.set_replay_scrub_pos(tick as f32 / replay.total_ticks().max(1) as f32);
                    app.set_replay_prefetch(
                        replay.prefetch_progress() as f32 / replay.total_ticks().max(1) as f32,
                    );
                    // A paused thread mid-chase is logically still playing.
                    app.set_replay_paused(replay.is_paused() && !replay.seek_will_resume());
                }
            }
            drop(st);

            if pvp_ended {
                end_session();
            }

            // --ui-shot: once the scan is folded in, walk the main
            // screens, snapshotting each, then quit.
            if let Some(dir) = &ui_shot_dir {
                if state.borrow().game_rows.is_empty() {
                    return;
                }
                let step = {
                    let mut s = shot_step.borrow_mut();
                    *s += 1;
                    *s
                };
                // The BN6 game row (buster stats + roster emblem in
                // the save viewer), when a BN6 rom + save are both
                // scanned. Recomputed per step — cheap and keeps the
                // walker stateless.
                let bn6_index = || {
                    let st = state.borrow();
                    let lang = st.config.language.clone();
                    st.game_rows.iter().position(|g| {
                        game::display_name(&lang, *g).contains("Cybeast")
                            && st.saves.get(g).is_some_and(|s| !s.is_empty())
                    })
                };
                // A few ticks between shots so layout/render settles.
                match step {
                    10 => snapshot(&app, &dir.join("ui-play-empty.png")),
                    20 => {
                        app.set_selected_game(0);
                        app.invoke_game_selected(0);
                        app.set_selected_save(0);
                        app.invoke_save_selected(0);
                    }
                    30 => snapshot(&app, &dir.join("ui-play-selected.png")),
                    // Bring up the lobby band via the direct /host path
                    // (no matchmaking server involved): the band shows
                    // the waiting status + matchup/command panes.
                    32 => {
                        app.set_link_code("/host".into());
                        app.invoke_fight_clicked();
                    }
                    47 => {
                        snapshot(&app, &dir.join("ui-lobby.png"));
                        app.invoke_lobby_leave();
                    }
                    // Save viewer on a BN6 save: the navi header must
                    // show the roster emblem + the buster stat row.
                    50 => {
                        if let Some(idx) = bn6_index() {
                            app.set_selected_game(idx as i32);
                            app.invoke_game_selected(idx as i32);
                            app.set_selected_save(0);
                            app.invoke_save_selected(0);
                        }
                    }
                    60 => {
                        if bn6_index().is_some() {
                            snapshot(&app, &dir.join("ui-save-bn6.png"));
                        }
                    }
                    70 => app.set_active_tab(1),
                    75 => {
                        if !state.borrow().replay_rows.is_empty() {
                            app.set_selected_replay(0);
                            app.invoke_replay_selected(0);
                        }
                    }
                    85 => snapshot(&app, &dir.join("ui-replays.png")),
                    95 => app.set_active_tab(3),
                    105 => {
                        snapshot(&app, &dir.join("ui-settings.png"));
                        let _ = slint::quit_event_loop();
                    }
                    _ => {}
                }
            }
        }
    });

    app.run()?;
    Ok(())
}

/// Headless replay-playback verification: boot the newest replay that
/// resolves, watch the tick counter advance for ~8 seconds, dump the
/// framebuffer as a PNG.
fn smoke_replay(config: &config::Config, audio_binder: &audio::LateBinder, out: &std::path::Path) -> anyhow::Result<()> {
    let roms = rom::scan_roms(&config.roms_path());
    let scanned = replays::scan_replays(&config.replays_path());
    println!("smoke-replay: {} roms, {} replays", roms.len(), scanned.len());

    let patches_path = config.patches_path();
    let mut session = None;
    for candidate in scanned.iter().take(20) {
        match start_replay(&candidate.path, &roms, &patches_path, audio_binder) {
            Ok(s) => {
                println!("smoke-replay: playing {}", candidate.path.display());
                session = Some(s);
                break;
            }
            Err(e) => {
                println!("smoke-replay: skipping {}: {e}", candidate.path.display());
            }
        }
    }
    let session = session.ok_or_else(|| anyhow::anyhow!("no playable replay found"))?;

    let mut last_tick = 0;
    for second in 1..=8 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let tick = session.current_tick();
        println!(
            "smoke-replay: t+{second}s tick {tick} / {}{}",
            session.total_ticks(),
            if session.is_complete() { " (complete)" } else { "" }
        );
        anyhow::ensure!(
            tick > last_tick || session.is_complete(),
            "playback stalled at tick {tick}"
        );
        last_tick = tick;
        if session.is_complete() {
            break;
        }
    }

    // Exercise the seek subsystem: jump back near (not at) the end,
    // paused — late enough that the frame is known to have content
    // (early ticks sit in the round-start white fade).
    let mid = session.total_ticks().saturating_sub(10);
    session.seek_to(mid, false);
    std::thread::sleep(std::time::Duration::from_secs(3));
    let landed = session.current_tick();
    println!("smoke-replay: seek to {mid} landed at {landed}");
    anyhow::ensure!(
        landed.abs_diff(mid) <= 2,
        "seek missed: wanted {mid}, landed {landed}"
    );

    let mut rgba = vec![0u8; session::SCREEN_WIDTH as usize * session::SCREEN_HEIGHT as usize * 4];
    session.read_frame(&mut rgba);
    let img = image::RgbaImage::from_raw(session::SCREEN_WIDTH, session::SCREEN_HEIGHT, rgba)
        .ok_or_else(|| anyhow::anyhow!("bad framebuffer size"))?;
    img.save(out)?;
    println!("smoke-replay: wrote {}", out.display());
    Ok(())
}

/// Headless two-instance PvP verification: run once as `--smoke-pvp
/// host <out.png>` and once (concurrently) as `--smoke-pvp connect
/// <out.png>` against the same data dir — or run BOTH sides as
/// `--smoke-pvp match <link_code> <out.png>` to rendezvous through the
/// real matchmaking server instead of the direct path (give each
/// instance its own `TANGO_IDENTITY_DIR` so they present distinct
/// client certs). Each side scans, picks the first game with both a
/// rom and a save (the same pick as `--smoke`, so the two sides
/// agree), drives the netplay state machine through connection
/// bring-up and the auto-lobby to a live `PvpSession`, then watches
/// ~8 seconds of emulation before dumping the framebuffer. This is
/// the GUI timer's event fold, minus slint.
fn smoke_pvp(
    config: &config::Config,
    audio_binder: &audio::LateBinder,
    rt: tokio::runtime::Handle,
    role_arg: &str,
    link_code: Option<&str>,
    out: &std::path::Path,
) -> anyhow::Result<()> {
    let tag = format!("smoke-pvp[{role_arg}]");
    let roms = rom::scan_roms(&config.roms_path());
    let saves = save::scan_saves(&config.saves_path());
    let patches = patch::scan(&config.patches_path());
    println!(
        "{tag}: {} roms, {} saves",
        roms.len(),
        saves.values().map(|v| v.len()).sum::<usize>()
    );

    let mut candidates: Vec<rom::GameRef> = roms
        .keys()
        .copied()
        .filter(|g| saves.get(g).is_some_and(|s| !s.is_empty()))
        .collect();
    candidates.sort_by_key(|g| game::display_name(&game::FALLBACK_LANG, *g));
    let game = *candidates
        .first()
        .ok_or_else(|| anyhow::anyhow!("no game with both a rom and a save"))?;
    let save = &saves[&game][0];
    println!(
        "{tag}: {} with {}",
        game::display_name(&game::FALLBACK_LANG, game),
        save.path.display()
    );

    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    let mut netplay = netplay::State::new(rt.clone(), tx.clone());
    let msg = match role_arg {
        "host" => netplay::Message::ConnectDirect {
            role: netplay::DirectRole::Host {
                port: net::DEFAULT_LOCAL_PORT,
            },
        },
        "connect" => netplay::Message::ConnectDirect {
            role: netplay::DirectRole::Connect {
                addr: format!("127.0.0.1:{}", net::DEFAULT_LOCAL_PORT),
            },
        },
        // The real matchmaking path: both instances dial the configured
        // endpoint with the same link code and rendezvous there, exactly
        // like the GUI's fight_clicked (identity included — the driver
        // points TANGO_IDENTITY_DIR at per-instance dirs).
        "match" => netplay::Message::Connect {
            link_code: link_code
                .ok_or_else(|| anyhow::anyhow!("--smoke-pvp match needs a <link_code>"))?
                .to_string(),
            endpoint: config.matchmaking_endpoint.clone(),
            use_relay: config.relay_mode.use_relay(),
            identity: identity::load(),
        },
        other => anyhow::bail!("--smoke-pvp needs `host`, `connect`, or `match`, got {other:?}"),
    };
    netplay.update(msg);

    // Drive the state machine by hand until the session is built —
    // exactly what the GUI timer's Event::Netplay arm does, sharing
    // `drive_auto_lobby`. The build itself runs on the runtime and hands
    // the session back over a channel: constructing it under a block_on
    // here would leave this thread's runtime context visible to nothing
    // useful, and the emulator thread must never see one at all
    // (PvpSender::send uses blocking_send).
    let nickname = format!("smoke-{role_arg}");
    let (session_tx, session_rx) = std::sync::mpsc::channel::<anyhow::Result<pvp::PvpSession>>();
    let mut spawned = false;
    let mut last_status = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    let session = loop {
        if let Ok(built) = session_rx.try_recv() {
            break built?;
        }
        if let netplay::Phase::Failed { error } = &netplay.phase {
            anyhow::bail!("netplay failed: {error}");
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "setup timed out ({})",
            if last_status.is_empty() { "idle" } else { &last_status }
        );
        match rx.recv_timeout(std::time::Duration::from_millis(16)) {
            Ok(Event::Netplay(msg)) => {
                if matches!(msg, netplay::Message::MatchHandoffReady) {
                    if spawned {
                        continue;
                    }
                    let Some(pre_match) = netplay.take_pre_match() else {
                        continue;
                    };
                    println!("{tag}: handoff ready, building session");
                    let resolved =
                        pvp::resolve_pvp_roms(&roms, &config.patches_path(), game, None, &pre_match.remote_settings)?;
                    let session_tx = session_tx.clone();
                    let frame_delay = config.frame_delay;
                    let disable_bgm = config.disable_bgm_in_pvp;
                    // Smoke matches must not pollute the real replay
                    // library — record their replays to the temp dir.
                    let replays_path = std::env::temp_dir().join("tango-ng-smoke-replays");
                    let audio_binder = audio_binder.clone();
                    rt.spawn(async move {
                        let _ = session_tx.send(
                            pvp::spawn_pvp(resolved, pre_match, frame_delay, disable_bgm, replays_path, audio_binder)
                                .await,
                        );
                    });
                    spawned = true;
                    netplay.finish_handoff();
                } else {
                    netplay.update(msg);
                    drive_auto_lobby(&mut netplay, &patches, &nickname, game, None, save);
                }
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => anyhow::bail!("event channel closed"),
        }
        let status = if spawned {
            "starting match…".to_string()
        } else {
            netplay_status_text(&netplay)
        };
        if status != last_status && !status.is_empty() {
            println!("{tag}: {status}");
            last_status = status;
        }
    };
    println!("{tag}: session up — playing as P{}", session.local_player_index() + 1);

    // ~8 s of live emulation: frames must keep landing and the match
    // must not end (nobody plays a round, so `is_ended`'s completion
    // gate must hold false throughout).
    for second in 1..=8 {
        let mut new_frames = 0u32;
        let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while std::time::Instant::now() < until {
            // Keep the event channel drained; nothing needs folding
            // post-handoff.
            match rx.recv_timeout(std::time::Duration::from_millis(16)) {
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if session.frame_dirty() {
                new_frames += 1;
            }
        }
        let latency = session
            .latency_raw()
            .map(|d| format!("{} ms", d.as_millis()))
            .unwrap_or_else(|| "—".to_string());
        println!(
            "{tag}: t+{second}s {new_frames} new frames · {:.1} tps · latency {latency}",
            session.tps()
        );
        anyhow::ensure!(new_frames > 0, "emulator stalled (no new frames in second {second})");
        anyhow::ensure!(!session.is_ended(), "session ended prematurely");
    }

    let mut rgba = vec![0u8; session::SCREEN_WIDTH as usize * session::SCREEN_HEIGHT as usize * 4];
    session.read_frame(&mut rgba);
    let img = image::RgbaImage::from_raw(session::SCREEN_WIDTH, session::SCREEN_HEIGHT, rgba)
        .ok_or_else(|| anyhow::anyhow!("bad framebuffer size"))?;
    img.save(out)?;
    println!("{tag}: wrote {}", out.display());
    session.request_close();
    // Orderly teardown: the mgba thread ends+joins when the *last*
    // handle to it drops, and the match-run task / Match hold handles
    // until they observe the cancel. Give them a moment while the
    // runtime is still healthy — returning immediately races process
    // exit against a still-running CPU thread (observed as a SIGSEGV in
    // ARMRunLoop during teardown).
    drop(session);
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(())
}

/// Save a `--ui-shot` snapshot of the window to `path`.
fn snapshot(app: &AppWindow, path: &std::path::Path) {
    let buf = match app.window().take_snapshot() {
        Ok(buf) => buf,
        Err(e) => {
            log::error!("take_snapshot: {e}");
            return;
        }
    };
    let Some(img) = image::RgbaImage::from_raw(buf.width(), buf.height(), buf.as_bytes().to_vec()) else {
        log::error!("snapshot: bad buffer");
        return;
    };
    if let Err(e) = img.save(path) {
        log::error!("snapshot: {}: {e}", path.display());
    } else {
        println!("ui-shot: wrote {}", path.display());
    }
}

/// Headless verification: boot the first (alphabetical) game that has a
/// save, emulate ~5 real seconds, dump the framebuffer as a PNG.
fn smoke(config: &config::Config, audio_binder: &audio::LateBinder, out: &std::path::Path) -> anyhow::Result<()> {
    let roms = rom::scan_roms(&config.roms_path());
    let saves = save::scan_saves(&config.saves_path());
    println!(
        "smoke: {} roms, {} saves",
        roms.len(),
        saves.values().map(|v| v.len()).sum::<usize>()
    );

    let mut candidates: Vec<rom::GameRef> = roms
        .keys()
        .copied()
        .filter(|g| saves.get(g).is_some_and(|s| !s.is_empty()))
        .collect();
    candidates.sort_by_key(|g| game::display_name(&game::FALLBACK_LANG, *g));
    let game = *candidates
        .first()
        .ok_or_else(|| anyhow::anyhow!("no game with both a rom and a save"))?;
    let save = &saves[&game][0];
    println!(
        "smoke: booting {} with {}",
        game::display_name(&game::FALLBACK_LANG, game),
        save.path.display()
    );

    // Run against a copy so smoke never touches the real save.
    let tmp_save = std::env::temp_dir().join("tango-ng-smoke.sav");
    std::fs::copy(&save.path, &tmp_save)?;

    let session = session::SinglePlayerSession::new(&roms[&game], &tmp_save, audio_binder)?;
    std::thread::sleep(std::time::Duration::from_secs(5));

    let mut rgba = vec![0u8; session::SCREEN_WIDTH as usize * session::SCREEN_HEIGHT as usize * 4];
    session.read_frame(&mut rgba);
    let img = image::RgbaImage::from_raw(session::SCREEN_WIDTH, session::SCREEN_HEIGHT, rgba)
        .ok_or_else(|| anyhow::anyhow!("bad framebuffer size"))?;
    img.save(out)?;
    println!("smoke: wrote {}", out.display());
    Ok(())
}
