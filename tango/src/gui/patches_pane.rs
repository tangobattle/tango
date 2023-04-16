use fluent_templates::Loader;

use crate::{game, i18n, patch, sync};

pub struct State {}

impl State {
    pub fn new() -> Self {
        Self {}
    }
}

pub fn show(
    ui: &mut egui::Ui,
    _state: &mut State,
    language: &unic_langid::LanguageIdentifier,
    repo_url: &str,
    starred_patches: &mut std::collections::HashSet<String>,
    patch_selection: &mut Option<String>,
    patches_path: &std::path::Path,
    patches_scanner: patch::Scanner,
) {
    egui::TopBottomPanel::top("patches-window-top-panel").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!patches_scanner.is_scanning(), |ui| {
                if ui
                    .button(format!(
                        "🔄 {}",
                        i18n::LOCALES.lookup(language, "patches-update").unwrap()
                    ))
                    .clicked()
                {
                    let egui_ctx = ui.ctx().clone();
                    tokio::task::spawn_blocking({
                        let patches_scanner = patches_scanner.clone();
                        let repo_url = repo_url.to_owned();
                        let patches_path = patches_path.to_path_buf();
                        move || {
                            patches_scanner.rescan(move || {
                                if let Err(e) = sync::block_on(patch::update(&repo_url, &patches_path)) {
                                    log::error!("failed to update patches: {:?}", e);
                                }
                                patch::scan(&patches_path).ok()
                            });
                            egui_ctx.request_repaint();
                        }
                    });
                }
            });

            if patches_scanner.is_scanning() {
                ui.spinner();
            }
        });
    });

    let patches = patches_scanner.read();
    egui::SidePanel::left("patches-window-left-panel").show_inside(ui, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_source("patch-window-left")
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    for (name, _) in patches.iter() {
                        if ui
                            .selectable_label(patch_selection.as_ref() == Some(name), name)
                            .clicked()
                        {
                            *patch_selection = Some(name.to_owned());
                        }
                    }
                });
            });
    });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        let (patch, patch_name) =
            if let Some((patch, n)) = patch_selection.as_ref().and_then(|n| patches.get(n).map(|p| (p, n))) {
                (patch, n)
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
                    let latest_version_and_info = patch.versions.iter().max_by_key(|(k, _)| *k);

                    ui.vertical(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if ui
                                .button(format!(
                                    "📂 {}",
                                    i18n::LOCALES.lookup(language, "patches-open-folder").unwrap(),
                                ))
                                .clicked()
                            {
                                let _ = open::that(&patch.path);
                            }

                            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                                ui.horizontal(|ui| {
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Max).with_main_wrap(true),
                                        |ui| {
                                            let is_starred = starred_patches.contains(patch_name);
                                            if ui
                                                .button(
                                                    if is_starred {
                                                        egui::RichText::new("★")
                                                    } else {
                                                        egui::RichText::new("☆")
                                                    }
                                                    .color(egui::Color32::GOLD),
                                                )
                                                .clicked()
                                            {
                                                if is_starred {
                                                    starred_patches.remove(patch_name);
                                                } else {
                                                    starred_patches.insert(patch_name.clone());
                                                }
                                            }

                                            ui.heading(&patch.title);
                                            if let Some((version, _)) = latest_version_and_info.as_ref() {
                                                ui.label(version.to_string());
                                            }
                                        },
                                    );
                                });
                            });
                        });
                        egui::Grid::new("patch-info-grid").num_columns(2).show(ui, |ui| {
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Min).with_cross_justify(true),
                                |ui| {
                                    ui.strong(i18n::LOCALES.lookup(language, "patches-details-authors").unwrap());
                                },
                            );
                            ui.vertical(|ui| {
                                for author in patch.authors.iter() {
                                    let name = author.display_name.as_ref().unwrap_or(&author.addr);
                                    if author.addr == "" {
                                        ui.label(name);
                                    } else {
                                        ui.hyperlink_to(name, format!("mailto:{}", author.addr));
                                    }
                                }
                            });
                            ui.end_row();

                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Min).with_cross_justify(true),
                                |ui| {
                                    ui.strong(i18n::LOCALES.lookup(language, "patches-details-license").unwrap());
                                },
                            );
                            if let Some(license) = patch.license.as_ref() {
                                ui.label(license);
                            } else {
                                ui.label(
                                    i18n::LOCALES
                                        .lookup(language, "patches-details-license.all-rights-reserved")
                                        .unwrap(),
                                );
                            }
                            ui.end_row();

                            if let Some(source) = patch.source.as_ref() {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Min).with_cross_justify(true),
                                    |ui| {
                                        ui.strong(i18n::LOCALES.lookup(language, "patches-details-source").unwrap());
                                    },
                                );
                                ui.hyperlink_to("🌐", source);
                                ui.end_row();
                            }

                            if let Some((_, version_info)) = latest_version_and_info.as_ref() {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Min).with_cross_justify(true),
                                    |ui| {
                                        ui.strong(i18n::LOCALES.lookup(language, "patches-details-games").unwrap());
                                    },
                                );
                                ui.vertical(|ui| {
                                    let mut games = version_info.supported_games.iter().cloned().collect::<Vec<_>>();
                                    game::sort_games(language, &mut games);
                                    for game in games.iter() {
                                        let (family, variant) = game.family_and_variant();
                                        ui.label(
                                            i18n::LOCALES
                                                .lookup(language, &format!("game-{}.variant-{}", family, variant))
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
                                ui.monospace(patch.readme.clone().unwrap_or("".to_string()));
                            });
                    });
                });
            },
        );
    });
}
