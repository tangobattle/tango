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
// Discord rich presence — desktop-only (no mobile Discord IPC).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod discord;
// Self-updater — desktop-only (mobile updates through the store).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod updater;
// Crash logging (panic hook + native crash handler) — desktop-only,
// paired with the supervisor trampoline below.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod crash_log;
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
// The read-only NaviCust grid raster; the editor's live-grid surface
// (ghosts, drop targets) waits on the save-editor port.
#[allow(dead_code)]
mod navicust;
mod patch;
// PvP session. The in-session HUD consumes the telemetry + reconnect
// surface; dead_code remains allowed for the corners the UI still
// doesn't read (raw latency, parts of the reconnect plumbing).
#[allow(dead_code)]
mod pvp;
mod randomcode;
mod replays;
mod rom;
mod save;
mod video;
mod save_manage;
mod session;

slint::include_modules!();

/// Android entry point (the android-activity contract, invoked by the
/// APK's NativeActivity glue): hand the activity to Slint's android
/// backend, then run the same app the desktop `main` runs. The rest of
/// the mobile story — APK packaging + the mgba NDK toolchain — rides
/// on top of this.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: slint::android::AndroidApp) {
    if let Err(e) = slint::android::init(app) {
        log::error!("slint android init failed: {e}");
        return;
    }
    if let Err(e) = run() {
        log::error!("tango-ng exited with error: {e:?}");
    }
}

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
    /// A patch-repo sync finished — from the Patches tab's Update
    /// button or the background autoupdater. Success triggers a
    /// rescan; `background` results never touch the tab's status
    /// line beyond the last-updated stamp (autoupdate errors only
    /// log, like tango's Autoupdater).
    PatchUpdateDone {
        result: Result<(), String>,
        background: bool,
    },
    /// Netplay task results + lobby-loop observations, forwarded into
    /// `netplay::State::update` by the timer below (which intercepts
    /// `MatchHandoffReady` itself, mirroring tango's App).
    Netplay(netplay::Message),
    /// A replay's decoded local-side SRAM, for the embedded save view
    /// (decoded off-thread; the image bake runs on the UI thread).
    ReplayLocalSram {
        path: std::path::PathBuf,
        sram: Vec<u8>,
    },
    /// Replay video render progress (frames emitted / total).
    ExportProgress { current: usize, total: usize },
    /// Replay video render finished (the written path) or failed.
    ExportDone { result: Result<std::path::PathBuf, String> },
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
    i18n.set_save_copy(t!(lang, "save-copy").into());
    i18n.set_save_copy_image(t!(lang, "save-copy-image").into());
    i18n.set_copied(t!(lang, "copied").into());
    i18n.set_patch_card4_none(t!(lang, "patch-card4-none").into());
    // save management
    i18n.set_save_open_folder(t!(lang, "save-open-folder").into());
    i18n.set_save_duplicate(t!(lang, "save-duplicate").into());
    i18n.set_save_rename(t!(lang, "save-rename").into());
    i18n.set_save_delete(t!(lang, "save-delete").into());
    i18n.set_save_rename_confirm(t!(lang, "save-rename-confirm").into());
    i18n.set_save_delete_confirm(t!(lang, "save-delete-confirm").into());
    i18n.set_save_new_confirm(t!(lang, "save-new-confirm").into());
    i18n.set_save_action_cancel(t!(lang, "save-action-cancel").into());
    i18n.set_save_name_placeholder(t!(lang, "save-name-placeholder").into());
    i18n.set_save_template_pick(t!(lang, "save-template-pick").into());
    // patches tab polish
    i18n.set_patches_open_folder(t!(lang, "patches-open-folder").into());
    i18n.set_patches_favorite(t!(lang, "patches-favorite").into());
    i18n.set_patches_unfavorite(t!(lang, "patches-unfavorite").into());
    // welcome overlay
    i18n.set_welcome_title(t!(lang, "welcome-title").into());
    i18n.set_welcome_subtitle(t!(lang, "welcome-subtitle").into());
    i18n.set_welcome_continue(t!(lang, "welcome-continue").into());
    i18n.set_welcome_step_roms(t!(lang, "welcome-step-roms").into());
    i18n.set_welcome_step_roms_description(t!(lang, "welcome-step-roms-description").into());
    i18n.set_welcome_step_nickname(t!(lang, "welcome-step-nickname").into());
    i18n.set_welcome_step_nickname_description(t!(lang, "welcome-step-nickname-description").into());
    i18n.set_welcome_open_folder(t!(lang, "welcome-open-folder").into());
    i18n.set_welcome_roms_needed(t!(lang, "welcome-roms-needed").into());
    i18n.set_welcome_rescan(t!(lang, "rescan").into());
    // replay export
    i18n.set_replays_show_incomplete(t!(lang, "replays-show-incomplete").into());
    i18n.set_replays_export(t!(lang, "replays-export").into());
    i18n.set_replays_export_cancel(t!(lang, "replays-export-cancel").into());
    i18n.set_replays_export_open(t!(lang, "replays-export-open").into());
    i18n.set_replays_export_scale(t!(lang, "replays-export-scale").into());
    i18n.set_replays_export_disable_bgm(t!(lang, "replays-export-disable-bgm").into());
    i18n.set_replays_export_twosided(t!(lang, "replays-export-twosided").into());
    // PvP in-session overlays
    i18n.set_pvp_disconnect(t!(lang, "playback-disconnect").into());
    i18n.set_pvp_disconnect_prompt(t!(lang, "playback-disconnect-prompt").into());
    i18n.set_pvp_disconnect_detail(t!(lang, "playback-disconnect-detail").into());
    i18n.set_pvp_reconnecting(t!(lang, "playback-reconnecting").into());
    i18n.set_pvp_reconnecting_detail(t!(lang, "playback-reconnecting-detail").into());
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
    // patches tab
    i18n.set_patches_search_placeholder(t!(lang, "patches-search-placeholder").into());
    i18n.set_patches_select_prompt(t!(lang, "patches-select-prompt").into());
    i18n.set_patches_update(t!(lang, "patches-update").into());
    i18n.set_patches_details_authors(t!(lang, "patches-details-authors").into());
    i18n.set_patches_details_license(t!(lang, "patches-details-license").into());
    i18n.set_patches_details_source(t!(lang, "patches-details-source").into());
    i18n.set_patches_details_games(t!(lang, "patches-details-games").into());
    i18n.set_patches_readme_placeholder(t!(lang, "patches-readme-placeholder").into());
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
    i18n.set_settings_streamer_mode(t!(lang, "settings-streamer-mode").into());
    i18n.set_settings_patch_repo(t!(lang, "settings-patch-repo").into());
    i18n.set_settings_patch_autoupdate(t!(lang, "settings-enable-patch-autoupdate").into());
    i18n.set_settings_fullscreen(t!(lang, "settings-fullscreen").into());
    i18n.set_settings_video_filter(t!(lang, "settings-video-filter").into());
    i18n.set_settings_enable_updater(t!(lang, "settings-enable-updater").into());
    i18n.set_settings_allow_prerelease(t!(lang, "settings-allow-prerelease-upgrades").into());
    i18n.set_updater_update_now(t!(lang, "updater-update-now").into());
    i18n.set_settings_mute_bgm(t!(lang, "settings-disable-bgm-in-pvp").into());
    i18n.set_settings_matchmaking_endpoint(t!(lang, "settings-matchmaking-endpoint").into());
    i18n.set_settings_use_relay(t!(lang, "settings-use-relay").into());
    i18n.set_settings_relay_auto(t!(lang, "settings-use-relay-auto").into());
    i18n.set_settings_relay_always(t!(lang, "settings-use-relay-always").into());
    i18n.set_settings_relay_never(t!(lang, "settings-use-relay-never").into());
    i18n.set_settings_show_opponent_setup(t!(lang, "settings-show-opponent-setup").into());
    i18n.set_settings_fractional(t!(lang, "settings-fractional-scaling").into());
    // input settings (the bezel caption + chip labels are per-key and
    // per-binding — refresh_input_ui pushes those, re-resolved on the
    // next tick after a language change)
    i18n.set_input_select_hint(t!(lang, "settings-input-select-hint").into());
    i18n.set_input_press_key(t!(lang, "settings-input-press-key").into());
    i18n.set_input_add(t!(lang, "settings-input-add").into());
    i18n.set_input_reset(t!(lang, "settings-input-reset").into());
    i18n.set_input_speed_up(t!(lang, "input-key-speed-up").into());
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
    /// The embedded save view for the selected replay's local side,
    /// plus the path it was built for.
    replay_loaded: Option<loaded::Loaded>,
    replay_loaded_path: Option<std::path::PathBuf>,
    /// Which dataset was last pushed into the SaveView global
    /// (0 = play, 1 = replays, 2 = PvP my setup, 3 = PvP opponent) —
    /// the timer re-pushes on tab switches; the session drawers switch
    /// explicitly.
    save_view_source: i32,
    /// Last pointer motion over the session view — the floating
    /// controls hide ~2.5 s after it goes stale.
    session_last_pointer: std::time::Instant,
    /// The live match's setup views, baked at PvpBuilt from the
    /// committed save data (remote stays None when they blinded).
    pvp_local_loaded: Option<loaded::Loaded>,
    pvp_remote_loaded: Option<loaded::Loaded>,
    /// Lazy duration/round/completion stats keyed by replay path,
    /// filled by the background worker after each scan. The
    /// show-incomplete filter reads it; missing entries stay visible
    /// (we only know a replay is incomplete once its stats land).
    replay_stats: HashMap<std::path::PathBuf, replays::ReplayStats>,
    patches: patch::PatchMap,
    /// Patch names shown in the patch picker (model index i+1 — index 0
    /// is the "No patch" sentinel).
    patch_rows: Vec<String>,
    /// Patch names shown in the Patches tab's list, parallel to the
    /// `patch-list` model (the search filter narrows this).
    patch_list_rows: Vec<String>,
    /// Lowercased name/title substring filter for the Patches tab.
    patch_filter: String,
    /// Patch shown in the Patches tab's detail pane.
    patch_detail_name: Option<String>,
    /// The detail pane's version picker rows, newest first.
    patch_detail_versions: Vec<semver::Version>,
    /// Manual patch-repo sync in flight (the Update button disables
    /// itself; a second click is refused).
    patch_updating: bool,
    /// Last manual sync failure, raw error text (rendered localized by
    /// [`refresh_patch_status`]).
    patch_update_error: Option<String>,
    /// When the last successful sync (manual or background) landed.
    patch_last_updated: Option<chrono::DateTime<chrono::Local>>,
    /// Versions shown in the version picker, newest first.
    version_rows: Vec<semver::Version>,
    /// The save viewer's parsed save + ROM assets + baked sprite
    /// images for the current (game, patch, save) selection. Rebuilt
    /// by [`refresh_loaded`]; `None` while nothing is selected.
    loaded: Option<loaded::Loaded>,
    /// The new-save form's template picker values, parallel to the
    /// `save-template-options` model rows.
    save_template_values: Vec<(rom::GameRef, String)>,
    /// The auto-generated name the new-save draft was last set to.
    /// While the user hasn't typed over it, a template pick
    /// regenerates the suggestion; once they edit, it's left alone.
    save_new_auto_default: Option<String>,
    /// Select this (game, save path) once the post-operation rescan
    /// lands — how rename/duplicate/create keep the new file focused.
    pending_select_save: Option<(rom::GameRef, std::path::PathBuf)>,
    /// When the active session started — the rich-presence elapsed
    /// timer's anchor.
    session_start: Option<std::time::SystemTime>,
    /// In-flight replay render: its cancel handle (Cancel button) —
    /// None when idle. The kill also terminates ffmpeg children.
    export_canceller: Option<tango_pvp::replay::export::Canceller>,
    /// The last successful render's output (the Open button).
    export_output: Option<std::path::PathBuf>,
    /// Discord rich-presence client (background auto-reconnect task);
    /// the timer pushes an activity snapshot each tick (deduped inside).
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    discord: discord::Client,
    /// GitHub self-updater (default-off in tango-ng — see config.rs).
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    updater: updater::Updater,
    session: Option<ActiveSession>,
    /// Currently-held physical inputs (keyboard via the FocusScopes'
    /// key events, gamepad via the timer's gilrs poll). Combined with
    /// `config.input_mapping` into session joyflags by
    /// [`sync_session_input`].
    held: input::Held,
    /// Last speed-up (fast-forward) state pushed to the session —
    /// [`sync_session_input`] only touches the session speed on edges.
    speed_up: bool,
    /// Input settings pane: the mapped key whose bindings the LCD
    /// bezel is showing (`None` = the select hint).
    input_selected: Option<input::MappedKey>,
    /// Input settings pane: capture armed for this key — the next
    /// keyboard press / gamepad button / axis crossing binds to it.
    input_capture: Option<input::MappedKey>,
    /// The input pane's last-pushed property values — diffed by
    /// [`refresh_input_ui`] so idle ticks don't invalidate Slint
    /// properties (same pattern as `lobby_ui`).
    input_ui: InputUiSnapshot,
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
    // Welcome overlay's ROMs step (live while the user drops files in).
    app.set_welcome_has_roms(!st.roms.is_empty());
    app.set_welcome_roms_detected(t!(&lang, "welcome-step-roms-detected", count = st.roms.len() as i64).into());
    app.set_welcome_roms_path(st.config.roms_path().display().to_string().into());
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
    apply_patch_filter(app, st);
    refresh_patch_status(app, st);
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
            // Off by default: hide replays whose loaded stats say
            // incomplete; not-yet-known replays stay visible.
            let complete_ok = app.get_replay_show_incomplete()
                || st.replay_stats.get(&r.path).is_none_or(|s| s.is_complete);
            family_ok && opponent_ok && complete_ok
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

