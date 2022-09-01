use fluent_templates::Loader;

use crate::{game, gui, i18n, rom, save};

pub struct State {}

impl State {
    pub fn new() -> Self {
        Self {}
    }
}

pub struct NavicustView {}

impl NavicustView {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show<'a>(
        &mut self,
        ui: &mut egui::Ui,
        clipboard: &mut arboard::Clipboard,
        font_families: &gui::FontFamilies,
        lang: &unic_langid::LanguageIdentifier,
        game: &'static (dyn game::Game + Send + Sync),
        navicust_view: &Box<dyn save::NavicustView<'a> + 'a>,
        assets: &Box<dyn rom::Assets + Send + Sync>,
        _state: &mut State,
    ) {
        for i in 0..navicust_view.count() {
            let ncp = if let Some(ncp) = navicust_view.navicust_part(i) {
                ncp
            } else {
                continue;
            };

            let info = if let Some(info) = assets.navicust_part(ncp.id, ncp.variant) {
                info
            } else {
                continue;
            };

            ui.label(
                egui::RichText::new(&info.name)
                    .family(font_families.for_language(&game.language())),
            );
        }
    }
}
