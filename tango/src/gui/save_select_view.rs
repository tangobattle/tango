use std::io::Write;

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

fn create_next_file(path: &std::path::Path) -> Result<(std::path::PathBuf, std::fs::File), std::io::Error> {
    let mut counter = 0;

    let file_stem = path.file_stem().unwrap_or_else(|| std::ffi::OsStr::new(""));
    let ext = path.extension().unwrap_or_else(|| std::ffi::OsStr::new(""));

    loop {
        let mut new_path = path.to_path_buf();
        if counter > 0 {
            let mut new_file_name = file_stem.to_os_string();
            new_file_name.push(format!(" {}", counter));
            if !ext.is_empty() {
                new_file_name.push(".");
                new_file_name.push(ext);
            }
            new_path.set_file_name(new_file_name);
        }
        match std::fs::File::options().write(true).create_new(true).open(&new_path) {
            Ok(f) => {
                return Ok((new_path, f));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                counter += 1;
                continue;
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
}

fn create_new_save(
    language: &unic_langid::LanguageIdentifier,
    saves_path: &std::path::Path,
    game: &(dyn game::Game + Send + Sync),
    name: &str,
) -> Result<(std::path::PathBuf, std::fs::File), std::io::Error> {
    let (family, variant) = game.family_and_variant();
    let mut prefix = i18n::LOCALES
        .lookup(language, &format!("game-{}.variant-{}", family, variant))
        .unwrap()
        .replace(":", "");
    if !name.is_empty() {
        prefix.push_str(" - ");
        prefix.push_str(
            &i18n::LOCALES
                .lookup(language, &format!("game-{}.save-{}", family, name))
                .unwrap(),
        );
    }
    prefix.push_str(".sav");

    Ok(create_next_file(&saves_path.join(prefix))?)
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
                if let Some(selection) = selection.as_ref() {
                    let _ = opener::reveal(&selection.save.path);
                } else {
                    let _ = opener::open(saves_path);
                }
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
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                if show.as_ref().unwrap().selection.is_some() {
                    if ui
                        .button(format!(
                            "⬅️ {}",
                            i18n::LOCALES
                                .lookup(language, "select-save.return-to-games-list")
                                .unwrap()
                        ))
                        .clicked()
                    {
                        show.as_mut().unwrap().selection = None;
                    }

                    if let Some(&(game, _)) = show.as_ref().unwrap().selection.as_ref() {
                        let save_templates = game.save_templates();
                        ui.add_enabled_ui(!save_templates.is_empty(), |ui| {
                            ui.menu_button(
                                format!("➕ {}", i18n::LOCALES.lookup(language, "select-save.new-save").unwrap()),
                                |ui| {
                                    let mut menu_selection = None;

                                    if save_templates.len() == 1 {
                                        menu_selection = save_templates.first().map(|(name, save)| (*name, *save));
                                    } else {
                                        for (name, save) in save_templates {
                                            if ui
                                                .button(format!(
                                                    "{}",
                                                    i18n::LOCALES
                                                        .lookup(
                                                            language,
                                                            &format!(
                                                                "game-{}.save-{}",
                                                                game.family_and_variant().0,
                                                                name
                                                            )
                                                        )
                                                        .unwrap()
                                                ))
                                                .clicked()
                                            {
                                                menu_selection = Some((*name, *save));
                                            }
                                        }
                                    }

                                    if let Some((name, save)) = menu_selection {
                                        let (path, mut f) = match create_new_save(language, saves_path, game, name) {
                                            Ok((path, f)) => (path, f),
                                            Err(e) => {
                                                log::error!("failed to create save: {}", e);
                                                ui.close_menu();
                                                return;
                                            }
                                        };

                                        let mut save = save.clone_box();
                                        save.rebuild();

                                        if let Err(e) = f.write_all(&save.to_vec()) {
                                            log::error!("failed to write save: {}", e);
                                            ui.close_menu();
                                            return;
                                        }

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
                                        *selection = Some(gui::Selection::new(
                                            game,
                                            save::ScannedSave { path, save },
                                            patch,
                                            rom,
                                        ));

                                        ui.close_menu();
                                    }
                                },
                            );
                        });
                    }
                }

                if show.is_none() {
                    return;
                }

                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        if let Some((game, _)) = show.as_ref().unwrap().selection.clone() {
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
                                    if ui
                                        .selectable_label(selected, layout_job)
                                        .context_menu(|ui| {
                                            if ui
                                                .button(egui::RichText::new(format!(
                                                    "📄 {}",
                                                    i18n::LOCALES
                                                        .lookup(language, "select-save.duplicate-save")
                                                        .unwrap()
                                                )))
                                                .clicked()
                                            {
                                                let (path, mut f) = match create_next_file(&save.path) {
                                                    Ok((path, f)) => (path, f),
                                                    Err(e) => {
                                                        log::error!("failed to create save: {}", e);
                                                        ui.close_menu();
                                                        return;
                                                    }
                                                };

                                                if let Err(e) = f.write_all(&save.save.to_vec()) {
                                                    log::error!("failed to write save: {}", e);
                                                    ui.close_menu();
                                                    return;
                                                }

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
                                                *selection = Some(gui::Selection::new(
                                                    game,
                                                    save::ScannedSave {
                                                        path,
                                                        save: save.save.clone_box(),
                                                    },
                                                    patch,
                                                    rom,
                                                ));

                                                ui.close_menu();
                                            }

                                            // if ui
                                            //     .button(egui::RichText::new(format!(
                                            //         "✏️ {}",
                                            //         i18n::LOCALES.lookup(language, "select-save.rename-save").unwrap()
                                            //     )))
                                            //     .clicked()
                                            // {
                                            //     // TODO: Show rename dialog.
                                            //     ui.close_menu();
                                            // }

                                            // if ui
                                            //     .button(
                                            //         egui::RichText::new(format!(
                                            //             "🗑️ {}",
                                            //             i18n::LOCALES
                                            //                 .lookup(language, "select-save.delete-save")
                                            //                 .unwrap()
                                            //         ))
                                            //         .color(egui::Color32::RED),
                                            //     )
                                            //     .clicked()
                                            // {
                                            //     // TODO: Show confirm dialog.
                                            //     ui.close_menu();
                                            // }
                                        })
                                        .clicked()
                                    {
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

                                let mut resp =
                                    ui.add_enabled(available, egui::SelectableLabel::new(selected, layout_job));
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
    });
}