/// Rebuild the Patches tab's list model from the scanned map with the
/// search filter applied — case-insensitive substring match on the
/// patch's directory name and title, like tango's tabs/patches.rs.
/// Same index-layer pattern as [`apply_replay_filter`]; selection
/// resets because the surviving indices no longer line up.
fn apply_patch_filter(app: &AppWindow, st: &mut State) {
    let query = st.patch_filter.clone();
    let favs = &st.config.favorite_patches;
    // Favorites first (starred), alphabetical within each group — the
    // map iterates name-sorted, so a stable partition preserves that.
    let mut entries: Vec<_> = st
        .patches
        .iter()
        .filter(|(n, p)| {
            query.is_empty() || n.to_lowercase().contains(&query) || p.title.to_lowercase().contains(&query)
        })
        .collect();
    entries.sort_by_key(|(name, _)| !favs.contains(*name));
    let mut names = Vec::new();
    let mut rows = Vec::new();
    for (name, patch) in entries {
        names.push(name.clone());
        rows.push(PatchRow {
            title: patch.title.clone().into(),
            // Caption: the authors — or the directory name when the
            // patch declares none, so the row keeps its second line.
            authors: if patch.authors.is_empty() {
                name.clone().into()
            } else {
                patch.authors.join(", ").into()
            },
            favorite: favs.contains(name),
        });
    }
    st.patch_list_rows = names;
    st.patch_detail_name = None;
    st.patch_detail_versions.clear();
    app.set_selected_patch_item(-1);
    app.set_patch_list(ModelRc::new(VecModel::from(rows)));
}

/// Push the Patches tab's detail pane for list row `index`: header
/// fields, version rows (newest first), the README body, and the
/// selected version's supported-games caption.
fn push_patch_detail(app: &AppWindow, st: &mut State, index: usize) {
    let Some((name, patch)) = st
        .patch_list_rows
        .get(index)
        .and_then(|n| st.patches.get(n).map(|p| (n.clone(), p.clone())))
    else {
        return;
    };
    app.set_patch_detail_favorite(st.config.favorite_patches.contains(&name));
    st.patch_detail_name = Some(name);
    st.patch_detail_versions = patch.versions.keys().rev().cloned().collect();
    app.set_patch_title(patch.title.clone().into());
    app.set_patch_authors(patch.authors.join(", ").into());
    app.set_patch_license(patch.license.clone().unwrap_or_default().into());
    app.set_patch_source(patch.source.clone().unwrap_or_default().into());
    app.set_patch_readme(patch.readme.clone().unwrap_or_default().into());
    let version_model: Vec<SharedString> = st.patch_detail_versions.iter().map(|v| format!("v{v}").into()).collect();
    app.set_patch_versions(ModelRc::new(VecModel::from(version_model)));
    app.set_selected_patch_version(if st.patch_detail_versions.is_empty() { -1 } else { 0 });
    push_patch_supported_games(app, st);
}

/// The supported-games caption for the detail pane's currently picked
/// version — localized game display names, sorted; "—" when the
/// version supports nothing (or nothing is picked).
fn push_patch_supported_games(app: &AppWindow, st: &State) {
    let lang = &st.config.language;
    let games = st
        .patch_detail_name
        .as_ref()
        .and_then(|n| st.patches.get(n))
        .and_then(|p| {
            usize::try_from(app.get_selected_patch_version())
                .ok()
                .and_then(|i| st.patch_detail_versions.get(i))
                .and_then(|v| p.versions.get(v))
        })
        .map(|v| {
            let mut names: Vec<String> = v.supported_games.iter().map(|g| game::display_name(lang, *g)).collect();
            names.sort();
            names.join(", ")
        })
        .unwrap_or_default();
    app.set_patch_supported_games(if games.is_empty() { "—".to_string() } else { games }.into());
}

/// The Patches tab's sync status line: in-flight beats the last error
/// beats the last-updated stamp beats nothing.
fn refresh_patch_status(app: &AppWindow, st: &State) {
    let lang = &st.config.language;
    let (text, is_error) = if st.patch_updating {
        (t!(lang, "patches-updating"), false)
    } else if let Some(e) = &st.patch_update_error {
        (t!(lang, "patches-update-failed", error = e.clone()), true)
    } else if let Some(ts) = &st.patch_last_updated {
        // tango-ng-only "last updated" stamp — no tango key; stays English.
        (format!("Updated {}", ts.format("%H:%M")), false)
    } else {
        (String::new(), false)
    };
    app.set_patches_update_status(text.into());
    app.set_patches_status_error(is_error);
    app.set_patches_updating(st.patch_updating);
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
    // The New save button enables whenever the selected family has an
    // owned-ROM variant with creatable templates — independent of a
    // save being selected, so an empty family can bootstrap its first.
    let can_new = selected_game(app, st).is_some_and(|game| {
        !save_manage::creation_options(
            &st.config.language,
            game.family_and_variant().0,
            &st.roms,
            &st.patches,
            selected_patch(app, st).as_ref(),
        )
        .is_empty()
    });
    app.set_save_new_enabled(can_new);
}

/// Push the save viewer's models — navi header, section tabs, folder
/// rows — from `st.loaded`, or clear the whole viewer when it's gone.
fn push_save_view(app: &AppWindow, st: &State) {
    let l = match st.save_view_source {
        1 => st.replay_loaded.as_ref(),
        2 => st.pvp_local_loaded.as_ref(),
        3 => st.pvp_remote_loaded.as_ref(),
        _ => st.loaded.as_ref(),
    };
    let Some(l) = l else {
        app.global::<SaveView>().set_save_loaded(false);
        app.global::<SaveView>().set_navi_header(NaviHeader::default());
        app.global::<SaveView>().set_save_tabs(ModelRc::new(VecModel::from(Vec::<SaveTabItem>::new())));
        app.global::<SaveView>().set_folder_chips(ModelRc::new(VecModel::from(Vec::<ChipRow>::new())));
        app.global::<SaveView>().set_navicust_image(Image::default());
        app.global::<SaveView>().set_navicust_style_name(SharedString::default());
        app.global::<SaveView>().set_navicust_parts(ModelRc::new(VecModel::from(Vec::<NcpPartRow>::new())));
        app.global::<SaveView>().set_patch_card_lines(ModelRc::new(VecModel::from(Vec::<PatchCardLine>::new())));
        app.global::<SaveView>().set_abd_rows(ModelRc::new(VecModel::from(Vec::<AbdRow>::new())));
        return;
    };
    let lang = &st.config.language;
    app.global::<SaveView>().set_navi_header(loaded::navi_header(l));
    // Section gating like tango's available_tabs — each tab exists iff
    // its view does, in tango's order (NaviCust, Folder, Patch Cards,
    // Auto Battle Data). The kind rides with the label so the bodies
    // don't depend on tab position.
    let mut tabs: Vec<SaveTabItem> = Vec::new();
    // Streamer mode leads with the Cover tab (available_tabs), so the
    // default view leaks nothing.
    if st.config.streamer_mode {
        let cover = loaded::cover_model(l);
        app.global::<SaveView>().set_cover_logos(ModelRc::new(VecModel::from(cover.logos)));
        app.global::<SaveView>().set_cover_lane_w(cover.lane_w);
        app.global::<SaveView>().set_cover_lane_h(cover.lane_h);
        tabs.push(SaveTabItem {
            label: t!(lang, "save-tab-cover").into(),
            kind: 4,
        });
    }
    if let Some(nc) = loaded::navicust_model(l) {
        tabs.push(SaveTabItem {
            label: t!(lang, "save-tab-navicust").into(),
            kind: 0,
        });
        app.global::<SaveView>().set_navicust_image(nc.image);
        app.global::<SaveView>().set_navicust_aspect(nc.aspect);
        app.global::<SaveView>().set_navicust_style_name(nc.style_name.into());
        app.global::<SaveView>().set_navicust_label_x_frac(nc.label_x_frac);
        app.global::<SaveView>().set_navicust_label_y_frac(nc.label_y_frac);
        app.global::<SaveView>().set_navicust_label_h_frac(nc.label_h_frac);
        app.global::<SaveView>().set_navicust_parts(ModelRc::new(VecModel::from(nc.parts)));
    } else {
        app.global::<SaveView>().set_navicust_image(Image::default());
        app.global::<SaveView>().set_navicust_style_name(SharedString::default());
        app.global::<SaveView>().set_navicust_parts(ModelRc::new(VecModel::from(Vec::<NcpPartRow>::new())));
    }
    if l.save.view_chips().is_some() {
        tabs.push(SaveTabItem {
            label: t!(lang, "save-tab-folder").into(),
            kind: 1,
        });
    }
    if let Some((kind, lines)) = loaded::patch_card_lines(lang, l) {
        tabs.push(SaveTabItem {
            label: t!(lang, "save-tab-patch-cards").into(),
            kind: 2,
        });
        app.global::<SaveView>().set_patch_cards_kind(kind);
        app.global::<SaveView>().set_patch_card_lines(ModelRc::new(VecModel::from(lines)));
    } else {
        app.global::<SaveView>().set_patch_card_lines(ModelRc::new(VecModel::from(Vec::<PatchCardLine>::new())));
    }
    let abd = loaded::abd_rows(lang, l);
    if !abd.is_empty() {
        tabs.push(SaveTabItem {
            label: t!(lang, "save-tab-auto-battle-data").into(),
            kind: 3,
        });
    }
    app.global::<SaveView>().set_abd_rows(ModelRc::new(VecModel::from(abd)));
    app.global::<SaveView>().set_save_tabs(ModelRc::new(VecModel::from(tabs)));
    app.global::<SaveView>().set_save_active_tab(0);
    app.global::<SaveView>().set_folder_has_mb(l.assets.chips_have_mb());
    app.global::<SaveView>().set_folder_chips(ModelRc::new(VecModel::from(loaded::folder_rows(
        l,
        app.global::<SaveView>().get_folder_grouped(),
    ))));
    app.global::<SaveView>().set_save_loaded(true);
}

