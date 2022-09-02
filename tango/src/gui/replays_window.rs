use fluent_templates::Loader;

use crate::{i18n, replay, scanner};

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
                                if ui
                                    .selectable_label(
                                        state.selection.as_ref() == Some(path),
                                        format!(
                                            "{}",
                                            path.as_path()
                                                .strip_prefix(replays_path)
                                                .unwrap_or(path.as_path())
                                                .display()
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
