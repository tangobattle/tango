use fluent_templates::Loader;

use crate::{audio, config, discord, gui, i18n, patch, rom, save, session, stats, sync, updater};

pub struct State {
    tab: Tab,
    patch_selection: Option<String>,
    play_pane: gui::play_pane::State,
    patches_pane: gui::patches_pane::State,
    replays_pane: gui::replays_pane::State,
    show_updater: bool,
}

impl State {
    pub fn new(show_updater: bool) -> Self {
        Self {
            tab: Tab::Play,
            patch_selection: None,
            play_pane: gui::play_pane::State::new(),
            patches_pane: gui::patches_pane::State::new(),
            replays_pane: gui::replays_pane::State::new(),
            show_updater,
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
    window: &winit::window::Window,
    show_settings: &mut Option<gui::settings_window::State>,
    replay_dump_windows: &mut gui::replay_dump_windows::State,
    clipboard: &mut arboard::Clipboard,
    audio_binder: audio::LateBinder,
    roms_scanner: rom::Scanner,
    saves_scanner: save::Scanner,
    patches_scanner: patch::Scanner,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    selection: &mut Option<gui::Selection>,
    state: &mut State,
    discord_client: &mut discord::Client,
    init_link_code: &mut Option<String>,
    updater: &updater::Updater,
) {
    egui::TopBottomPanel::top("main-top-panel").show(ctx, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui
                        .selectable_label(show_settings.is_some(), "⚙️")
                        .on_hover_text_at_pointer(i18n::LOCALES.lookup(&config.language, "settings").unwrap())
                        .clicked()
                    {
                        *show_settings = if show_settings.is_none() {
                            Some(gui::settings_window::State::new())
                        } else {
                            None
                        };
                    }
                    let updater_status = sync::block_on(updater.status());
                    match updater_status {
                        updater::Status::UpToDate { .. } => {}
                        _ => {
                            if ui
                                .selectable_label(state.show_updater, "🆕")
                                .on_hover_text_at_pointer(match updater_status {
                                    updater::Status::ReadyToUpdate { .. } => i18n::LOCALES
                                        .lookup(&config.language, "updater-ready-to-update")
                                        .unwrap(),
                                    updater::Status::UpdateAvailable { .. } => i18n::LOCALES
                                        .lookup(&config.language, "updater-update-available")
                                        .unwrap(),
                                    updater::Status::Downloading { current, total, .. } => i18n::LOCALES
                                        .lookup_with_args(
                                            &config.language,
                                            "updater-downloading",
                                            &std::collections::HashMap::from([(
                                                "percent",
                                                if total > 0 {
                                                    format!("{}", current * 100 / total)
                                                } else {
                                                    "?".to_string()
                                                }
                                                .into(),
                                            )]),
                                        )
                                        .unwrap(),
                                    updater::Status::UpToDate { .. } => unreachable!(),
                                })
                                .clicked()
                            {
                                state.show_updater = !state.show_updater;
                            }
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                            ui.set_width(ui.available_width());

                            ui.selectable_value(&mut state.tab, Tab::Play, "🎮")
                                .on_hover_text_at_pointer(i18n::LOCALES.lookup(&config.language, "play").unwrap());

                            if ui
                                .selectable_value(&mut state.tab, Tab::Replays, "📽️")
                                .on_hover_text_at_pointer(i18n::LOCALES.lookup(&config.language, "replays").unwrap())
                                .clicked()
                            {
                                state.replays_pane.rescan(ui.ctx(), &config.replays_path());
                            }

                            if ui
                                .selectable_value(&mut state.tab, Tab::Patches, "🩹")
                                .on_hover_text_at_pointer(i18n::LOCALES.lookup(&config.language, "patches").unwrap())
                                .clicked()
                            {
                                let egui_ctx = ui.ctx().clone();
                                tokio::task::spawn_blocking({
                                    let patches_scanner = patches_scanner.clone();
                                    let patches_path = config.patches_path();
                                    move || {
                                        patches_scanner
                                            .rescan(move || Some(patch::scan(&patches_path).unwrap_or_default()));
                                        egui_ctx.request_repaint();
                                    }
                                });
                            }
                        });
                    });
                });
            });
        });
    });

    if state.show_updater {
        gui::updater_window::show(ctx, &mut state.show_updater, &config.language, updater);
    }

    // If a join is requested, switch immediately to the play tab.
    if discord_client.has_current_join_secret() || init_link_code.is_some() {
        state.tab = Tab::Play;
    }

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(ctx.style().visuals.window_fill())
                .inner_margin(egui::style::Margin::same(0.0)),
        )
        .show(ctx, |ui| match state.tab {
            Tab::Play => {
                gui::play_pane::show(
                    ui,
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
                    selection,
                    &mut state.patch_selection,
                    emu_tps_counter.clone(),
                    &mut state.play_pane,
                    discord_client,
                    init_link_code,
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
                let patches_path = config.patches_path().clone();
                gui::patches_pane::show(
                    ui,
                    &mut state.patches_pane,
                    &config.language,
                    if !config.patch_repo.is_empty() {
                        config.patch_repo.as_str()
                    } else {
                        config::DEFAULT_PATCH_REPO
                    },
                    &mut config.starred_patches,
                    &mut state.patch_selection,
                    &patches_path,
                    patches_scanner.clone(),
                );
            }
        });
}