/// Push the current app state into Discord rich presence (tango's
/// App::update presence arm): in a session → single-player /
/// match-in-progress with the elapsed timer; dialing / negotiating →
/// "Looking for match" (matchmaking codes ride as the join secret);
/// lobby → "In lobby"; else the base game card. The client dedupes,
/// so the per-tick call is cheap.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn update_discord_presence(app: &AppWindow, st: &State) {
    let lang = &st.config.language;
    let game_info = selected_game(app, st).map(|g| {
        discord::make_game_info(g, selected_patch(app, st).as_ref().map(|(n, v)| (n.as_str(), v)), lang)
    });
    let activity = if let Some(session) = &st.session {
        let start = st.session_start.unwrap_or_else(std::time::SystemTime::now);
        match session {
            ActiveSession::Pvp(_) => discord::make_in_progress_activity(start, lang, game_info),
            _ => discord::make_single_player_activity(start, lang, game_info),
        }
    } else {
        match &st.netplay.phase {
            netplay::Phase::Connecting { ident, .. } | netplay::Phase::Negotiating { ident } => {
                discord::make_looking_activity(ident, lang, game_info)
            }
            netplay::Phase::Lobby { ident } => discord::make_in_lobby_activity(ident, lang, game_info),
            netplay::Phase::Idle | netplay::Phase::Failed { .. } => discord::make_base_activity(game_info),
        }
    };
    st.discord.set_current_activity(Some(activity));
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn update_discord_presence(_app: &AppWindow, _st: &State) {}

/// The active save-view section's kind (-1 when none) — mirror of the
/// .slint-side derived `save-kind` property, for the copy callbacks.
fn save_active_kind(app: &AppWindow) -> i32 {
    use slint::Model;
    let tabs = app.global::<SaveView>().get_save_tabs();
    usize::try_from(app.global::<SaveView>().get_save_active_tab())
        .ok()
        .and_then(|i| tabs.row_data(i))
        .map(|t| t.kind)
        .unwrap_or(-1)
}

/// --ui-shot: point the Play tab at the first (game, save) whose save
/// satisfies `pred` — game_rows order, then that game's save order
/// (the same order the save picker shows). No-op when nothing matches.
fn select_save_where(
    app: &AppWindow,
    state: &Rc<RefCell<State>>,
    pred: &dyn Fn(&(dyn tango_dataview::save::Save + Send + Sync)) -> bool,
) {
    let target = {
        let st = state.borrow();
        st.game_rows.iter().enumerate().find_map(|(gi, g)| {
            st.saves
                .get(g)
                .and_then(|saves| saves.iter().position(|s| pred(s.save.as_ref())))
                .map(|si| (gi, si))
        })
    };
    let Some((gi, si)) = target else { return };
    app.set_selected_game(gi as i32);
    app.invoke_game_selected(gi as i32);
    app.set_selected_save(si as i32);
    app.invoke_save_selected(si as i32);
}

/// --ui-shot: the save-tab-strip index of the section with `kind`.
fn save_tab_index_of_kind(app: &AppWindow, kind: i32) -> Option<i32> {
    use slint::Model;
    let tabs = app.global::<SaveView>().get_save_tabs();
    (0..tabs.row_count())
        .find(|&i| tabs.row_data(i).is_some_and(|t| t.kind == kind))
        .map(|i| i as i32)
}

/// Flip the copy button's label to "Copied!" for a moment (tango's
/// copy_feedback flash), reverting via a single-shot timer.
fn flash_copy_feedback(app: &AppWindow, image: bool) {
    if image {
        app.global::<SaveView>().set_save_copy_image_flash(true);
    } else {
        app.global::<SaveView>().set_save_copy_flash(true);
    }
    let app_weak = app.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(1200), move || {
        let Some(app) = app_weak.upgrade() else { return };
        if image {
            app.global::<SaveView>().set_save_copy_image_flash(false);
        } else {
            app.global::<SaveView>().set_save_copy_flash(false);
        }
    });
}

/// Reveal a file/folder in the OS file manager (tango's open_path).
/// Desktop-only; mobile has no folder browsing to speak of.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn open_path(path: impl AsRef<std::path::Path>) {
    let path = path.as_ref();
    if let Err(e) = open::that(path) {
        log::error!("open {}: {e}", path.display());
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn open_path(_path: impl AsRef<std::path::Path>) {}

/// Put plain text on the system clipboard. Desktop-only: arboard has
/// no Android/iOS backend, so mobile builds compile this to a no-op
/// (the mobile copy story is OS share sheets, a follow-up).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn copy_text_to_clipboard(text: &str) -> bool {
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
        Ok(()) => true,
        Err(e) => {
            log::error!("clipboard text copy failed: {e}");
            false
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn copy_text_to_clipboard(_text: &str) -> bool {
    false
}

/// Put an RGBA image on the system clipboard (the NaviCust grid).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn copy_image_to_clipboard(img: image::RgbaImage) -> bool {
    let (width, height) = (img.width() as usize, img.height() as usize);
    let data = arboard::ImageData {
        width,
        height,
        bytes: std::borrow::Cow::Owned(img.into_raw()),
    };
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_image(data)) {
        Ok(()) => true,
        Err(e) => {
            log::error!("clipboard image copy failed: {e}");
            false
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn copy_image_to_clipboard(_img: image::RgbaImage) -> bool {
    false
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
            // Streamer privacy: never render the joinable code on
            // screen (the masked input already hides the typed form).
            Some(netplay::LinkIdent::Matchmaking(_)) if st.config.streamer_mode => {
                t!(lang, "lobby-link-code", code = "•••".to_string())
            }
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

/// Recompute the joyflags from the configured mapping + held inputs
/// and push them to the active session; apply speed-up (fast-forward)
/// on its edges. The single input funnel — both the key-event
/// callback and the timer's gamepad poll end here.
fn sync_session_input(st: &mut State) {
    let joyflags = st.config.input_mapping.to_joyflags(&st.held);
    match &st.session {
        // PvP takes live input exactly like singleplayer — the
        // netcode reads the joyflag atomic on the primary trap.
        Some(ActiveSession::SinglePlayer(session)) => session.set_joyflags(joyflags),
        Some(ActiveSession::Pvp(session)) => session.set_joyflags(joyflags),
        _ => {}
    }
    let speed_up = st.config.input_mapping.speed_up_held(&st.held);
    if speed_up != st.speed_up {
        st.speed_up = speed_up;
        match &st.session {
            Some(ActiveSession::SinglePlayer(session)) => {
                session.set_speed(if speed_up { 3.0 } else { 1.0 });
            }
            Some(ActiveSession::Replay(session)) => {
                session.set_speed(if speed_up { 4.0 } else { st.replay_speed });
            }
            // PvP is throttled by the netcode's clock-sync — no
            // fast-forward.
            Some(ActiveSession::Pvp(_)) | None => {}
        }
    }
}

/// Localized display name for a mapped key — the bezel caption under
/// the input pane's LCD.
fn mapped_key_label(lang: &unic_langid::LanguageIdentifier, k: input::MappedKey) -> String {
    match k {
        input::MappedKey::Up => t!(lang, "input-key-up"),
        input::MappedKey::Down => t!(lang, "input-key-down"),
        input::MappedKey::Left => t!(lang, "input-key-left"),
        input::MappedKey::Right => t!(lang, "input-key-right"),
        input::MappedKey::A => t!(lang, "input-key-a"),
        input::MappedKey::B => t!(lang, "input-key-b"),
        input::MappedKey::L => t!(lang, "input-key-l"),
        input::MappedKey::R => t!(lang, "input-key-r"),
        input::MappedKey::Start => t!(lang, "input-key-start"),
        input::MappedKey::Select => t!(lang, "input-key-select"),
        input::MappedKey::SpeedUp => t!(lang, "input-key-speed-up"),
    }
}

/// Everything the Input settings pane renders. Computed fresh each
/// tick by [`compute_input_snapshot`] and diffed against the previous
/// push in [`refresh_input_ui`].
#[derive(Clone, PartialEq)]
struct InputUiSnapshot {
    /// Index into `input::MappedKey::ALL`; -1 = nothing selected.
    selected: i32,
    caption: String,
    capturing: bool,
    /// Per-key "a binding is held right now", parallel to `ALL`.
    lit: Vec<bool>,
    /// The selected key's bindings: (chip glyph, label, held).
    chips: Vec<(SharedString, SharedString, bool)>,
}

impl Default for InputUiSnapshot {
    /// Matches the `.slint` property defaults, so the first diff pass
    /// pushes only what differs.
    fn default() -> Self {
        Self {
            selected: -1,
            caption: String::new(),
            capturing: false,
            lit: Vec::new(),
            chips: Vec::new(),
        }
    }
}

fn compute_input_snapshot(st: &State) -> InputUiSnapshot {
    let lang = &st.config.language;
    let mapping = &st.config.input_mapping;
    let lit = input::MappedKey::ALL
        .iter()
        .map(|&k| mapping.slot(k).iter().any(|b| st.held.is_active(b)))
        .collect();
    let (selected, caption, chips) = match st.input_selected {
        Some(k) => {
            let chips = mapping
                .slot(k)
                .iter()
                .map(|b| {
                    let (kind, label) = input::describe(lang, b);
                    let glyph = match kind {
                        // Lucide keyboard / gamepad-2 — same
                        // codepoints as the Glyphs global.
                        input::BindingKind::Keyboard => "\u{e284}",
                        input::BindingKind::Gamepad => "\u{e0df}",
                    };
                    (glyph.into(), label.into(), st.held.is_active(b))
                })
                .collect();
            (k.index() as i32, mapped_key_label(lang, k), chips)
        }
        None => (-1, String::new(), Vec::new()),
    };
    InputUiSnapshot {
        selected,
        caption,
        capturing: st.input_capture.is_some(),
        lit,
        chips,
    }
}

/// Refresh the Input settings pane: push only the properties whose
/// values changed since the last push. Runs every tick (the live
/// key-light feedback) — an idle tick pushes nothing.
fn refresh_input_ui(app: &AppWindow, st: &mut State) {
    let snap = compute_input_snapshot(st);
    let prev = &st.input_ui;
    if snap.selected != prev.selected {
        app.set_input_selected(snap.selected);
    }
    if snap.caption != prev.caption {
        app.set_input_selected_label(snap.caption.as_str().into());
    }
    if snap.capturing != prev.capturing {
        app.set_input_capturing(snap.capturing);
    }
    if snap.lit != prev.lit {
        app.set_input_lit(ModelRc::new(VecModel::from(snap.lit.clone())));
    }
    if snap.chips != prev.chips {
        app.set_input_chips(ModelRc::new(VecModel::from(
            snap.chips
                .iter()
                .map(|(glyph, label, lit)| InputChip {
                    glyph: glyph.clone(),
                    label: label.clone(),
                    lit: *lit,
                })
                .collect::<Vec<_>>(),
        )));
    }
    st.input_ui = snap;
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

/// Marks a process as the supervised child (the actual UI); the
/// supervisor sets it to "1" before respawning ourselves.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const TANGO_CHILD_ENV_VAR: &str = "TANGO_CHILD";

/// Set by the supervisor to the `minidumper` IPC socket path the child
/// connects to for out-of-process crash dumps.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const TANGO_CRASH_SOCKET_ENV_VAR: &str = "TANGO_CRASH_SOCKET";

/// Desktop entry: the crash-handling trampoline (tango's main.rs).
/// The parent spawns `current_exe()` again as the supervised child
/// with stderr piped into a timestamped log, runs the out-of-process
/// minidump server, and pops a localized dialog on a non-zero exit.
/// The child branch just runs the app. Android enters via
/// `android_main` → [`run`] directly — no trampoline there.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn main() -> anyhow::Result<()> {
    if std::env::var(TANGO_CHILD_ENV_VAR).as_deref() == Ok("1") {
        return run();
    }
    match supervisor_main() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("crash supervisor failed: {e:?}");
            std::process::exit(2);
        }
    }
}

/// Parent half of the trampoline — see [`main`].
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn supervisor_main() -> anyhow::Result<i32> {
    use std::io::Write;
    let config = config::Config::load();
    let lang = config.language.clone();

    let logs_dir = config.logs_path();
    let _ = std::fs::create_dir_all(&logs_dir);
    let ts = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let log_path = logs_dir.join(format!("{ts}.log"));
    let dump_path = logs_dir.join(format!("{ts}.dmp"));

    let mut log_file = match std::fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title(t!(&lang, "window-title"))
                .set_description(t!(&lang, "crash-no-log", error = format!("{e:?}")))
                .set_level(rfd::MessageLevel::Error)
                .show();
            return Err(e.into());
        }
    };

    // The minidump server runs here, so route the supervisor's own
    // logging into the log file too.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(log_file.try_clone()?)))
        .try_init();

    // Start the dump server before spawning the child so the child's
    // connect can't race the bind; on failure we still run, just
    // without minidumps.
    let sock_path = crash_socket_path();
    let _ = std::fs::remove_file(&sock_path);
    let crash_server = start_crash_server(&sock_path, log_file.try_clone()?, dump_path.clone());

    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args_os().skip(1).collect::<Vec<std::ffi::OsString>>())
        .env(TANGO_CHILD_ENV_VAR, "1")
        .env("RUST_BACKTRACE", "1")
        .stderr(log_file.try_clone()?);
    if crash_server.is_some() {
        cmd.env(TANGO_CRASH_SOCKET_ENV_VAR, &sock_path);
    }
    let status = cmd.spawn()?.wait()?;

    writeln!(&mut log_file, "exit status: {status:?}")?;

    if let Some((handle, shutdown)) = crash_server {
        shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = handle.join();
    }
    let _ = std::fs::remove_file(&sock_path);

    if !status.success() {
        rfd::MessageDialog::new()
            .set_title(t!(&lang, "window-title"))
            .set_description(t!(&lang, "crash", path = log_path.display().to_string()))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }

    Ok(status.code().unwrap_or(0))
}

