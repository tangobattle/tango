use fluent_templates::Loader;

use crate::{i18n, sync, updater};

pub struct State {
    commonmark_cache: egui_commonmark::CommonMarkCache,
}

impl State {
    pub fn new() -> Self {
        Self {
            commonmark_cache: egui_commonmark::CommonMarkCache::default(),
        }
    }
}

pub fn show(
    ctx: &egui::Context,
    state: &mut Option<State>,
    language: &unic_langid::LanguageIdentifier,
    updater: &updater::Updater,
) {
    let mut open = state.is_some();
    egui::Window::new(format!("🆕 {}", i18n::LOCALES.lookup(language, "updater").unwrap()))
        .id(egui::Id::new("updater-window"))
        .open(&mut open)
        .show(ctx, |ui| {
            let status = sync::block_on(updater.status());
            let (is_loading, release) = match &status {
                updater::Status::UpToDate { release } => match release.as_ref() {
                    Some(r) => (false, r.as_ref()),
                    None => (true, None),
                },
                updater::Status::UpdateAvailable { release } => (false, Some(release)),
                updater::Status::Downloading { release, .. } => (false, Some(release)),
                updater::Status::ReadyToUpdate { release } => (false, Some(release)),
            };

            egui::Grid::new("updater-window-grid").num_columns(2).show(ui, |ui| {
                ui.strong(i18n::LOCALES.lookup(language, "updater-current-version").unwrap());
                ui.label(format!("v{}", updater.current_version()));
                ui.end_row();

                ui.strong(i18n::LOCALES.lookup(language, "updater-latest-version").unwrap());
                ui.label(format!(
                    "v{}",
                    release
                        .as_ref()
                        .map(|r| &r.version)
                        .unwrap_or_else(|| updater.current_version())
                ));
                ui.end_row();
            });

            ui.set_min_height(100.0);
            ui.group(|ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(400.0)
                    .id_source("updater-version-info")
                    .show(ui, |ui| {
                        if is_loading {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(i18n::LOCALES.lookup(language, "updater-loading").unwrap());
                            });
                        } else if let Some(release) = release.as_ref() {
                            egui_commonmark::CommonMarkViewer::new("release-info").show(
                                ui,
                                &mut state.as_mut().unwrap().commonmark_cache,
                                &release.info,
                            );
                        } else {
                            ui.label(i18n::LOCALES.lookup(language, "updater-no-info").unwrap());
                        }
                    });
            });

            match &status {
                updater::Status::Downloading { current, total, .. } => {
                    ui.add(
                        egui::widgets::ProgressBar::new({
                            if *total > 0 {
                                *current as f32 / *total as f32
                            } else {
                                -1.0
                            }
                        })
                        .show_percentage(),
                    );
                }
                updater::Status::ReadyToUpdate { .. } => {
                    ui.add(
                        egui::widgets::ProgressBar::new(1.0)
                            .text(i18n::LOCALES.lookup(language, "updater-ready-to-update").unwrap()),
                    );
                    if ui
                        .button(i18n::LOCALES.lookup(language, "updater-update-now").unwrap())
                        .clicked()
                    {
                        updater.finish_update();
                    }
                }
                _ => {}
            }
        });
    if !open {
        *state = None;
    }
}
