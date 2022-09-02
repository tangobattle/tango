use fluent_templates::Loader;

use crate::{game, gui, i18n};

pub struct State {
    selection: Option<String>,
}

impl State {
    pub fn new(selection: Option<String>) -> Self {
        Self { selection }
    }
}

pub struct PatchesWindow {}

impl PatchesWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        show: &mut Option<State>,
        language: &unic_langid::LanguageIdentifier,
        patches_path: &std::path::Path,
        patches_scanner: gui::PatchesScanner,
    ) {
        let mut show_window = show.is_some();
        egui::Window::new(format!(
            "🩹 {}",
            i18n::LOCALES.lookup(language, "patches").unwrap()
        ))
        .id(egui::Id::new("patches-window"))
        .resizable(true)
        .min_width(400.0)
        .default_width(600.0)
        .open(&mut show_window)
        .show(ctx, |ui| {
            let state = show.as_mut().unwrap();

            ui.horizontal_top(|ui| {
                ui.add_enabled_ui(!patches_scanner.is_scanning(), |ui| {
                    if ui
                        .button(i18n::LOCALES.lookup(language, "patches.update").unwrap())
                        .clicked()
                    {
                        // TODO
                    }
                });
            });

            ui.separator();

            let patches = patches_scanner.read();
            ui.horizontal_top(|ui| {
                egui::ScrollArea::vertical()
                    .max_width(200.0)
                    .auto_shrink([false, false])
                    .id_source("patch-window-left")
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            for (name, _) in patches.iter() {
                                if ui
                                    .selectable_label(state.selection.as_ref() == Some(name), name)
                                    .clicked()
                                {
                                    state.selection = Some(name.to_owned());
                                }
                            }
                        });
                    });

                let patch =
                    if let Some(patch) = state.selection.as_ref().and_then(|n| patches.get(n)) {
                        patch
                    } else {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .id_source("patch-window-right-empty")
                            .show(ui, |_ui| {});
                        return;
                    };

                ui.with_layout(
                    egui::Layout::top_down_justified(egui::Align::Min).with_main_justify(true),
                    |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let latest_version_and_info =
                                patch.versions.iter().max_by_key(|(k, _)| *k);

                            ui.vertical(|ui| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Min),
                                    |ui| {
                                        if ui
                                            .button(
                                                i18n::LOCALES
                                                    .lookup(language, "patches.open-folder")
                                                    .unwrap(),
                                            )
                                            .clicked()
                                        {
                                            let _ = open::that(&patch.path);
                                        }

                                        ui.with_layout(
                                            egui::Layout::top_down_justified(egui::Align::Min),
                                            |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.with_layout(
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Max,
                                                        )
                                                        .with_main_wrap(true),
                                                        |ui| {
                                                            ui.heading(&patch.title);
                                                            if let Some((version, _)) =
                                                                latest_version_and_info.as_ref()
                                                            {
                                                                ui.label(version.to_string());
                                                            }
                                                        },
                                                    );
                                                });
                                            },
                                        );
                                    },
                                );
                                egui::Grid::new("patch-info-grid")
                                    .num_columns(2)
                                    .show(ui, |ui| {
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Min)
                                                .with_cross_justify(true),
                                            |ui| {
                                                ui.strong(
                                                    i18n::LOCALES
                                                        .lookup(language, "patches.authors")
                                                        .unwrap(),
                                                );
                                            },
                                        );
                                        ui.vertical(|ui| {
                                            for author in patch.authors.iter() {
                                                let name = author
                                                    .display_name
                                                    .as_ref()
                                                    .unwrap_or(&author.addr);
                                                if author.addr == "" {
                                                    ui.label(name);
                                                } else {
                                                    ui.hyperlink_to(
                                                        name,
                                                        format!("mailto:{}", author.addr),
                                                    );
                                                }
                                            }
                                        });
                                        ui.end_row();

                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Min)
                                                .with_cross_justify(true),
                                            |ui| {
                                                ui.strong(
                                                    i18n::LOCALES
                                                        .lookup(language, "patches.license")
                                                        .unwrap(),
                                                );
                                            },
                                        );
                                        if let Some(license) = patch.license.as_ref() {
                                            ui.label(license);
                                        } else {
                                            ui.label(
                                                i18n::LOCALES
                                                    .lookup(language, "patches.all-rights-reserved")
                                                    .unwrap(),
                                            );
                                        }
                                        ui.end_row();

                                        if let Some(source) = patch.source.as_ref() {
                                            ui.with_layout(
                                                egui::Layout::left_to_right(egui::Align::Min)
                                                    .with_cross_justify(true),
                                                |ui| {
                                                    ui.strong(
                                                        i18n::LOCALES
                                                            .lookup(language, "patches.source")
                                                            .unwrap(),
                                                    );
                                                },
                                            );
                                            ui.hyperlink_to("🌐", source);
                                            ui.end_row();
                                        }

                                        if let Some((_, version_info)) =
                                            latest_version_and_info.as_ref()
                                        {
                                            ui.with_layout(
                                                egui::Layout::left_to_right(egui::Align::Min)
                                                    .with_cross_justify(true),
                                                |ui| {
                                                    ui.strong(
                                                        i18n::LOCALES
                                                            .lookup(language, "patches.games")
                                                            .unwrap(),
                                                    );
                                                },
                                            );
                                            ui.vertical(|ui| {
                                                let mut games = version_info
                                                    .supported_games
                                                    .iter()
                                                    .cloned()
                                                    .collect::<Vec<_>>();
                                                game::sort_games(language, &mut games);
                                                for game in games.iter() {
                                                    let (family, variant) =
                                                        game.family_and_variant();
                                                    ui.label(
                                                        i18n::LOCALES
                                                            .lookup(
                                                                language,
                                                                &format!(
                                                                    "game-{}.variant-{}",
                                                                    family, variant
                                                                ),
                                                            )
                                                            .unwrap(),
                                                    );
                                                }
                                            });
                                            ui.end_row();
                                        }
                                    });
                                ui.separator();

                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .id_source("patch-window-readme")
                                    .show(ui, |ui| {
                                        ui.monospace(
                                            patch.readme.clone().unwrap_or("".to_string()),
                                        );
                                    });
                            });
                        });
                    },
                );
            });
        });
        if !show_window {
            *show = None;
        }
    }
}