/// Absolute AF_UNIX socket path for the crash IPC channel, per-pid.
/// macOS pins it under short `/tmp` — the path doubles as the mach
/// service name / `sun_path`, capped around 104 bytes.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn crash_socket_path() -> std::path::PathBuf {
    let name = format!("tango-ng-crash-{}.sock", std::process::id());
    #[cfg(target_os = "macos")]
    {
        std::path::PathBuf::from("/tmp").join(name)
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::temp_dir().join(name)
    }
}

/// Bind the `minidumper` server and run it on a background thread;
/// `None` if it couldn't start (the child then gets no minidumps).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn start_crash_server(
    sock_path: &std::path::Path,
    log: std::fs::File,
    dump_path: std::path::PathBuf,
) -> Option<(
    std::thread::JoinHandle<()>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
)> {
    let mut server = match minidumper::Server::with_name(sock_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not start crash dump server: {e:?}");
            return None;
        }
    };
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_thread = shutdown.clone();
    // Separate handle so a fatal `run()` error still lands in the log.
    let mut err_log = log.try_clone().ok();
    let handler = CrashServerHandler {
        log: std::sync::Mutex::new(log),
        dump_path,
        sock_path: sock_path.to_path_buf(),
    };
    let handle = std::thread::spawn(move || {
        if let Err(e) = server.run(Box::new(handler), &shutdown_thread, None) {
            if let Some(l) = err_log.as_mut() {
                use std::io::Write;
                let _ = writeln!(l, "crash dump server error: {e:?}");
                let _ = l.flush();
            }
        }
    });
    Some((handle, shutdown))
}

