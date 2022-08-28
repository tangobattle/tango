use fluent_templates::Loader;

use crate::{games, gui, i18n};

pub struct State {
    selection: Option<(
        &'static (dyn games::Game + Send + Sync),
        Option<std::path::PathBuf>,
    )>,
}

impl State {
    pub fn new(
        selection: Option<(
            &'static (dyn games::Game + Send + Sync),
            Option<std::path::PathBuf>,
        )>,
    ) -> Self {
        Self { selection }
    }
}

pub struct SaveSelectWindow;

impl SaveSelectWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        show: &mut Option<State>,
        selection: &mut Option<gui::main_view::Selection>,
        language: &unic_langid::LanguageIdentifier,
        saves_path: &std::path::Path,
        saves_list: gui::SavesListState,
    ) {
        let mut show_play_bool = show.is_some();
        egui::Window::new(format!(
            "{}",
            i18n::LOCALES.lookup(language, "select-save").unwrap()
        ))
        .id(egui::Id::new("select-save-window"))
        .open(&mut show_play_bool)
        .show(ctx, |ui| {
            let saves_list = saves_list.read();
            let games = games::sorted_games(language);
            if let Some((game, _)) = show.as_mut().unwrap().selection {
                let (family, variant) = game.family_and_variant();
                ui.horizontal(|ui| {
                    if ui.selectable_label(false, "⬅️").clicked() {
                        show.as_mut().unwrap().selection = None;
                    }

                    ui.heading(
                        i18n::LOCALES
                            .lookup(language, &format!("games.{}-{}", family, variant))
                            .unwrap(),
                    );
                });
            }

            ui.group(|ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        if let Some((game, _)) = show.as_ref().unwrap().selection.clone() {
                            if let Some(saves) = saves_list.saves.get(&game) {
                                for save in saves {
                                    if ui
                                        .selectable_label(
                                            selection
                                                .as_ref()
                                                .map(|selection| {
                                                    selection.save_path.as_path() == save.as_path()
                                                })
                                                .unwrap_or(false),
                                            format!(
                                                "{}",
                                                save.as_path()
                                                    .strip_prefix(saves_path)
                                                    .unwrap_or(save.as_path())
                                                    .display()
                                            ),
                                        )
                                        .clicked()
                                    {
                                        *show = None;
                                        *selection = Some(gui::main_view::Selection {
                                            game,
                                            save_path: save.as_path().to_path_buf(),
                                            rom: saves_list.roms.get(&game).unwrap().clone(),
                                        });
                                    }
                                }
                            }
                        } else {
                            for (available, game) in games
                                .iter()
                                .filter(|g| saves_list.roms.contains_key(*g))
                                .map(|g| (true, g))
                                .chain(
                                    games
                                        .iter()
                                        .filter(|g| !saves_list.roms.contains_key(*g))
                                        .map(|g| (false, g)),
                                )
                            {
                                let (family, variant) = game.family_and_variant();
                                let text = i18n::LOCALES
                                    .lookup(language, &format!("games.{}-{}", family, variant))
                                    .unwrap();

                                if available {
                                    if ui.selectable_label(false, text).clicked() {
                                        show.as_mut().unwrap().selection = Some((*game, None));
                                    }
                                } else {
                                    ui.weak(text);
                                }
                            }
                        }
                    });
                });
            });
        });

        if !show_play_bool {
            *show = None;
        }
    }
}
