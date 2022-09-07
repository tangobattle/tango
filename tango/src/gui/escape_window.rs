use fluent_templates::Loader;

use crate::{gui, i18n, session};

pub struct State {}

impl State {
    pub fn new() -> Self {
        Self {}
    }
}

pub fn show(
    ctx: &egui::Context,
    session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    selection: &mut Option<gui::Selection>,
    show_escape_window: &mut Option<State>,
    language: &unic_langid::LanguageIdentifier,
    show_settings: &mut Option<gui::settings_window::State>,
) {
    let mut open = show_escape_window.is_some();
    egui::Window::new("")
        .id(egui::Id::new("escape-window"))
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .resizable(false)
        .title_bar(false)
        .show(ctx, |ui| {
            ui.vertical_centered_justified(|ui| {
                if ui
                    .button(
                        egui::RichText::new(
                            i18n::LOCALES.lookup(language, "escape-settings").unwrap(),
                        )
                        .heading(),
                    )
                    .clicked()
                {
                    *show_settings = Some(gui::settings_window::State::new());
                    *show_escape_window = None;
                }
                if ui
                    .button(
                        egui::RichText::new(
                            i18n::LOCALES.lookup(language, "escape-end-game").unwrap(),
                        )
                        .heading(),
                    )
                    .clicked()
                {
                    *session.lock() = None;
                    // Current save file needs to be reloaded from disk.
                    // TODO: Maybe we even need to rescan saves if region lock status changed? (e.g. EXE4 -> BN4)
                    if let Some(selection) = selection.as_mut() {
                        let _ = selection.reload_save();
                    }
                    *show_escape_window = None;
                }
            });
        });
    if !open {
        *show_escape_window = None;
    }
}
