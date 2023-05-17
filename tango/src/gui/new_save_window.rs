use fluent_templates::Loader;

use crate::{game, i18n};

pub struct State {
    game: &'static (dyn game::Game + Send + Sync),
    name: String,
}

impl State {
    pub fn new(game: &'static (dyn game::Game + Send + Sync)) -> Self {
        Self {
            game,
            name: "".to_string(),
        }
    }
}

pub fn show(ctx: &egui::Context, state: &mut Option<State>, language: &unic_langid::LanguageIdentifier) {
    let mut open = state.is_some();
    egui::Window::new(format!("➕ {}", i18n::LOCALES.lookup(language, "new-save").unwrap()))
        .id(egui::Id::new("new-save"))
        .open(&mut open)
        .show(ctx, |ui| {
            // TODO
        });
    if !open {
        *state = None;
    }
}
