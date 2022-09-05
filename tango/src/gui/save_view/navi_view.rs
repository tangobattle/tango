use fluent_templates::Loader;

use crate::{gui, i18n, rom, save};

pub struct State {
    emblem_texture_cache: Option<egui::TextureHandle>,
}

impl State {
    pub fn new() -> Self {
        Self {
            emblem_texture_cache: None,
        }
    }
}

pub fn show_navi4dot556<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    _lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    navi4dot556_view: &Box<dyn save::Navi4dot556View<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
) {
    let navi = if let Some(navi) = assets.navi4dot556(navi4dot556_view.navi()) {
        navi
    } else {
        return;
    };

    egui::ScrollArea::vertical()
        .id_source("navi-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical_centered_justified(|ui| {
                ui.image(
                    state
                        .emblem_texture_cache
                        .get_or_insert_with(|| {
                            ui.ctx().load_texture(
                                "emblem",
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [15, 15],
                                    &image::imageops::crop_imm(&navi.emblem, 1, 0, 15, 15)
                                        .to_image(),
                                ),
                                egui::TextureFilter::Nearest,
                            )
                        })
                        .id(),
                    egui::Vec2::new(30.0, 30.0),
                );
                ui.heading(
                    egui::RichText::new(&navi.name).family(font_families.for_language(game_lang)),
                );
            });
        });
}

pub fn show<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    navi_view: &save::NaviView,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
) {
    match navi_view {
        save::NaviView::Navi4dot556(navi4dot556_view) => show_navi4dot556(
            ui,
            clipboard,
            font_families,
            lang,
            game_lang,
            navi4dot556_view,
            assets,
            state,
        ),
    }
}
