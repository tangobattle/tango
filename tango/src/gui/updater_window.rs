use fluent_templates::Loader;

use crate::{i18n, sync, updater};

pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    language: &unic_langid::LanguageIdentifier,
    updater: &updater::Updater,
) {
    egui::Window::new(format!("🆕 {}", i18n::LOCALES.lookup(language, "updater").unwrap()))
        .id(egui::Id::new("updater-window"))
        .open(open)
        .show(ctx, |ui| {
            let status = sync::block_on(updater.status());
            egui::Grid::new("updater-window-grid").num_columns(2).show(ui, |ui| {
                ui.strong(i18n::LOCALES.lookup(language, "updater-current-version").unwrap());
                ui.label(format!("v{}", updater.current_version()));
                ui.end_row();

                ui.strong(i18n::LOCALES.lookup(language, "updater-latest-version").unwrap());
                ui.label(format!(
                    "v{}",
                    match &status {
                        updater::Status::UpToDate => {
                            updater.current_version()
                        }
                        updater::Status::UpdateAvailable { version } => {
                            version
                        }
                        updater::Status::Downloading { version, .. } => {
                            version
                        }
                        updater::Status::ReadyToUpdate { version } => {
                            version
                        }
                    }
                ));
                ui.end_row();
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
}
