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
    },
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
    session: Option<session::SinglePlayerSession>,
    joyflags: u32,
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
        session: None,
        joyflags: 0,
    }));

    // Background scan; results come back over the channel and are folded
    // into the UI by the frame timer below.
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let st = state.borrow();
        let roms_path = st.config.roms_path();
        let saves_path = st.config.saves_path();
        std::thread::spawn(move || {
            let roms = rom::scan_roms(&roms_path);
            let saves = save::scan_saves(&saves_path);
            let _ = tx.send(Event::ScanDone { roms, saves });
        });
    }
    app.set_status("Scanning…".into());
    {
        let st = state.borrow();
        app.set_cfg_nickname(st.config.nickname.as_deref().unwrap_or("—").into());
        app.set_cfg_language(st.config.language.to_string().into());
        app.set_cfg_data_path(st.config.data_path.display().to_string().into());
    }

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
            match session::SinglePlayerSession::new(rom, &save.path, &st.audio_binder) {
                Ok(session) => {
                    st.session = Some(session);
                    st.joyflags = 0;
                    app.set_in_session(true);
                }
                Err(e) => {
                    log::error!("failed to start session: {e:?}");
                    app.set_status(format!("Failed to start: {e}").into());
                }
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
        move |text, pressed| {
            let mut st = state.borrow_mut();
            match input::classify(text.as_str()) {
                Some(input::KeyAction::Joyflag(flag)) => {
                    if pressed {
                        st.joyflags |= flag;
                    } else {
                        st.joyflags &= !flag;
                    }
                    if let Some(session) = &st.session {
                        session.set_joyflags(st.joyflags);
                    }
                }
                Some(input::KeyAction::FastForward) => {
                    if let Some(session) = &st.session {
                        session.set_speed(if pressed { 3.0 } else { 1.0 });
                    }
                }
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
                    Event::ScanDone { roms, saves } => {
                        let mut st = state.borrow_mut();
                        st.roms = roms;
                        st.saves = saves;

                        let lang = st.config.language.clone();
                        let mut game_rows: Vec<rom::GameRef> = st.roms.keys().copied().collect();
                        game_rows.sort_by_key(|g| game::display_name(&lang, *g));
                        let rows: Vec<SharedString> = game_rows
                            .iter()
                            .map(|g| game::display_name(&lang, *g).into())
                            .collect();
                        st.game_rows = game_rows;
                        st.save_rows.clear();

                        app.set_status(
                            format!(
                                "{} games · {} saves",
                                st.game_rows.len(),
                                st.saves.values().map(|v| v.len()).sum::<usize>()
                            )
                            .into(),
                        );
                        app.set_selected_game(-1);
                        app.set_selected_save(-1);
                        app.set_games(ModelRc::new(VecModel::from(rows)));
                        app.set_saves(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
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
