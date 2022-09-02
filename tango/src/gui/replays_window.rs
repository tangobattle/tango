use fluent_templates::Loader;

use crate::i18n;

pub struct State {
    selection: Option<String>,
}

impl State {
    pub fn new() -> Self {
        Self { selection: None }
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
        .show(ctx, |ui| {});
    }
}
