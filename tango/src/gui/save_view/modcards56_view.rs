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
            .column(egui_extras::Size::exact(100.0))
            .column(egui_extras::Size::exact(100.0))
            .striped(true)
            .body(|body| {
                body.rows(28.0, items.len(), |i, mut row| {
                    let item = &items[i];
                    row.col(|ui| {
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
                    let effects = item
                        .as_ref()
                        .and_then(|item| assets.modcard56(item.id))
                        .map(|info| info.effects.as_slice())
                        .unwrap_or(&[][..]);

                    row.col(|ui| {
                        ui.vertical(|ui| {
                            for effect in effects {
                                if effect.is_ability {
                                    continue;
                                }

                                ui.label(format!("{} {}", effect.name, effect.is_debuff));
                            }
                        });
                    });
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            for effect in effects {
                                if !effect.is_ability {
                                    continue;
                                }

                                ui.label(format!("{} {}", effect.name, effect.is_debuff));
                            }
                        });
                    });
                });
            });
    }
}
