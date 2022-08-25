use fluent_templates::Loader;

use crate::{audio, games, i18n, session, stats};

pub struct State {
    selected_game: Option<&'static (dyn games::Game + Send + Sync)>,
}

impl State {
    pub fn new() -> Self {
        State {
            selected_game: None,
        }
    }
}

pub struct PlayWindow;

impl PlayWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        show_play: &mut Option<State>,
        last_cursor_activity_time: &mut Option<std::time::Instant>,
        language: &unic_langid::LanguageIdentifier,
        saves_path: &std::path::Path,
        session: &mut Option<session::Session>,
        roms: &mut std::collections::HashMap<&'static (dyn games::Game + Send + Sync), Vec<u8>>,
        saves: &mut std::collections::HashMap<
            &'static (dyn games::Game + Send + Sync),
            Vec<std::path::PathBuf>,
        >,
        audio_binder: audio::LateBinder,
        emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    ) {
        let mut show_play_bool = show_play.is_some();
        egui::Window::new(format!(
            "🎮 {}",
            i18n::LOCALES.lookup(language, "play").unwrap()
        ))
        .id(egui::Id::new("play-window"))
        .open(&mut show_play_bool)
        .show(ctx, |ui| {
            let games = games::sorted_games(language);
            if let Some(game) = show_play.as_ref().unwrap().selected_game {
                let (family, variant) = game.family_and_variant();
                ui.heading(
                    i18n::LOCALES
                        .lookup(language, &format!("games.{}-{}", family, variant))
                        .unwrap(),
                );
            }

            ui.group(|ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        if let Some(selected_game) = show_play.as_ref().unwrap().selected_game {
                            if ui
                                .selectable_label(
                                    false,
                                    format!(
                                        "⬅️ {}",
                                        i18n::LOCALES
                                            .lookup(language, "play.return-to-games-list")
                                            .unwrap()
                                    ),
                                )
                                .clicked()
                            {
                                show_play.as_mut().unwrap().selected_game = None;
                            }

                            if let Some(saves) = saves.get(&selected_game) {
                                for save in saves {
                                    if ui
                                        .selectable_label(
                                            false,
                                            save.as_path()
                                                .strip_prefix(saves_path)
                                                .unwrap_or(save.as_path())
                                                .to_string_lossy(),
                                        )
                                        .clicked()
                                    {
                                        *show_play = None;
                                        *last_cursor_activity_time = None;

                                        // HACK: audio::Binding has to be dropped first.
                                        *session = None;
                                        *session = Some(
                                            session::Session::new_singleplayer(
                                                audio_binder.clone(),
                                                roms.get(&selected_game).unwrap(),
                                                save.as_path(),
                                                emu_tps_counter.clone(),
                                            )
                                            .unwrap(),
                                        );
                                    }
                                }
                            }
                        } else {
                            for (available, game) in games
                                .iter()
                                .filter(|g| roms.contains_key(*g))
                                .map(|g| (true, g))
                                .chain(
                                    games
                                        .iter()
                                        .filter(|g| !roms.contains_key(*g))
                                        .map(|g| (false, g)),
                                )
                            {
                                let (family, variant) = game.family_and_variant();
                                let text = i18n::LOCALES
                                    .lookup(language, &format!("games.{}-{}", family, variant))
                                    .unwrap();

                                if available {
                                    if ui.selectable_label(false, text).clicked() {
                                        show_play.as_mut().unwrap().selected_game = Some(*game);
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
            *show_play = None;
        }
    }
}
