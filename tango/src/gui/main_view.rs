use fluent_templates::Loader;

use crate::{audio, config, gui, i18n, patch, session, stats};

pub struct State {
    tab: Tab,
    patch_selection: Option<String>,
    play_pane: gui::play_pane::State,
    patches_pane: gui::patches_pane::State,
    replays_pane: gui::replays_pane::State,
}

impl State {
    pub fn new() -> Self {
        Self {
            tab: Tab::Play,
            patch_selection: None,
            play_pane: gui::play_pane::State::new(),
            patches_pane: gui::patches_pane::State::new(),
            replays_pane: gui::replays_pane::State::new(),
        }
    }
}

#[derive(PartialEq)]
enum Tab {
    Play,
    Patches,
    Replays,
}

pub fn show(
    ctx: &egui::Context,
    font_families: &gui::FontFamilies,
    config: &mut config::Config,
    config_arc: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    handle: tokio::runtime::Handle,
    window: &glutin::window::Window,
    show_settings: &mut Option<gui::settings_window::State>,
    replay_dump_windows: &mut gui::replay_dump_windows::State,
    clipboard: &mut arboard::Clipboard,
    audio_binder: audio::LateBinder,
    roms_scanner: gui::ROMsScanner,
    saves_scanner: gui::SavesScanner,
    patches_scanner: gui::PatchesScanner,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    selection: std::sync::Arc<parking_lot::Mutex<Option<gui::Selection>>>,
    state: &mut State,
) {
    egui::TopBottomPanel::top("main-top-panel").show(ctx, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui
                        .selectable_label(show_settings.is_some(), "⚙️")
                        .on_hover_text_at_pointer(
                            i18n::LOCALES.lookup(&config.language, "settings").unwrap(),
                        )
                        .clicked()
                    {
                        *show_settings = if show_settings.is_none() {
                            Some(gui::settings_window::State::new())
                        } else {
                            None
                        };
                    }
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                            ui.set_width(ui.available_width());

                            ui.selectable_value(&mut state.tab, Tab::Play, "🎮")
                                .on_hover_text_at_pointer(
                                    i18n::LOCALES.lookup(&config.language, "play").unwrap(),
                                );

                            if ui
                                .selectable_value(&mut state.tab, Tab::Replays, "📽️")
                                .on_hover_text_at_pointer(
                                    i18n::LOCALES.lookup(&config.language, "replays").unwrap(),
                                )
                                .clicked()
                            {
                                state.replays_pane.rescan(&config.replays_path());
                            }

                            if ui
                                .selectable_value(&mut state.tab, Tab::Patches, "🩹")
                                .on_hover_text_at_pointer(
                                    i18n::LOCALES.lookup(&config.language, "patches").unwrap(),
                                )
                                .clicked()
                            {
                                rayon::spawn({
                                    let patches_scanner = patches_scanner.clone();
                                    let patches_path = config.patches_path();
                                    move || {
                                        patches_scanner.rescan(move || {
                                            Some(patch::scan(&patches_path).unwrap_or_default())
                                        });
                                    }
                                });
                            }
                        });
                    });
                });
            });
        });
    });

    egui::CentralPanel::default().show(ctx, |ui| match state.tab {
        Tab::Play => {
            gui::play_pane::show(
                ui,
                handle.clone(),
                &font_families,
                window,
                clipboard,
                config,
                config_arc,
                roms_scanner.clone(),
                saves_scanner.clone(),
                patches_scanner.clone(),
                audio_binder.clone(),
                session.clone(),
                selection.clone(),
                &mut state.patch_selection,
                emu_tps_counter.clone(),
                &mut state.play_pane,
            );
        }
        Tab::Replays => {
            gui::replays_pane::show(
                ui,
                clipboard,
                &font_families,
                &mut state.replays_pane,
                replay_dump_windows,
                &config.language,
                &config.patches_path(),
                patches_scanner.clone(),
                roms_scanner.clone(),
                &config.replays_path(),
                audio_binder.clone(),
                emu_tps_counter.clone(),
                session.clone(),
            );
        }
        Tab::Patches => {
            gui::patches_pane::show(
                ui,
                &mut state.patches_pane,
                &config.language,
                if !config.patch_repo.is_empty() {
                    config.patch_repo.as_str()
                } else {
                    config::DEFAULT_PATCH_REPO
                },
                &mut state.patch_selection,
                &config.patches_path(),
                patches_scanner.clone(),
            );
        }
    });
}
