use crate::gui;

pub struct State {}

impl State {
    pub fn new() -> Self {
        Self {}
    }
}

pub struct EscapeWindow {}

impl EscapeWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        main_view: std::sync::Arc<parking_lot::Mutex<gui::main_view::State>>,
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
                        .button(egui::RichText::new("Settings").heading())
                        .clicked()
                    {
                        *show_settings = Some(gui::settings_window::State::new());
                        *show_escape_window = None;
                    }
                    if ui
                        .button(egui::RichText::new("End game").heading())
                        .clicked()
                    {
                        let mut main_view = main_view.lock();
                        *main_view = gui::main_view::State::new();
                        *show_escape_window = None;
                    }
                });
            });
        if !open {
            *show_escape_window = None;
        }
    }
}
