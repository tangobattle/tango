//! tango-ng: the next-generation Tango frontend, built on Slint so the
//! same UI can target desktop and mobile. It reuses the workspace's
//! UI-agnostic backend crates (mgba, tango-gamesupport, tango-dataview);
//! modules copied from the `tango` crate say so in their headers.
//!
//! Verification modes (instead of the interactive UI):
//! - `tango-ng --smoke [out.png]`: headless — scan, boot the first game
//!   with a save (against a temp copy of the save), emulate ~5 real
//!   seconds with audio, dump the framebuffer.
//! - `tango-ng --ui-shot [out_dir]`: open the real UI, wait for the scan,
//!   then snapshot the main screens to PNGs and exit. Run with
//!   `SLINT_BACKEND=winit-software` for reliable snapshots.

mod audio;
mod bnlc;
mod config;
mod game;
mod input;
mod patch;
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
}

/// Selectable UI languages, same set as tango's i18n::SUPPORTED_LANGS.
const LANGS: &[&str] = &[
    "en-US", "ja-JP", "zh-CN", "zh-TW", "de-DE", "es-419", "fr-FR", "nl-NL", "pt-BR", "ru-RU", "vi-VN",
];

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
    patches: patch::PatchMap,
    /// Patch names shown in the patch picker (model index i+1 — index 0
    /// is the "No patch" sentinel).
    patch_rows: Vec<String>,
    /// Versions shown in the version picker, newest first.
    version_rows: Vec<semver::Version>,
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
}

/// At most one session is active at a time.
enum ActiveSession {
    SinglePlayer(session::SinglePlayerSession),
    Replay(session::ReplaySession),
}

impl ActiveSession {
    fn frame_dirty(&self) -> bool {
        match self {
            ActiveSession::SinglePlayer(s) => s.frame_dirty(),
            ActiveSession::Replay(s) => s.frame_dirty(),
        }
    }

    fn read_frame(&self, dst_rgba: &mut [u8]) {
        match self {
            ActiveSession::SinglePlayer(s) => s.read_frame(dst_rgba),
            ActiveSession::Replay(s) => s.read_frame(dst_rgba),
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
    let mut filter_model: Vec<SharedString> = vec!["All games".into()];
    filter_model.extend(
        families
            .iter()
            .map(|f| SharedString::from(game::family_display_name(&lang, f))),
    );
    st.replay_filter_families = families;
    app.set_replay_game_filters(ModelRc::new(VecModel::from(filter_model)));
    app.set_selected_replay_filter(0);

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

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("tango-ng {}", env!("CARGO_PKG_VERSION"));

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
    let ui_shot_dir: Option<std::path::PathBuf> = (args.get(1).map(|s| s.as_str()) == Some("--ui-shot"))
        .then(|| std::path::PathBuf::from(args.get(2).map(|s| s.as_str()).unwrap_or(".")));

    let app = AppWindow::new()?;
    let shot_step = Rc::new(RefCell::new(0i32));
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
        patches: patch::PatchMap::new(),
        patch_rows: Vec::new(),
        version_rows: Vec::new(),
        session: None,
        joyflags: 0,
        replay_speed: 1.0,
        scrub_was_playing: false,
        scrub_forced: false,
    }));

    // Background scan; results come back over the channel and are folded
    // into the UI by the frame timer below. Rc so the data-path setting
    // can retrigger it.
    let (tx, rx) = std::sync::mpsc::channel();
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
        app.set_languages(ModelRc::new(VecModel::from(
            LANGS.iter().map(|l| SharedString::from(*l)).collect::<Vec<_>>(),
        )));
        app.set_app_version(format!("version {}", env!("CARGO_PKG_VERSION")).into());
        app.set_settings_nickname(st.config.nickname.as_deref().unwrap_or("").into());
        app.set_settings_data_path(st.config.data_path.display().to_string().into());
        app.set_settings_language(
            LANGS
                .iter()
                .position(|l| *l == st.config.language.to_string())
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
            let Some(lang) = LANGS.get(index as usize).and_then(|l| l.parse().ok()) else {
                return;
            };
            let mut st = state.borrow_mut();
            st.config.language = lang;
            st.config.save();
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
        let spawn_scan = spawn_scan.clone();
        move |text| {
            {
                let mut st = state.borrow_mut();
                st.config.data_path = std::path::PathBuf::from(text.as_str());
                st.config.save();
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
            let mut patch_model: Vec<SharedString> = vec!["No patch".into()];
            patch_model.extend(st.patch_rows.iter().map(|n| SharedString::from(n.as_str())));
            app.set_patches(ModelRc::new(VecModel::from(patch_model)));
            app.set_selected_patch(0);
            app.set_versions(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
            app.set_selected_version(-1);
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
        }
    });

    app.on_replay_selected({
        let state = state.clone();
        let app_weak = app.as_weak();
        move |index| {
            let Some(app) = app_weak.upgrade() else { return };
            let st = state.borrow();
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
            app.set_replay_detail(ModelRc::new(VecModel::from(lines)));
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
        move |text, pressed| {
            let mut st = state.borrow_mut();
            match input::classify(text.as_str()) {
                Some(input::KeyAction::Joyflag(flag)) => {
                    if matches!(&st.session, Some(ActiveSession::SinglePlayer(_))) {
                        if pressed {
                            st.joyflags |= flag;
                        } else {
                            st.joyflags &= !flag;
                        }
                        if let Some(ActiveSession::SinglePlayer(session)) = &st.session {
                            session.set_joyflags(st.joyflags);
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
                    None => {}
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
                }
            }

            let st = state.borrow();
            if let Some(session) = &st.session {
                if session.frame_dirty() {
                    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(
                        session::SCREEN_WIDTH,
                        session::SCREEN_HEIGHT,
                    );
                    session.read_frame(pixels.make_mut_bytes());
                    app.set_frame(Image::from_rgba8(pixels));
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
                // A few ticks between shots so layout/render settles.
                match step {
                    10 => snapshot(&app, &dir.join("ui-play-empty.png")),
                    20 => {
                        app.set_selected_game(0);
                        app.invoke_game_selected(0);
                        app.set_selected_save(0);
                    }
                    30 => snapshot(&app, &dir.join("ui-play-selected.png")),
                    40 => app.set_active_tab(1),
                    45 => {
                        if !state.borrow().replay_rows.is_empty() {
                            app.set_selected_replay(0);
                            app.invoke_replay_selected(0);
                        }
                    }
                    50 => snapshot(&app, &dir.join("ui-replays.png")),
                    60 => app.set_active_tab(3),
                    70 => {
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
