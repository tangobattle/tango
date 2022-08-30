use crate::{game, gui, rom, save};

pub struct FolderView {}

impl FolderView {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show<'a>(
        &mut self,
        ui: &mut egui::Ui,
        font_families: &gui::FontFamilies,
        lang: &unic_langid::LanguageIdentifier,
        game: &'static (dyn game::Game + Send + Sync),
        chips_view: &Box<dyn save::ChipsView<'a> + 'a>,
        assets: &Box<dyn rom::Assets + Send + Sync>,
        texture_cache: &mut std::collections::HashMap<
            (gui::save_view::CachedAssetType, usize),
            egui::TextureHandle,
        >,
    ) {
        egui_extras::TableBuilder::new(ui)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Size::exact(28.0))
            .column(egui_extras::Size::remainder())
            .column(egui_extras::Size::exact(28.0))
            .column(egui_extras::Size::exact(30.0))
            .striped(true)
            .body(|body| {
                body.rows(28.0, 30, |i, mut row| {
                    let chip = chips_view
                        .chip(chips_view.equipped_folder_index(), i)
                        .unwrap();
                    let info = if let Some(info) = assets.chip(chip.id) {
                        info
                    } else {
                        return;
                    };
                    row.col(|ui| {
                        ui.image(
                            texture_cache
                                .entry((gui::save_view::CachedAssetType::ChipIcon, chip.id))
                                .or_insert_with(|| {
                                    ui.ctx().load_texture(
                                        "",
                                        egui::ColorImage::from_rgba_unmultiplied(
                                            [14, 14],
                                            &image::imageops::crop_imm(&info.icon, 1, 1, 14, 14)
                                                .to_image(),
                                        ),
                                        egui::TextureFilter::Nearest,
                                    )
                                })
                                .id(),
                            egui::Vec2::new(28.0, 28.0),
                        );
                    });
                    row.col(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(
                            egui::RichText::new(&info.name)
                                .family(font_families.for_language(&game.language())),
                        );
                        ui.label(format!(" {}", chips_view.chip_codes()[chip.code] as char));
                    });
                    row.col(|ui| {
                        if let Some(icon) = assets.element_icon(info.element) {
                            ui.image(
                                texture_cache
                                    .entry((
                                        gui::save_view::CachedAssetType::ElementIcon,
                                        info.element,
                                    ))
                                    .or_insert_with(|| {
                                        ui.ctx().load_texture(
                                            "",
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [14, 14],
                                                &image::imageops::crop_imm(icon, 1, 1, 14, 14)
                                                    .to_image(),
                                            ),
                                            egui::TextureFilter::Nearest,
                                        )
                                    })
                                    .id(),
                                egui::Vec2::new(28.0, 28.0),
                            );
                        }
                    });
                    row.col(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if info.damage > 0 {
                                ui.label(format!("{}", info.damage));
                            }
                        });
                    });
                });
            });
    }
}
