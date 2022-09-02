use chrono_locale::LocaleDate;
use fluent_templates::Loader;

use crate::{game, gui, i18n, replay, scanner};

pub struct State {
    replays_scanner:
        scanner::Scanner<std::collections::BTreeMap<std::path::PathBuf, replay::Metadata>>,
    selection: Option<std::path::PathBuf>,
}

impl State {
    pub fn new(replays_path: &std::path::Path) -> Self {
        let replays_scanner = scanner::Scanner::new();
        rayon::spawn({
            let replays_scanner = replays_scanner.clone();
            let replays_path = replays_path.to_path_buf();
            move || {
                replays_scanner.rescan(move || {
                    let mut replays = std::collections::BTreeMap::new();
                    for entry in walkdir::WalkDir::new(&replays_path) {
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(_) => {
                                continue;
                            }
                        };

                        if !entry.file_type().is_file() {
                            continue;
                        }

                        let path = entry.path();
                        let mut f = match std::fs::File::open(path) {
                            Ok(f) => f,
                            Err(_) => {
                                continue;
                            }
                        };

                        let metadata = match replay::read_metadata(&mut f) {
                            Ok(metadata) => metadata,
                            Err(_) => {
                                continue;
                            }
                        };

                        replays.insert(path.to_path_buf(), metadata);
                    }
                    replays
                })
            }
        });
        Self {
            selection: None,
            replays_scanner,
        }
    }
}

pub struct ReplaysWindow {}

impl ReplaysWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        show: &mut Option<State>,
        language: &unic_langid::LanguageIdentifier,
        patches_scanner: gui::PatchesScanner,
        roms_scanner: gui::ROMsScanner,
        replays_path: &std::path::PathBuf,
    ) {
        let mut show_window = show.is_some();
        egui::Window::new(format!(
            "📽️ {}",
            i18n::LOCALES.lookup(language, "replays").unwrap()
        ))
        .id(egui::Id::new("replays-window"))
        .resizable(true)
        .min_width(400.0)
        .default_width(600.0)
        .open(&mut show_window)
        .show(ctx, |ui| {
            let state = show.as_mut().unwrap();
            if state.replays_scanner.is_scanning() {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(10.0));
                    ui.label(i18n::LOCALES.lookup(language, "replays.scanning").unwrap());
                });
                return;
            }

            let replays = state.replays_scanner.read();
            ui.horizontal_top(|ui| {
                egui::ScrollArea::vertical()
                    .max_width(200.0)
                    .auto_shrink([false, false])
                    .id_source("replays-window-left")
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            for (path, metadata) in replays.iter().rev() {
                                let ts = if let Some(ts) = std::time::UNIX_EPOCH
                                    .checked_add(std::time::Duration::from_millis(metadata.ts))
                                {
                                    ts
                                } else {
                                    continue;
                                };

                                let local_side = if let Some(side) = metadata.local_side.as_ref() {
                                    side
                                } else {
                                    continue;
                                };

                                let remote_side = if let Some(side) = metadata.remote_side.as_ref()
                                {
                                    side
                                } else {
                                    continue;
                                };

                                let local_game =
                                    local_side.game_info.as_ref().and_then(|game_info| {
                                        game::find_by_family_and_variant(
                                            game_info.rom_family.as_str(),
                                            game_info.rom_variant as u8,
                                        )
                                    });

                                if ui
                                    .selectable_label(
                                        state.selection.as_ref() == Some(path),
                                        format!(
                                            "{}",
                                            chrono::DateTime::<chrono::Local>::from(ts)
                                                .formatl("%c", &language.to_string())
                                        ),
                                    )
                                    .clicked()
                                {
                                    state.selection = Some(path.clone());
                                }
                            }
                        });
                    });
            });
        });

        if !show_window {
            *show = None;
        }
    }
}