/// `minidumper` server-side hooks: dump location + crash-block logging
/// into the same file the child's stderr streams into.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
struct CrashServerHandler {
    log: std::sync::Mutex<std::fs::File>,
    dump_path: std::path::PathBuf,
    sock_path: std::path::PathBuf,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl minidumper::ServerHandler for CrashServerHandler {
    fn create_minidump_file(&self) -> Result<(std::fs::File, std::path::PathBuf), std::io::Error> {
        let file = std::fs::File::create(&self.dump_path)?;
        Ok((file, self.dump_path.clone()))
    }

    fn on_minidump_created(
        &self,
        result: Result<minidumper::MinidumpBinary, minidumper::Error>,
    ) -> minidumper::LoopAction {
        use std::io::Write;
        if let Ok(mut log) = self.log.lock() {
            let _ = writeln!(log, "\n=== native crash ===");
            match result {
                Ok(md) => {
                    let _ = writeln!(log, "minidump written: {}", md.path.display());
                }
                Err(e) => {
                    let _ = writeln!(log, "minidump FAILED: {e:?}");
                }
            }
            let _ = writeln!(log, "=== end native crash ===\n");
            let _ = log.flush();
        }
        minidumper::LoopAction::Continue
    }

    fn on_message(&self, _kind: u32, _buffer: Vec<u8>) {}

    fn on_client_connected(&self, _num_clients: usize) -> minidumper::LoopAction {
        // The endpoint keeps working off open fds — unlink the name now.
        let _ = std::fs::remove_file(&self.sock_path);
        minidumper::LoopAction::Continue
    }

    fn on_client_disconnected(&self, _num_clients: usize) -> minidumper::LoopAction {
        minidumper::LoopAction::Exit
    }
}

/// The whole app: scan, window, event loop (and the --smoke/--ui-shot
/// verification modes). The desktop binary calls this from `main`; the
/// Android entry will call it from `android_main` once the NDK story
/// lands — the bin/lib split exists for that entry point.
pub fn run() -> anyhow::Result<()> {
    #[cfg(not(target_os = "android"))]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    #[cfg(target_os = "android")]
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    // Catch native crashes (segfaults / SEH / mach exceptions) from the
    // mgba C side: connect to the supervisor's out-of-process dump
    // server when launched under one, and install the panic hook (full
    // inline backtrace to stderr — the supervisor pipes it to the log).
    // Leaked so it stays installed for the process's lifetime.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let crash_client = std::env::var_os(TANGO_CRASH_SOCKET_ENV_VAR).and_then(|name| {
            match minidumper::Client::with_name(std::path::Path::new(&name)) {
                Ok(c) => Some(c),
                Err(e) => {
                    log::error!("could not connect to crash dump server: {e:?}");
                    None
                }
            }
        });
        std::mem::forget(crash_log::install(crash_client));
    }
    log::info!("tango-ng {}", env!("CARGO_PKG_VERSION"));

    // The async (netplay) layer's runtime. Held for the program lifetime;
    // tasks get the Handle (threaded into netplay::State below) — the
    // emulator thread must never have the runtime *entered*
    // (PvpSender::send uses blocking_send).
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;

    let config = config::Config::load();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let config_for_updater = (config.data_path.clone(), config.allow_prerelease_upgrades);
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

    // `tango-ng Join <code>` / `tango-ng tango://join/<code>` (Discord
    // deep-links + tango's CLI Join): pre-fill the Play tab's link code.
    let init_link_code: Option<String> = match (args.get(1).map(|s| s.as_str()), args.get(2)) {
        (Some("Join"), Some(code)) => Some(code.clone()),
        (Some(url), _) if url.starts_with("tango://join/") => {
            Some(url.trim_start_matches("tango://join/").trim_end_matches('/').to_string())
        }
        _ => None,
    };

    let app = AppWindow::new()?;
    if let Some(code) = &init_link_code {
        app.set_link_code(code.as_str().into());
        app.set_active_tab(0);
    }
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
        replay_stats: HashMap::new(),
        replay_loaded: None,
        replay_loaded_path: None,
        save_view_source: 0,
        pvp_local_loaded: None,
        pvp_remote_loaded: None,
        session_last_pointer: std::time::Instant::now(),
        patches: patch::PatchMap::new(),
        patch_rows: Vec::new(),
        patch_list_rows: Vec::new(),
        patch_filter: String::new(),
        patch_detail_name: None,
        patch_detail_versions: Vec::new(),
        patch_updating: false,
        patch_update_error: None,
        patch_last_updated: None,
        version_rows: Vec::new(),
        loaded: None,
        session: None,
        held: input::Held::default(),
        speed_up: false,
        input_selected: None,
        input_capture: None,
        input_ui: InputUiSnapshot::default(),
        replay_speed: 1.0,
        scrub_was_playing: false,
        scrub_forced: false,
        netplay: netplay::State::new(tokio_runtime.handle().clone(), tx.clone()),
        // Loaded once here; every matchmaking Connect clones it.
        identity: identity::load(),
        lobby_mt_rows: Vec::new(),
        lobby_ui: LobbySnapshot::default(),
        save_template_values: Vec::new(),
        save_new_auto_default: None,
        pending_select_save: None,
        session_start: None,
        export_canceller: None,
        export_output: None,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        discord: discord::Client::new(tokio_runtime.handle().clone()),
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        updater: updater::Updater::new(
            tokio_runtime.handle().clone(),
            &config_for_updater.0.join("updater"),
            config_for_updater.1,
        ),
    }));
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if state.borrow().config.enable_updater {
        state.borrow_mut().updater.set_enabled(true);
    }

    // Background scan; results come back over the channel and are folded
    // into the UI by the frame timer below. Rc so the data-path setting
    // can retrigger it.
    let stats_tx = tx.clone();
    let stats_sweep_tx = tx.clone();
    let export_tx = tx.clone();
    let pvp_tx = tx.clone();
    let patch_update_tx = tx.clone();
    let autoupdate_tx = tx.clone();
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

    // Background patch-repo autoupdater (tango's patch::Autoupdater
    // minus the Scanner plumbing): first sync immediately, then every
    // 15 minutes; each result folds back through the same
    // PatchUpdateDone event as the manual Update button (success →
    // rescan). Skipped under --ui-shot — the walker's selections must
    // not be reset by a mid-walk rescan, and the shot run shouldn't
    // touch the network.
    if state.borrow().config.enable_patch_autoupdate && ui_shot_dir.is_none() {
        let url = state.borrow().config.patch_repo_url();
        let root = state.borrow().config.patches_path();
        let tx = autoupdate_tx;
        log::info!("starting patch autoupdater (every {:?})", patch::AUTOUPDATE_INTERVAL);
        tokio_runtime.handle().spawn(async move {
            loop {
                let result = patch::update(url.clone(), root.clone()).await.map_err(|e| e.to_string());
                if tx.send(Event::PatchUpdateDone { result, background: true }).is_err() {
                    break;
                }
                tokio::time::sleep(patch::AUTOUPDATE_INTERVAL).await;
            }
        });
    }

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
        app.set_settings_video_filter(
            video::Filter::ALL
                .iter()
                .position(|f| *f == video::Filter::from_config(&st.config.video_filter))
                .unwrap_or(0) as i32,
        );
        st.audio_binder.set_volume(st.config.volume);
        app.set_settings_streamer(st.config.streamer_mode);
        app.set_settings_patch_repo(st.config.patch_repo.clone().into());
        app.set_settings_patch_autoupdate(st.config.enable_patch_autoupdate);
        app.set_settings_fullscreen(st.config.full_screen);
        app.set_settings_mute_bgm(st.config.disable_bgm_in_pvp);
        app.set_settings_matchmaking_endpoint(st.config.matchmaking_endpoint.clone().into());
        app.set_settings_relay(match st.config.relay_mode {
            config::RelayMode::Auto => 0,
            config::RelayMode::Always => 1,
            config::RelayMode::Never => 2,
        });
        app.set_settings_show_opponent_setup(st.config.show_opponent_setup);
        app.set_settings_updater_enabled(st.config.enable_updater);
        app.set_settings_prerelease(st.config.allow_prerelease_upgrades);
        if st.config.full_screen {
            app.window().set_fullscreen(true);
        }
        // CREDITS.md, embedded at build time (same file the iced About
        // renders); pre-split into lines for the ListView.
        app.set_about_credits(ModelRc::new(VecModel::from(
            include_str!("../../CREDITS.md")
                .lines()
                .map(SharedString::from)
                .collect::<Vec<_>>(),
        )));
        // First run (no nickname yet): the welcome overlay leads —
        // language pick, ROMs, nickname. Continue clears it for good.
        if st.config.nickname.is_none() && ui_shot_dir.is_none() {
            app.set_welcome_visible(true);
        }
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

    app.on_streamer_changed({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |streamer| {
            let Some(app) = app_weak.upgrade() else { return };
            {
                let mut st = state.borrow_mut();
                st.config.streamer_mode = streamer;
                st.config.save();
            }
            // The Cover tab appears/disappears with the mode.
            let st = state.borrow();
            push_save_view(&app, &st);
        }
    });

    app.on_patch_repo_changed({
        let state = state.clone();
        move |repo| {
            let mut st = state.borrow_mut();
            st.config.patch_repo = repo.trim().to_string();
            st.config.save();
        }
    });

    app.on_patch_autoupdate_changed({
        let state = state.clone();
        move |enabled| {
            let mut st = state.borrow_mut();
            st.config.enable_patch_autoupdate = enabled;
            st.config.save();
        }
    });

    app.on_video_filter_changed({
        let state = state.clone();
        move |index| {
            let mut st = state.borrow_mut();
            let filter = usize::try_from(index)
                .ok()
                .and_then(|i| video::Filter::ALL.get(i).copied())
                .unwrap_or_default();
            st.config.video_filter = filter.config_name().to_string();
            st.config.save();
        }
    });

    app.on_fullscreen_changed({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |fullscreen| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.config.full_screen = fullscreen;
            st.config.save();
            app.window().set_fullscreen(fullscreen);
        }
    });

    app.on_mute_bgm_changed({
        let state = state.clone();
        move |mute| {
            let mut st = state.borrow_mut();
            st.config.disable_bgm_in_pvp = mute;
            st.config.save();
        }
    });

    app.on_matchmaking_endpoint_changed({
        let state = state.clone();
        move |endpoint| {
            let mut st = state.borrow_mut();
            let endpoint = endpoint.trim();
            st.config.matchmaking_endpoint = if endpoint.is_empty() {
                config::DEFAULT_MATCHMAKING_ENDPOINT.to_string()
            } else {
                endpoint.to_string()
            };
            st.config.save();
        }
    });

    app.on_relay_changed({
        let state = state.clone();
        move |index| {
            let mut st = state.borrow_mut();
            st.config.relay_mode = match index {
                1 => config::RelayMode::Always,
                2 => config::RelayMode::Never,
                _ => config::RelayMode::Auto,
            };
            st.config.save();
        }
    });

    app.on_updater_enabled_changed({
        let state = state.clone();
        move |enabled| {
            let mut st = state.borrow_mut();
            st.config.enable_updater = enabled;
            st.config.save();
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            st.updater.set_enabled(enabled);
            #[cfg(any(target_os = "android", target_os = "ios"))]
            let _ = enabled;
        }
    });

    app.on_prerelease_changed({
        let state = state.clone();
        move |allow| {
            // Sampled by the updater at startup, like tango - takes
            // effect next launch.
            let mut st = state.borrow_mut();
            st.config.allow_prerelease_upgrades = allow;
            st.config.save();
        }
    });

    app.on_update_now({
        let state = state.clone();
        move || {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            state.borrow().updater.finish_update();
            #[cfg(any(target_os = "android", target_os = "ios"))]
            let _ = &state;
        }
    });

    app.on_show_opponent_setup_changed({
        let state = state.clone();
        move |show| {
            let mut st = state.borrow_mut();
            st.config.show_opponent_setup = show;
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

            // Patches supporting this game (any version), favorites
            // first + starred like tango's loadout picker.
            st.patch_rows = st
                .patches
                .iter()
                .filter(|(_, p)| p.versions.values().any(|v| v.supported_games.contains(&game)))
                .map(|(name, _)| name.clone())
                .collect();
            let favs = st.config.favorite_patches.clone();
            st.patch_rows.sort_by_key(|name| !favs.contains(name));
            st.version_rows.clear();
            let mut patch_model: Vec<SharedString> =
                vec![SharedString::from(t!(&st.config.language, "play-no-patch"))];
            patch_model.extend(st.patch_rows.iter().map(|n| {
                if st.config.favorite_patches.contains(n) {
                    SharedString::from(format!("★ {n}"))
                } else {
                    SharedString::from(n.as_str())
                }
            }));
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

    // Save-view copy affordances (tango's CopyTab / CopyTabImage): the
    // active section as TSV text, or the NaviCust grid as an image.
    // Desktop-only — the clipboard crate has no mobile backends; the
    // buttons stay visible but inert there.
    app.global::<SaveView>().on_save_copy({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
            let l = match st.save_view_source {
                1 => st.replay_loaded.as_ref(),
                2 => st.pvp_local_loaded.as_ref(),
                3 => st.pvp_remote_loaded.as_ref(),
                _ => st.loaded.as_ref(),
            };
            let Some(l) = l else { return };
            let kind = save_active_kind(&app);
            let Some(text) = loaded::section_as_text(l, kind, app.global::<SaveView>().get_folder_grouped()) else {
                return;
            };
            if copy_text_to_clipboard(&text) {
                flash_copy_feedback(&app, false);
            }
        }
    });

    app.global::<SaveView>().on_save_copy_image({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
            let l = match st.save_view_source {
                1 => st.replay_loaded.as_ref(),
                2 => st.pvp_local_loaded.as_ref(),
                3 => st.pvp_remote_loaded.as_ref(),
                _ => st.loaded.as_ref(),
            };
            let Some(l) = l else { return };
            let Some(img) = loaded::navicust_clipboard_image(l) else {
                return;
            };
            if copy_image_to_clipboard(img) {
                flash_copy_feedback(&app, true);
            }
        }
    });

    // ---- welcome / first-run overlay (tabs/welcome.rs) ----

    app.on_welcome_open_roms({
        let state = state.clone();
        move || {
            let st = state.borrow();
            let path = st.config.roms_path();
            let _ = std::fs::create_dir_all(&path);
            open_path(path);
        }
    });

    app.on_welcome_rescan({
        let spawn_scan = spawn_scan.clone();
        move || spawn_scan()
    });

    app.on_welcome_continue({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let nickname = app.get_welcome_nickname().trim().to_string();
            if nickname.is_empty() || st.roms.is_empty() {
                return;
            }
            st.config.nickname = Some(nickname.clone());
            st.config.save();
            app.set_settings_nickname(nickname.into());
            app.set_welcome_visible(false);
        }
    });

    // ---- replay video export (tabs/replays/export.rs, condensed:
    // whole-replay render, no per-round mask / save-as dialog — the
    // output lands beside the replay as <stem>-render.<ext>) ----

    app.on_replay_export_scale_edited({
        let app_weak = app.as_weak();
        move |value| {
            let Some(app) = app_weak.upgrade() else { return };
            let scale = (value.clamp(0.0, 1.0) * 10.0).round() as u32;
            app.set_replay_export_scale_label(if scale == 0 {
                "lossless".into()
            } else {
                format!("{scale}x").into()
            });
        }
    });

    app.on_replay_export_start({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            if st.export_canceller.is_some() {
                return;
            }
            let Some(path) = st.replay_detail_path.clone() else { return };
            // Resolve both sides' ROMs up front (cheap; scanned bytes +
            // BPS reapply); the decode + render run on their own thread.
            let metadata = match st.replay_rows.iter().find(|r| r.path == path) {
                Some(r) => r.metadata.clone(),
                None => return,
            };
            let patches_path = st.config.patches_path();
            let prep = (|| -> anyhow::Result<_> {
                let (lg, lr) = resolve_replay_rom(&st.roms, &patches_path, metadata.local_side.as_ref())?;
                let (rg, rr) = resolve_replay_rom(&st.roms, &patches_path, metadata.remote_side.as_ref())?;
                Ok((lg, lr, rg, rr))
            })();
            let (local_game, local_rom, remote_game, remote_rom) = match prep {
                Ok(p) => p,
                Err(e) => {
                    let lang = &st.config.language;
                    app.set_replay_export_status(t!(lang, "replays-export-error", error = format!("{e}")).into());
                    return;
                }
            };
            let scale = (app.get_replay_export_scale().clamp(0.0, 1.0) * 10.0).round() as u32;
            let disable_bgm = app.get_replay_export_mute();
            let twosided = app.get_replay_export_twosided();
            let output = path.with_file_name(format!(
                "{}-render.{}",
                path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                if scale == 0 { "mkv" } else { "mp4" }
            ));
            let canceller = tango_pvp::replay::export::Canceller::default();
            st.export_canceller = Some(canceller.clone());
            st.export_output = None;
            app.set_replay_export_has_output(false);
            app.set_replay_export_status(String::new().into());
            app.set_replay_export_progress(0.0);
            app.set_replay_exporting(true);
            let tx = export_tx.clone();
            // Dedicated OS thread: the export is fully synchronous
            // (std::process ffmpeg pipes), like tango's export thread.
            std::thread::Builder::new()
                .name("replay-export".to_string())
                .spawn(move || {
                    let result = (|| -> anyhow::Result<std::path::PathBuf> {
                        let f = std::fs::File::open(&path)?;
                        let replay = tango_pvp::replay::Replay::decode(f)?;
                        let mut settings = tango_pvp::replay::export::Settings::default_with_scale(
                            (scale > 0).then_some(scale as usize),
                        );
                        settings.disable_bgm = disable_bgm;
                        let masks = vec![vec![true; replay.rounds.len()]];
                        let cb_tx = tx.clone();
                        let cb = move |current: usize, total: usize| {
                            let _ = cb_tx.send(Event::ExportProgress { current, total });
                        };
                        if twosided {
                            tango_pvp::replay::export::export_twosided(
                                &local_rom,
                                local_game.hooks,
                                &remote_rom,
                                remote_game.hooks,
                                &[replay],
                                &masks,
                                &output,
                                &settings,
                                &canceller,
                                cb,
                            )?;
                        } else {
                            tango_pvp::replay::export::export(
                                &local_rom,
                                local_game.hooks,
                                &remote_rom,
                                remote_game.hooks,
                                &[replay],
                                &masks,
                                &output,
                                &settings,
                                &canceller,
                                cb,
                            )?;
                        }
                        Ok(output)
                    })();
                    let _ = tx.send(Event::ExportDone {
                        result: result.map_err(|e| format!("{e}")),
                    });
                })
                .expect("spawn replay-export thread");
        }
    });

    app.on_replay_export_cancel({
        let state = state.clone();
        move || {
            let st = state.borrow();
            if let Some(c) = &st.export_canceller {
                c.kill();
            }
        }
    });

    app.on_replay_export_open({
        let state = state.clone();
        move || {
            let st = state.borrow();
            if let Some(out) = &st.export_output {
                open_path(out);
            }
        }
    });

    app.on_replay_incomplete_toggled({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |_show| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let family = usize::try_from(app.get_selected_replay_filter())
                .ok()
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| st.replay_filter_families.get(i))
                .cloned();
            apply_replay_filter(&app, &mut st, family.as_deref());
        }
    });

    app.on_replay_open_folder({
        let state = state.clone();
        move || {
            let st = state.borrow();
            if let Some(dir) = st.replay_detail_path.as_ref().and_then(|p| p.parent()) {
                open_path(dir);
            }
        }
    });

    // ---- patches tab polish (favorites, open folder, source link) ----

    app.on_patch_favorite_toggle({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some(name) = st.patch_detail_name.clone() else { return };
            if !st.config.favorite_patches.remove(&name) {
                st.config.favorite_patches.insert(name.clone());
            }
            st.config.save();
            // Re-sort the list around the change, then re-point the
            // selection at the same patch (the indices moved).
            apply_patch_filter(&app, &mut st);
            if let Some(idx) = st.patch_list_rows.iter().position(|n| *n == name) {
                app.set_selected_patch_item(idx as i32);
                push_patch_detail(&app, &mut st, idx);
            }
        }
    });

    app.on_patch_open_folder({
        let state = state.clone();
        move || {
            let st = state.borrow();
            if let Some(patch) = st.patch_detail_name.as_ref().and_then(|n| st.patches.get(n)) {
                open_path(&patch.path);
            }
        }
    });

    app.on_patch_open_source({
        let state = state.clone();
        move || {
            let st = state.borrow();
            if let Some(source) = st
                .patch_detail_name
                .as_ref()
                .and_then(|n| st.patches.get(n))
                .and_then(|p| p.source.clone())
            {
                // open::that handles URLs as well as paths.
                open_path(source);
            }
        }
    });

    // ---- save management (save_manage.rs; tango's save_manage flows) ----

    app.on_save_open_folder({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
            let Some(save) = usize::try_from(app.get_selected_save())
                .ok()
                .and_then(|i| st.save_rows.get(i))
            else {
                return;
            };
            if let Some(dir) = save.path.parent() {
                open_path(dir);
            }
        }
    });

    app.on_save_rename_start({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
            let Some(save) = usize::try_from(app.get_selected_save())
                .ok()
                .and_then(|i| st.save_rows.get(i))
            else {
                return;
            };
            let stem = save
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            app.set_save_action_draft(stem.into());
            app.set_save_action(1);
        }
    });

    app.on_save_duplicate_start({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
            let Some(save) = usize::try_from(app.get_selected_save())
                .ok()
                .and_then(|i| st.save_rows.get(i))
            else {
                return;
            };
            app.set_save_action_draft(save_manage::suggest_duplicate_stem(&save.path).into());
            app.set_save_action(2);
        }
    });

    app.on_save_delete_start({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
            let Some(save) = usize::try_from(app.get_selected_save())
                .ok()
                .and_then(|i| st.save_rows.get(i))
            else {
                return;
            };
            let name = save
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            app.set_save_delete_prompt_text(t!(&st.config.language, "save-delete-prompt", name = name).into());
            app.set_save_action(3);
        }
    });

    app.on_save_new_start({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some(game) = selected_game(&app, &st) else { return };
            let lang = st.config.language.clone();
            let patch = selected_patch(&app, &st);
            let options = save_manage::creation_options(
                &lang,
                game.family_and_variant().0,
                &st.roms,
                &st.patches,
                patch.as_ref(),
            );
            if options.is_empty() {
                return;
            }
            let labels: Vec<SharedString> = options.iter().map(|o| o.label.clone().into()).collect();
            // Auto-select only when there's exactly one option;
            // otherwise force an explicit pick (Create stays disabled).
            let (selected, draft) = if options.len() == 1 {
                let o = &options[0];
                (
                    0,
                    save_manage::disambiguate_save_name(
                        &st.config.saves_path(),
                        &save_manage::suggest_save_name(&lang, o.game, Some(&o.raw)),
                    ),
                )
            } else {
                (
                    -1,
                    save_manage::disambiguate_save_name(
                        &st.config.saves_path(),
                        &save_manage::sanitize_filename(&game::family_display_name(
                            &lang,
                            game.family_and_variant().0,
                        )),
                    ),
                )
            };
            st.save_template_values = options.into_iter().map(|o| (o.game, o.raw)).collect();
            st.save_new_auto_default = Some(draft.clone());
            app.set_save_template_options(ModelRc::new(VecModel::from(labels)));
            app.set_save_template_selected(selected);
            app.set_save_action_draft(draft.into());
            app.set_save_action(4);
        }
    });

    app.on_save_template_picked({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some((game, raw)) = usize::try_from(index)
                .ok()
                .and_then(|i| st.save_template_values.get(i))
                .cloned()
            else {
                return;
            };
            // Regenerate the suggested name only while the user hasn't
            // typed over the last auto-generated one.
            let current = app.get_save_action_draft();
            if st.save_new_auto_default.as_deref() == Some(current.as_str()) {
                let lang = st.config.language.clone();
                let draft = save_manage::disambiguate_save_name(
                    &st.config.saves_path(),
                    &save_manage::suggest_save_name(&lang, game, Some(&raw)),
                );
                st.save_new_auto_default = Some(draft.clone());
                app.set_save_action_draft(draft.into());
            }
        }
    });

    app.on_save_action_cancel({
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            app.set_save_action(0);
        }
    });

    app.on_save_action_confirm({
        let state = state.clone();
        let app_weak = app.as_weak();
        let spawn_scan = spawn_scan.clone();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let action = app.get_save_action();
            let draft = app.get_save_action_draft().trim().to_string();
            app.set_save_action(0);
            let result: anyhow::Result<()> = (|| {
                let mut st = state.borrow_mut();
                let selected = usize::try_from(app.get_selected_save())
                    .ok()
                    .and_then(|i| st.save_rows.get(i))
                    .map(|s| s.path.clone());
                let game = selected_game(&app, &st);
                match action {
                    1 | 2 => {
                        anyhow::ensure!(!draft.is_empty(), "empty save name");
                        let src = selected.ok_or_else(|| anyhow::anyhow!("no save selected"))?;
                        let dst = if action == 1 {
                            save_manage::rename_save(&src, &draft)?
                        } else {
                            save_manage::duplicate_save(&src, &draft)?
                        };
                        st.pending_select_save = game.map(|g| (g, dst));
                    }
                    3 => {
                        let src = selected.ok_or_else(|| anyhow::anyhow!("no save selected"))?;
                        std::fs::remove_file(&src)?;
                    }
                    4 => {
                        anyhow::ensure!(!draft.is_empty(), "empty save name");
                        let (game, raw) = usize::try_from(app.get_save_template_selected())
                            .ok()
                            .and_then(|i| st.save_template_values.get(i))
                            .cloned()
                            .ok_or_else(|| anyhow::anyhow!("no template selected"))?;
                        let patch = selected_patch(&app, &st);
                        let template = save_manage::creation_template(game, &raw, &st.patches, patch.as_ref())
                            .ok_or_else(|| anyhow::anyhow!("template vanished"))?;
                        let dst = save_manage::create_new_save(&st.config.saves_path(), &draft, template.as_ref())?;
                        st.pending_select_save = Some((game, dst));
                    }
                    _ => {}
                }
                Ok(())
            })();
            match result {
                // The files changed under the scanner — rescan; the
                // ScanDone fold reselects `pending_select_save`.
                Ok(()) => spawn_scan(),
                Err(e) => {
                    log::error!("save action {action} failed: {e}");
                    app.set_status(format!("{e}").into());
                }
            }
        }
    });

    app.global::<SaveView>().on_folder_grouped_toggled({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |grouped| {
            let Some(app) = app_weak.upgrade() else { return };
            // Only the folder model depends on grouping — no rebake.
            let st = state.borrow();
            let l = match st.save_view_source {
                1 => st.replay_loaded.as_ref(),
                2 => st.pvp_local_loaded.as_ref(),
                3 => st.pvp_remote_loaded.as_ref(),
                _ => st.loaded.as_ref(),
            };
            if let Some(l) = l {
                app.global::<SaveView>().set_folder_chips(ModelRc::new(VecModel::from(loaded::folder_rows(l, grouped))));
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

            // Stats need a full decode — served from the background
            // worker's cache when it already has them, else computed
            // off-thread and folded in when they land (if still selected).
            let path = replay.path.clone();
            st.replay_detail_path = Some(path.clone());
            st.replay_detail_lines = lines;
            // Embedded save view: decode the replay's local SRAM
            // off-thread; the fold bakes the Loaded (UI-thread images).
            if st.replay_loaded_path.as_deref() != Some(path.as_path()) {
                let sram_path = path.clone();
                let sram_tx = stats_tx.clone();
                std::thread::spawn(move || {
                    let sram = std::fs::File::open(&sram_path)
                        .map_err(anyhow::Error::from)
                        .and_then(|f| Ok(tango_pvp::replay::Replay::decode(f)?.local_sram));
                    match sram {
                        Ok(sram) => {
                            let _ = sram_tx.send(Event::ReplayLocalSram { path: sram_path, sram });
                        }
                        Err(e) => log::warn!("{}: local sram decode failed: {e}", sram_path.display()),
                    }
                });
            }
            if let Some(stats) = st.replay_stats.get(&path).copied() {
                let tx = stats_tx.clone();
                let _ = tx.send(Event::ReplayStats { path, stats });
            } else {
                let tx = stats_tx.clone();
                std::thread::spawn(move || match replays::compute_stats(&path) {
                    Ok(stats) => {
                        let _ = tx.send(Event::ReplayStats { path, stats });
                    }
                    Err(e) => log::warn!("{}: stats failed: {e}", path.display()),
                });
            }
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

    app.on_patch_list_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Ok(index) = usize::try_from(index) else { return };
            push_patch_detail(&app, &mut st, index);
        }
    });

    app.on_patch_detail_version_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |_index| {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
            push_patch_supported_games(&app, &st);
        }
    });

    app.on_patch_search_edited({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |text| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.patch_filter = text.trim().to_lowercase();
            apply_patch_filter(&app, &mut st);
        }
    });

    app.on_patch_update_clicked({
        let state = state.clone();
        let app_weak = app.as_weak();
        let rt = tokio_runtime.handle().clone();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            // Defense in depth behind the button disabling itself.
            if st.patch_updating {
                return;
            }
            st.patch_updating = true;
            st.patch_update_error = None;
            refresh_patch_status(&app, &st);
            let url = st.config.patch_repo_url();
            let root = st.config.patches_path();
            let tx = patch_update_tx.clone();
            rt.spawn(async move {
                let result = patch::update(url, root).await.map_err(|e| e.to_string());
                let _ = tx.send(Event::PatchUpdateDone { result, background: false });
            });
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
                    st.session_start = Some(std::time::SystemTime::now());
                    // Key releases outside a focused FocusScope are
                    // never delivered — start from a clean keyboard
                    // state (gamepad state is polled and stays valid).
                    st.held.clear_keys();
                    st.speed_up = false;
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
            let mut text = app.get_link_code().trim().to_string();
            if text.is_empty() {
                // Empty input generates a memorable random code (and
                // puts it on the clipboard for the opponent DM), like
                // tango's Fight button.
                text = randomcode::generate(&st.config.language);
                app.set_link_code(text.clone().into());
                copy_text_to_clipboard(&text);
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
                    st.session_start = Some(std::time::SystemTime::now());
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
            st.session_start = None;
            st.pvp_local_loaded = None;
            st.pvp_remote_loaded = None;
            app.set_session_drawer(0);
            st.held.clear_keys();
            st.speed_up = false;
            app.set_in_session(false);
            app.set_session_confirm_exit(false);
            app.set_pvp_reconnecting(false);
            app.set_frame(Image::default());
        }
    };

    // Tearing a *live* PvP match down is gated behind the confirm
    // modal (session/view's disconnect modal); everything else — and a
    // match that's already wound down — closes immediately.
    let request_end_session = {
        let state = state.clone();
        let app_weak = app.as_weak();
        let end_session = end_session.clone();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let confirm = {
                let st = state.borrow();
                matches!(&st.session, Some(ActiveSession::Pvp(p)) if !p.is_ended())
            };
            if confirm && !app.get_session_confirm_exit() {
                app.set_session_confirm_exit(true);
            } else {
                end_session();
            }
        }
    };

    app.on_stop_clicked(request_end_session.clone());
    app.on_exit_confirmed(end_session.clone());

    app.on_session_pointer_moved({
        let state = state.clone();
        move || {
            state.borrow_mut().session_last_pointer = std::time::Instant::now();
        }
    });

    app.on_session_drawer_changed({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |drawer| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.save_view_source = match drawer {
                1 => 2,
                2 => 3,
                _ => {
                    if app.get_active_tab() == 1 {
                        1
                    } else {
                        0
                    }
                }
            };
            push_save_view(&app, &st);
        }
    });

    app.on_pvp_frame_delay_changed({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |value| {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
            let frames = tango_pvp::battle::MIN_FRAME_DELAY
                + (value.clamp(0.0, 1.0)
                    * (tango_pvp::battle::MAX_FRAME_DELAY - tango_pvp::battle::MIN_FRAME_DELAY) as f32)
                    .round() as u32;
            if let Some(ActiveSession::Pvp(p)) = &st.session {
                p.set_frame_delay(frames);
            }
            app.set_pvp_frame_delay_label(frames.to_string().into());
        }
    });

    // Key events arrive from two FocusScopes that never coexist: the
    // session view's (in-session) and the Input settings pane's (which
    // feeds the capture flow + the live key-lights — no session can be
    // active while settings are visible).
    app.on_key_event({
        let state = state.clone();
        let app_weak = app.as_weak();
        let request_end_session = request_end_session.clone();
        move |text, pressed| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some(c) = text.as_str().chars().next() else { return };
            // Escape is not bindable: it cancels an in-flight binding
            // capture, peels the disconnect-confirm modal, else asks to
            // end the session (a live PvP match confirms first).
            if c == char::from(slint::platform::Key::Escape) {
                if !pressed {
                    return;
                }
                if st.input_capture.take().is_some() {
                    refresh_input_ui(&app, &mut st);
                } else if app.get_session_settings_open() {
                    // Escape peels overlays before it asks to end.
                    app.set_session_settings_open(false);
                } else if app.get_session_confirm_exit() {
                    app.set_session_confirm_exit(false);
                } else if st.session.is_some() {
                    drop(st);
                    request_end_session();
                }
                return;
            }
            // Replay seek shortcuts (session/view's transport keys):
            // arrows ±5 s keeping the play state, ,/. single-frame
            // steps (pausing). Handled before binding resolution —
            // replays take no joyflags, so these keys are free here.
            if let Some(ActiveSession::Replay(session)) = &st.session {
                let is_seek_key = c == char::from(slint::platform::Key::LeftArrow)
                    || c == char::from(slint::platform::Key::RightArrow)
                    || c == ','
                    || c == '.';
                if is_seek_key {
                    if pressed {
                        let (delta, frame_step) = if c == char::from(slint::platform::Key::LeftArrow) {
                            (-300i64, false)
                        } else if c == char::from(slint::platform::Key::RightArrow) {
                            (300, false)
                        } else if c == ',' {
                            (-1, true)
                        } else {
                            (1, true)
                        };
                        let cur = session.current_tick() as i64;
                        let target = (cur + delta).clamp(0, session.total_ticks() as i64) as u32;
                        if frame_step {
                            session.set_paused(true);
                            session.seek_to(target, false);
                            app.set_replay_paused(true);
                        } else {
                            session.seek_to(target, !session.is_paused());
                        }
                    }
                    return;
                }
            }
            let Some(name) = input::key_name(c) else { return };
            let was_held = st.held.is_key_held(&name);
            st.held.set_key(&name, pressed);
            // Capture armed: the next fresh press binds instead of
            // reaching the (nonexistent) session.
            if let Some(k) = st.input_capture {
                if pressed && !was_held {
                    st.input_capture = None;
                    let binding = input::Binding::Key(name);
                    let slot = st.config.input_mapping.slot_mut(k);
                    if !slot.contains(&binding) {
                        slot.push(binding);
                    }
                    st.config.save();
                    refresh_input_ui(&app, &mut st);
                }
                return;
            }
            // Replays take no joyflags; a fresh press of any
            // Select-bound key doubles as pause toggle (and
            // play-at-end = rewind to the start). Keyboard-only —
            // gamepad stays joyflags + speed-up.
            if let Some(ActiveSession::Replay(session)) = &st.session {
                let is_select = st
                    .config
                    .input_mapping
                    .select
                    .iter()
                    .any(|b| matches!(b, input::Binding::Key(n) if *n == name));
                if is_select && pressed && !was_held {
                    let paused = if session.is_complete() && session.is_paused() {
                        session.seek_to(0, true);
                        false
                    } else {
                        let paused = !session.is_paused();
                        session.set_paused(paused);
                        paused
                    };
                    app.set_replay_paused(paused);
                }
            }
            sync_session_input(&mut st);
        }
    });

    // Input settings pane: key selection, chip removal, the capture
    // flow (its keyboard side lands in on_key_event above; the
    // gamepad side in the timer's gilrs poll below), reset. Every
    // mapping change persists immediately.
    app.on_input_key_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |idx| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.input_selected = input::MappedKey::ALL.get(idx as usize).copied();
            // Switching keys mid-capture doesn't retarget it — drop it.
            st.input_capture = None;
            refresh_input_ui(&app, &mut st);
        }
    });

    app.on_input_binding_remove({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |idx| {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            let Some(k) = st.input_selected else { return };
            let slot = st.config.input_mapping.slot_mut(k);
            if (idx as usize) < slot.len() {
                slot.remove(idx as usize);
                st.config.save();
            }
            refresh_input_ui(&app, &mut st);
        }
    });

    app.on_input_capture_start({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.input_capture = st.input_selected;
            refresh_input_ui(&app, &mut st);
        }
    });

    app.on_input_capture_cancel({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.input_capture = None;
            refresh_input_ui(&app, &mut st);
        }
    });

    app.on_input_reset({
        let state = state.clone();
        let app_weak = app.as_weak();
        move || {
            let Some(app) = app_weak.upgrade() else { return };
            let mut st = state.borrow_mut();
            st.config.input_mapping = input::Mapping::default();
            st.input_capture = None;
            st.config.save();
            refresh_input_ui(&app, &mut st);
        }
    });

    // Gamepad input. gilrs stays on the UI thread (its platform
    // backends aren't Send) — created here, polled by the frame timer
    // below. No backend is non-fatal: keyboard input still works, the
    // Input pane just never sees gamepad events.
    let mut gilrs = match gilrs::Gilrs::new() {
        Ok(g) => {
            for (_id, gamepad) in g.gamepads() {
                log::info!("gamepad connected: {}", gamepad.name());
            }
            Some(g)
        }
        Err(e) => {
            log::warn!("gamepad support unavailable: {e}");
            None
        }
    };

    // Frame pump + event fold, ~60 Hz. Cheap when idle: a gilrs poll,
    // a try_recv and a dirty-flag check.
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), {
        let state = state.clone();
        let app_weak = app.as_weak();
        let rt = tokio_runtime.handle().clone();
        let end_session = end_session.clone();
        let spawn_scan = spawn_scan.clone();
        move || {
            let Some(app) = app_weak.upgrade() else { return };

            // Gamepad poll: fold this tick's gilrs events into the
            // held state (and the capture flow, when the Input pane
            // has one armed), then recompute the session joyflags
            // once if anything moved. The input pane refresh runs
            // every tick — it's diffed, so idle ticks push nothing.
            {
                let mut st = state.borrow_mut();
                let mut moved = false;
                if let Some(gilrs) = gilrs.as_mut() {
                    while let Some(ev) = gilrs.next_event() {
                        match ev.event {
                            gilrs::EventType::Connected => {
                                log::info!("gamepad connected: {}", gilrs.gamepad(ev.id).name());
                            }
                            gilrs::EventType::Disconnected => {
                                log::info!("gamepad disconnected");
                                st.held.clear_gamepad();
                                moved = true;
                            }
                            gilrs::EventType::ButtonPressed(b, _) => {
                                let Some(name) = input::button_name(b) else { continue };
                                if let Some(k) = st.input_capture.take() {
                                    let binding = input::Binding::Button(name.clone());
                                    let slot = st.config.input_mapping.slot_mut(k);
                                    if !slot.contains(&binding) {
                                        slot.push(binding);
                                    }
                                    st.config.save();
                                }
                                st.held.set_button(&name, true);
                                moved = true;
                            }
                            gilrs::EventType::ButtonReleased(b, _) => {
                                let Some(name) = input::button_name(b) else { continue };
                                st.held.set_button(&name, false);
                                moved = true;
                            }
                            gilrs::EventType::AxisChanged(a, value, _) => {
                                let Some(name) = input::axis_name(a) else { continue };
                                // Captures bind on the threshold
                                // *crossing*, so a stick resting
                                // off-center can't insta-bind.
                                if st.input_capture.is_some()
                                    && value.abs() > input::AXIS_THRESHOLD
                                    && st.held.axis(&name).abs() <= input::AXIS_THRESHOLD
                                {
                                    let k = st.input_capture.take().expect("checked above");
                                    let binding = input::Binding::Axis {
                                        axis: name.clone(),
                                        dir: if value > 0.0 { 1 } else { -1 },
                                    };
                                    let slot = st.config.input_mapping.slot_mut(k);
                                    if !slot.contains(&binding) {
                                        slot.push(binding);
                                    }
                                    st.config.save();
                                }
                                st.held.set_axis(&name, value);
                                moved = true;
                            }
                            _ => {}
                        }
                    }
                }
                if moved {
                    sync_session_input(&mut st);
                }
                refresh_input_ui(&app, &mut st);
                // The SaveView global is shared by the play + replays
                // tabs and the session drawers; outside a session the
                // visible tab owns it (the drawers switch explicitly).
                if !app.get_in_session() {
                    let wanted_source = if app.get_active_tab() == 1 { 1 } else { 0 };
                    if wanted_source != st.save_view_source {
                        st.save_view_source = wanted_source;
                        push_save_view(&app, &st);
                    }
                }
                update_discord_presence(&app, &st);
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                {
                    let lang = &st.config.language;
                    let (line, ready) = if !st.config.enable_updater {
                        (String::new(), false)
                    } else {
                        match st.updater.status_blocking() {
                            updater::Status::UpToDate { release: None } => (t!(lang, "updater-loading"), false),
                            updater::Status::UpToDate { release: Some(_) } => (
                                t!(lang, "updater-up-to-date", version = env!("CARGO_PKG_VERSION")),
                                false,
                            ),
                            updater::Status::UpdateAvailable { release } => (
                                t!(lang, "updater-latest-version", version = release.version.to_string()),
                                false,
                            ),
                            updater::Status::Downloading { current, total, .. } => (
                                t!(
                                    lang,
                                    "updater-downloading",
                                    pct = (if total > 0 { current * 100 / total } else { 0 }) as i64
                                ),
                                false,
                            ),
                            updater::Status::ReadyToUpdate { .. } => {
                                (t!(lang, "updater-ready-to-update"), true)
                            }
                        }
                    };
                    app.set_about_updater_status(line.into());
                    app.set_about_update_ready(ready);
                }
                // Accepting a Discord "Ask to Join" drops the code
                // into Play, ready to Fight (tango's join handler).
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                if let Some(code) = st.discord.take_current_join_secret() {
                    app.set_link_code(code.into());
                    app.set_active_tab(0);
                }
            }

            while let Ok(event) = rx.try_recv() {
                match event {
                    Event::ScanDone {
                        roms,
                        saves,
                        replays,
                        patches,
                    } => {
                        let pending = {
                            let mut st = state.borrow_mut();
                            st.roms = roms;
                            st.saves = saves;
                            st.replay_rows = replays;
                            st.patches = patches;
                            refresh_models(&app, &mut st);
                            // Background stats sweep (tango's lazy stats
                            // worker): decode every replay we don't have
                            // stats for yet, oldest scan wins by path.
                            let missing: Vec<std::path::PathBuf> = st
                                .replay_rows
                                .iter()
                                .map(|r| r.path.clone())
                                .filter(|p| !st.replay_stats.contains_key(p))
                                .collect();
                            if !missing.is_empty() {
                                let tx = stats_sweep_tx.clone();
                                std::thread::spawn(move || {
                                    for path in missing {
                                        match replays::compute_stats(&path) {
                                            Ok(stats) => {
                                                let _ = tx.send(Event::ReplayStats { path, stats });
                                            }
                                            Err(e) => log::warn!("{}: stats failed: {e}", path.display()),
                                        }
                                    }
                                });
                            }
                            st.pending_select_save.take()
                        };
                        // A save-management op wants its result focused:
                        // re-point the pickers at the new file. The
                        // selection callbacks re-borrow the state, so
                        // the borrow above must be dropped first.
                        if let Some((game, path)) = pending {
                            let gi = state.borrow().game_rows.iter().position(|g| *g == game);
                            if let Some(gi) = gi {
                                app.set_selected_game(gi as i32);
                                app.invoke_game_selected(gi as i32);
                                let si = state.borrow().save_rows.iter().position(|s| s.path == path);
                                if let Some(si) = si {
                                    app.set_selected_save(si as i32);
                                    app.invoke_save_selected(si as i32);
                                }
                            }
                        }
                    }
                    Event::ReplayLocalSram { path, sram } => {
                        let mut st = state.borrow_mut();
                        if st.replay_detail_path.as_deref() != Some(path.as_path()) {
                            continue;
                        }
                        let row = st.replay_rows.iter().find(|r| r.path == path);
                        let side = row.and_then(|r| r.metadata.local_side.clone());
                        let patches_path = st.config.patches_path();
                        let built = resolve_replay_rom(&st.roms, &patches_path, side.as_ref())
                            .ok()
                            .and_then(|(game, rom)| {
                                let save = game.parse_save(&sram).ok()?;
                                // The resolved ROM already has the side's
                                // patch applied, so no patch pass here
                                // (matches what playback boots).
                                Some(loaded::Loaded::build(game, &rom, save, &patches_path, None))
                            });
                        match built {
                            Some(l) => {
                                st.replay_loaded = Some(l);
                                st.replay_loaded_path = Some(path);
                            }
                            None => {
                                st.replay_loaded = None;
                                st.replay_loaded_path = None;
                            }
                        }
                        if st.save_view_source == 1 {
                            push_save_view(&app, &st);
                        }
                    }
                    Event::ReplayStats { path, stats } => {
                        let mut st = state.borrow_mut();
                        let newly_known = st.replay_stats.insert(path.clone(), stats).is_none();
                        // A newly-known incomplete replay may need hiding
                        // (only when the toggle is off and it's shown).
                        if newly_known && !stats.is_complete && !app.get_replay_show_incomplete() {
                            let family = usize::try_from(app.get_selected_replay_filter())
                                .ok()
                                .and_then(|i| i.checked_sub(1))
                                .and_then(|i| st.replay_filter_families.get(i))
                                .cloned();
                            apply_replay_filter(&app, &mut st, family.as_deref());
                        }
                        let st = &*st;
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
                    Event::ExportProgress { current, total } => {
                        app.set_replay_export_progress(if total > 0 {
                            current as f32 / total as f32
                        } else {
                            0.0
                        });
                    }
                    Event::ExportDone { result } => {
                        let mut st = state.borrow_mut();
                        st.export_canceller = None;
                        app.set_replay_exporting(false);
                        let lang = &st.config.language;
                        match result {
                            Ok(path) => {
                                app.set_replay_export_status(t!(lang, "replays-export-success").into());
                                st.export_output = Some(path);
                                app.set_replay_export_has_output(true);
                            }
                            Err(e) => {
                                app.set_replay_export_status(
                                    t!(lang, "replays-export-error", error = e).into(),
                                );
                            }
                        }
                    }
                    Event::PatchUpdateDone { result, background } => {
                        // Fold the sync result into the Patches tab
                        // status, then rescan on success so the new /
                        // updated patches appear everywhere (the Play
                        // pickers included). spawn_scan re-borrows the
                        // state RefCell — the borrow must drop first.
                        let rescan = {
                            let mut st = state.borrow_mut();
                            if !background {
                                st.patch_updating = false;
                            }
                            let rescan = match result {
                                Ok(()) => {
                                    st.patch_last_updated = Some(chrono::Local::now());
                                    // A successful sync (either path)
                                    // supersedes any earlier failure.
                                    st.patch_update_error = None;
                                    true
                                }
                                Err(e) => {
                                    log::warn!("patch update failed: {e}");
                                    if !background {
                                        st.patch_update_error = Some(e);
                                    }
                                    false
                                }
                            };
                            refresh_patch_status(&app, &st);
                            rescan
                        };
                        if rescan {
                            spawn_scan();
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
                                // Seed the telemetry footer's frame-delay
                                // slider from the session's live value
                                // (set once; per-tick writes would fight
                                // the user's drag).
                                let fd = session.frame_delay();
                                app.set_pvp_frame_delay(
                                    (fd - tango_pvp::battle::MIN_FRAME_DELAY) as f32
                                        / (tango_pvp::battle::MAX_FRAME_DELAY - tango_pvp::battle::MIN_FRAME_DELAY)
                                            as f32,
                                );
                                app.set_pvp_frame_delay_label(fd.to_string().into());
                                app.set_session_confirm_exit(false);
                                app.set_pvp_reconnecting(false);
                                // Bake the setup drawers' save views from
                                // the committed SRAM (the ROMs are already
                                // patched); a blinded opponent bakes nothing.
                                let patches_path = st.config.patches_path();
                                st.pvp_local_loaded = session
                                    .setup
                                    .local_game
                                    .parse_save(&session.setup.local_save_data)
                                    .ok()
                                    .map(|save| {
                                        loaded::Loaded::build(
                                            session.setup.local_game,
                                            &session.setup.local_rom,
                                            save,
                                            &patches_path,
                                            None,
                                        )
                                    });
                                st.pvp_remote_loaded = if session.setup.remote_blind {
                                    None
                                } else {
                                    session
                                        .setup
                                        .remote_game
                                        .parse_save(&session.setup.remote_save_data)
                                        .ok()
                                        .map(|save| {
                                            loaded::Loaded::build(
                                                session.setup.remote_game,
                                                &session.setup.remote_rom,
                                                save,
                                                &patches_path,
                                                None,
                                            )
                                        })
                                };
                                app.set_pvp_opponent_blind(st.pvp_remote_loaded.is_none());
                                // Auto-open the opponent's setup at match
                                // start when the setting asks for it.
                                if st.pvp_remote_loaded.is_some() && st.config.show_opponent_setup {
                                    app.set_session_drawer(2);
                                    st.save_view_source = 3;
                                    push_save_view(&app, &st);
                                } else {
                                    app.set_session_drawer(0);
                                }
                                st.session = Some(ActiveSession::Pvp(Box::new(session)));
                                st.session_start = Some(std::time::SystemTime::now());
                                st.held.clear_keys();
                                st.speed_up = false;
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
                    // CPU video filter (video.rs): scale/shape the frame
                    // before upload; passthrough costs nothing.
                    let filter = video::Filter::from_config(&st.config.video_filter);
                    match filter.apply(
                        pixels.as_bytes(),
                        session::SCREEN_WIDTH,
                        session::SCREEN_HEIGHT,
                    ) {
                        Some((fw, fh, buf)) => {
                            let mut fp = SharedPixelBuffer::<Rgba8Pixel>::new(fw, fh);
                            fp.make_mut_bytes().copy_from_slice(&buf);
                            app.set_frame(Image::from_rgba8(fp));
                        }
                        None => app.set_frame(Image::from_rgba8(pixels)),
                    }
                }
                let pinned = app.get_session_settings_open()
                    || app.get_session_confirm_exit()
                    || app.get_pvp_reconnecting()
                    || app.get_session_drawer() != 0
                    || matches!(session, ActiveSession::Replay(r) if r.is_paused());
                let controls_visible =
                    pinned || st.session_last_pointer.elapsed() < std::time::Duration::from_millis(2500);
                if controls_visible != app.get_session_controls_visible() {
                    app.set_session_controls_visible(controls_visible);
                }
                if let ActiveSession::Pvp(p) = session {
                    // Self-close once the match has wound down (completion +
                    // peer EndOfMatch / disconnect / grace — see
                    // PvpSession::is_ended); actual teardown happens below,
                    // after this borrow drops.
                    pvp_ended = p.is_ended();
                    // Telemetry footer: player tag, tps, median ping,
                    // skew/lead — the condensed session/view HUD.
                    let mut line = format!("P{} · {:.1} tps", p.local_player_index() + 1, p.tps());
                    if let Some(l) = p.latency() {
                        line += &format!(" · {} ms", l.as_millis());
                    }
                    if let Some(rs) = p.round_stats() {
                        line += &format!(" · skew {:+} · lead {:+}", rs.skew, rs.lead);
                    }
                    app.set_pvp_stats(line.into());
                    // Reconnect overlay state (the session pauses the
                    // emulator itself while the transport rebuilds).
                    app.set_pvp_reconnecting(p.is_reconnecting());
                    app.set_pvp_reconnect_progress(p.reconnect_progress().unwrap_or(0.0));
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
                    // Patches tab: list + detail (first patch, when
                    // any are installed).
                    87 => app.set_active_tab(2),
                    89 => {
                        if !state.borrow().patch_list_rows.is_empty() {
                            app.set_selected_patch_item(0);
                            app.invoke_patch_list_selected(0);
                        }
                    }
                    93 => snapshot(&app, &dir.join("ui-patches.png")),
                    95 => app.set_active_tab(3),
                    105 => snapshot(&app, &dir.join("ui-settings.png")),
                    // Input section with A selected, so the shot shows
                    // the console shell + A's default binding chips.
                    107 => {
                        app.set_settings_section(3);
                        app.invoke_input_key_selected(input::MappedKey::A.index() as i32);
                    }
                    117 => snapshot(&app, &dir.join("ui-settings-input.png")),
                    // Save-view sections beyond Folder, each on the
                    // first save that actually exposes the view (a BN6
                    // link-navi save legitimately has none of them).
                    119 => app.set_active_tab(0),
                    120 => select_save_where(&app, &state, &|s| s.view_navicust().is_some()),
                    130 => {
                        if save_tab_index_of_kind(&app, 0).is_some() {
                            snapshot(&app, &dir.join("ui-save-navicust.png"));
                        }
                    }
                    132 => select_save_where(&app, &state, &|s| s.view_auto_battle_data().is_some()),
                    140 => {
                        if let Some(ti) = save_tab_index_of_kind(&app, 3) {
                            app.global::<SaveView>().set_save_active_tab(ti);
                        }
                    }
                    145 => {
                        if save_tab_index_of_kind(&app, 3).is_some() {
                            snapshot(&app, &dir.join("ui-save-abd.png"));
                        }
                    }
                    147 => select_save_where(&app, &state, &|s| s.view_patch_cards().is_some()),
                    155 => {
                        if let Some(ti) = save_tab_index_of_kind(&app, 2) {
                            app.global::<SaveView>().set_save_active_tab(ti);
                        }
                    }
                    160 => {
                        if save_tab_index_of_kind(&app, 2).is_some() {
                            snapshot(&app, &dir.join("ui-save-patch-cards.png"));
                        }
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
