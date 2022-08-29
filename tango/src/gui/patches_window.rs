use fluent_templates::Loader;

use crate::{games, gui, i18n, patch};

pub struct State {
    selection: Option<std::ffi::OsString>,
}

impl State {
    pub fn new() -> Self {
        Self { selection: None }
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
            "{}",
            i18n::LOCALES.lookup(language, "select-patch").unwrap()
        ))
        .id(egui::Id::new("select-patch-window"))
        .resizable(true)
        .default_width(600.0)
        .open(&mut show_window)
        .show(ctx, |ui| {
            let state = show.as_mut().unwrap();

            let patches = patches_scanner.read();
            ui.horizontal_top(|ui| {
                egui::ScrollArea::vertical()
                    .max_width(200.0)
                    .auto_shrink([false, false])
                    .id_source("select-patch-window-left")
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            for (name, _) in patches.iter() {
                                if ui
                                    .selectable_label(
                                        state.selection == Some(name.to_owned()),
                                        format!("{}", name.to_string_lossy()),
                                    )
                                    .clicked()
                                {
                                    state.selection = Some(name.to_owned());
                                }
                            }
                        });
                    });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_source("select-patch-window-right")
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let patch = if let Some(patch) =
                                state.selection.as_ref().and_then(|n| patches.get(n))
                            {
                                patch
                            } else {
                                return;
                            };

                            ui.vertical(|ui| {
                                ui.heading(&patch.title);
                                egui::Grid::new("select-patch-info-grid")
                                    .num_columns(2)
                                    .show(ui, |ui| {
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Min)
                                                .with_cross_justify(true),
                                            |ui| {
                                                ui.label(
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

                                        if let Some(source) = patch.source.as_ref() {
                                            ui.with_layout(
                                                egui::Layout::left_to_right(egui::Align::Min)
                                                    .with_cross_justify(true),
                                                |ui| {
                                                    ui.label(
                                                        i18n::LOCALES
                                                            .lookup(language, "patches.source")
                                                            .unwrap(),
                                                    );
                                                },
                                            );
                                            ui.hyperlink_to("🌐", source);
                                            ui.end_row();
                                        }
                                    });
                                ui.separator();

                                ui.monospace(patch.readme.clone().unwrap_or("".to_string()));
                            });
                        });
                    });
            });
        });
        if !show_window {
            *show = None;
        }
    }
}
