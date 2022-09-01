use fluent_templates::Loader;

use crate::{game, gui, i18n, rom, save};

pub struct State {}

impl State {
    pub fn new() -> Self {
        Self {}
    }
}

pub struct Modcards56View {}

impl Modcards56View {
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
        modcards56_view: &Box<dyn save::Modcards56View<'a> + 'a>,
        assets: &Box<dyn rom::Assets + Send + Sync>,
        state: &mut State,
    ) {
        let items = (0..modcards56_view.count())
            .map(|slot| modcards56_view.modcard(slot))
            .collect::<Vec<_>>();
        egui_extras::TableBuilder::new(ui)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Size::remainder())
            .striped(true)
            .body(|body| {
                body.rows(28.0, items.len(), |i, mut row| {
                    row.col(|ui| {
                        let item = &items[i];
                        ui.label(
                            if let Some(modcard) =
                                item.as_ref().and_then(|item| assets.modcard56(item.id))
                            {
                                &modcard.name
                            } else {
                                ""
                            },
                        );
                    });
                });
            });
    }
}
