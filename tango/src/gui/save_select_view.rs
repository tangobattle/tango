use fluent_templates::Loader;

use crate::{game, gui, i18n, net, patch, rom, save};

pub struct State {
    selection: Option<(&'static (dyn game::Game + Send + Sync), Option<std::path::PathBuf>)>,
}

impl State {
    pub fn new(selection: Option<(&'static (dyn game::Game + Send + Sync), Option<std::path::PathBuf>)>) -> Self {
        Self { selection }
    }
}

pub fn show(
    ui: &mut egui::Ui,
    show: &mut Option<State>,
    selection: &mut Option<gui::Selection>,
    language: &unic_langid::LanguageIdentifier,
    saves_path: &std::path::Path,
    roms_scanner: rom::Scanner,
    saves_scanner: save::Scanner,
    patches_scanner: patch::Scanner,
    remote_settings: Option<&net::protocol::Settings>,
) {
    let roms = roms_scanner.read();
    let saves = saves_scanner.read();
    let patches = patches_scanner.read();

    ui.vertical(|ui| {
        let games = game::sorted_all_games(language);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if ui
                .button(format!(
                    "📂 {}",
                    i18n::LOCALES.lookup(language, "select-save.open-folder").unwrap(),
                ))
                .clicked()
            {
                let _ = open::that(
                    if let Some(path) = selection.as_ref().and_then(|selection| selection.save.path.parent()) {
                        path
                    } else {
                        saves_path
                    },
                );
            }

            if let Some((game, _)) = show.as_mut().unwrap().selection {
                let (family, variant) = game.family_and_variant();
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(
                            egui::Layout::left_to_right(egui::Align::Max).with_main_wrap(true),
                            |ui| {
                                ui.label(
                                    i18n::LOCALES
                                        .lookup(language, &format!("game-{}.variant-{}", family, variant))
                                        .unwrap(),
                                );
                            },
                        );
                    });
                });
            }
        });

        ui.group(|ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    if let Some((game, _)) = show.as_ref().unwrap().selection.clone() {
                        if ui
                            .selectable_label(
                                false,
                                format!(
                                    "⬅️ {}",
                                    i18n::LOCALES
                                        .lookup(language, "select-save.return-to-games-list")
                                        .unwrap()
                                ),
                            )
                            .clicked()
                        {
                            show.as_mut().unwrap().selection = None;
                        }

                        if let Some(saves) = saves.get(&game) {
                            for save in saves {
                                let selected = selection
                                    .as_ref()
                                    .map(|selection| selection.save.path.as_path() == save.path.as_path())
                                    .unwrap_or(false);
                                let mut layout_job = egui::text::LayoutJob::default();
                                layout_job.append(
                                    &format!(
                                        "{}",
                                        save.path
                                            .as_path()
                                            .strip_prefix(saves_path)
                                            .unwrap_or(save.path.as_path())
                                            .display()
                                    ),
                                    0.0,
                                    egui::TextFormat::simple(
                                        ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                        if selected {
                                            ui.visuals().selection.stroke.color
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                    ),
                                );
                                if ui.selectable_label(selected, layout_job).clicked() {
                                    let (game, rom, patch) = if let Some(selection) = selection.take() {
                                        if selection.game == game {
                                            (selection.game, selection.rom, selection.patch)
                                        } else {
                                            (game, roms.get(&game).unwrap().clone(), None)
                                        }
                                    } else {
                                        (game, roms.get(&game).unwrap().clone(), None)
                                    };

                                    *show = None;
                                    *selection = Some(gui::Selection::new(game, save.clone(), patch, rom));
                                }
                            }
                        }
                    } else {
                        for (available, game) in games
                            .iter()
                            .filter(|g| roms.contains_key(*g))
                            .map(|g| (true, g))
                            .chain(games.iter().filter(|g| !roms.contains_key(*g)).map(|g| (false, g)))
                        {
                            let (family, variant) = game.family_and_variant();

                            let selected = selection
                                .as_ref()
                                .map(|selection| selection.game == *game)
                                .unwrap_or(false);

                            let warning = (|| {
                                let remote_settings = if let Some(remote_settings) = remote_settings.as_ref() {
                                    remote_settings
                                } else {
                                    return None;
                                };

                                if !remote_settings
                                    .available_games
                                    .iter()
                                    .any(|(family, variant)| game.family_and_variant() == (family, *variant))
                                {
                                    return Some(gui::play_pane::Warning::NoRemoteROM(*game));
                                }

                                let remote_gi = if let Some(remote_gi) = remote_settings.game_info.as_ref() {
                                    remote_gi
                                } else {
                                    return None;
                                };

                                if let Some(netplay_compatibility) =
                                    gui::play_pane::get_netplay_compatibility_from_game_info(remote_gi, &patches)
                                {
                                    if &netplay_compatibility != family
                                        && !patches.values().any(|metadata| {
                                            metadata.versions.values().any(|version| {
                                                version.supported_games.contains(game)
                                                    && version.netplay_compatibility == netplay_compatibility
                                            })
                                        })
                                    {
                                        return Some(gui::play_pane::Warning::Incompatible);
                                    }
                                }
                                None
                            })();

                            let mut layout_job = egui::text::LayoutJob::default();
                            if warning.is_some() {
                                gui::warning::append_to_layout_job(ui, &mut layout_job);
                            }
                            layout_job.append(
                                &i18n::LOCALES
                                    .lookup(language, &format!("game-{}.variant-{}", family, variant))
                                    .unwrap(),
                                0.0,
                                egui::TextFormat::simple(
                                    ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                    if selected {
                                        ui.visuals().selection.stroke.color
                                    } else {
                                        ui.visuals().text_color()
                                    },
                                ),
                            );

                            let mut resp = ui.add_enabled(available, egui::SelectableLabel::new(selected, layout_job));
                            if let Some(warning) = warning {
                                resp = resp.on_hover_text(warning.description(language));
                            }

                            if resp.clicked() {
                                show.as_mut().unwrap().selection = Some((*game, None));
                            }
                        }
                    }
                });
            });
        });
    });
}
