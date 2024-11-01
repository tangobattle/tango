use crate::{config, gui, i18n, patch, sync, updater};
use fluent_templates::Loader;

pub struct State {
    tab: Tab,
    patch_selection: Option<String>,
    play_pane: gui::play_pane::State,
    patches_pane: gui::patches_pane::State,
    replays_pane: gui::replays_pane::State,
    updater: Option<gui::updater_window::State>,
}

impl State {
    pub fn new(selection: Option<gui::save_select_view::Selection>, updater: bool) -> Self {
        Self {
            tab: Tab::Play,
            patch_selection: None,
            play_pane: gui::play_pane::State::new(selection),
            patches_pane: gui::patches_pane::State::new(),
            replays_pane: gui::replays_pane::State::new(),
            updater: if updater {
                Some(gui::updater_window::State::new())
            } else {
                None
            },
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
    config: &mut config::Config,
    shared_root_state: &mut gui::SharedRootState,
    window: &winit::window::Window,
    show_settings: &mut Option<gui::settings_window::State>,
    state: &mut State,
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
                                .selectable_label(state.updater.is_some(), "🆕")
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
                                state.updater = if state.updater.is_none() {
                                    Some(gui::updater_window::State::new())
                                } else {
                                    None
                                };
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
                                    let patches_scanner = shared_root_state.scanners.patches.clone();
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

    if state.updater.is_some() {
        gui::updater_window::show(ctx, &mut state.updater, &config.language, updater);
    }

    // If a join is requested, switch immediately to the play tab.
    if shared_root_state.discord_client.has_current_join_secret() || init_link_code.is_some() {
        state.tab = Tab::Play;
    }

    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(ctx.style().visuals.window_fill())
                .inner_margin(egui::Margin::same(0.0)),
        )
        .show(ctx, |ui| match state.tab {
            Tab::Play => {
                gui::play_pane::show(
                    ui,
                    config,
                    shared_root_state,
                    window,
                    &mut state.patch_selection,
                    &mut state.play_pane,
                    init_link_code,
                );
            }
            Tab::Replays => {
                gui::replays_pane::show(ui, config, shared_root_state, &mut state.replays_pane);
            }
            Tab::Patches => {
                let patches_path = config.patches_path().clone();
                gui::patches_pane::show(
                    ui,
                    config,
                    shared_root_state,
                    &mut state.patches_pane,
                    &mut state.patch_selection,
                    &patches_path,
                );
            }
        });
}
