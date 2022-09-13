#[allow(unused_imports)]
use fluent_templates::Loader;

#[allow(unused_imports)]
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

pub fn show<'a>(
    ui: &mut egui::Ui,
    _clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    _lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    navi_view: &Box<dyn save::NaviView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
) {
    let navi_id = navi_view.navi();
    let navi = if let Some(navi) = assets.navi(navi_id) {
        navi
    } else {
        return;
    };

    egui::ScrollArea::vertical()
        .id_source("navi-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical_centered_justified(|ui| {
                if state.emblem_texture_cache.is_none() {
                    state.emblem_texture_cache = Some(ui.ctx().load_texture(
                        "emblem",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [15, 15],
                            &image::imageops::crop_imm(&navi.emblem(), 1, 0, 15, 15).to_image(),
                        ),
                        egui::TextureFilter::Nearest,
                    ));
                }

                if let Some(texture_handle) = state.emblem_texture_cache.as_ref() {
                    ui.image(texture_handle.id(), egui::Vec2::new(30.0, 30.0));
                }
                ui.heading(egui::RichText::new(&navi.name()).family(font_families.for_language(game_lang)));
            });
        });
}
