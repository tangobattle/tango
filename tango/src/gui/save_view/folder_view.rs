use fluent_templates::Loader;

use crate::{game, gui, i18n, rom, save};

pub struct State {
    grouped: bool,
}

impl State {
    pub fn new() -> Self {
        Self { grouped: true }
    }
}

pub struct FolderView {}

impl FolderView {
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
        chips_view: &Box<dyn save::ChipsView<'a> + 'a>,
        assets: &Box<dyn rom::Assets + Send + Sync>,
        texture_cache: &mut std::collections::HashMap<
            (gui::save_view::CachedAssetType, usize),
            egui::TextureHandle,
        >,
        state: &mut State,
    ) {
        struct GroupedChip {
            count: usize,
            is_regular: bool,
            tag_count: usize,
        }

        let mut chips = (0..30)
            .map(|i| {
                chips_view
                    .chip(chips_view.equipped_folder_index(), i)
                    .unwrap()
            })
            .collect::<Vec<_>>();

        if !chips_view.regular_chip_is_in_place() {
            if let Some(regular_chip_index) =
                chips_view.regular_chip_index(chips_view.equipped_folder_index())
            {
                let spliced = chips.splice(0..1, vec![]).collect::<Vec<_>>();
                chips.splice(regular_chip_index..regular_chip_index + 1, spliced);
            }
        }

        let items = if state.grouped {
            let mut grouped = indexmap::IndexMap::new();
            for (i, chip) in chips.iter().enumerate() {
                let g = grouped.entry(chip).or_insert(GroupedChip {
                    count: 0,
                    is_regular: false,
                    tag_count: 0,
                });
                g.count += 1;
                if chips_view.regular_chip_index(chips_view.equipped_folder_index()) == Some(i) {
                    g.is_regular = true;
                }
                if chips_view
                    .tag_chip_indexes(chips_view.equipped_folder_index())
                    .map_or(false, |is| is.contains(&i))
                {
                    g.tag_count += 1;
                }
            }

            grouped.into_iter().collect::<Vec<_>>()
        } else {
            chips
                .iter()
                .enumerate()
                .map(|(i, chip)| {
                    (
                        chip,
                        GroupedChip {
                            count: 1,
                            is_regular: chips_view
                                .regular_chip_index(chips_view.equipped_folder_index())
                                == Some(i),
                            tag_count: if chips_view
                                .tag_chip_indexes(chips_view.equipped_folder_index())
                                .map_or(false, |is| is.contains(&i))
                            {
                                1
                            } else {
                                0
                            },
                        },
                    )
                })
                .collect::<Vec<_>>()
        };

        ui.horizontal(|ui| {
            ui.checkbox(
                &mut state.grouped,
                i18n::LOCALES.lookup(lang, "save-group").unwrap(),
            );
            if ui
                .button(format!(
                    "📋 {}",
                    i18n::LOCALES.lookup(lang, "copy-to-clipboard").unwrap(),
                ))
                .clicked()
            {
                let _ = clipboard.set_text(
                    items
                        .iter()
                        .map(|(chip, g)| {
                            let mut buf = String::new();
                            if state.grouped {
                                buf.push_str(&format!("{}\t", g.count));
                            }
                            let info = assets.chip(chip.id);
                            buf.push_str(&format!(
                                "{}\t{}\t",
                                info.map(|info| info.name.as_str()).unwrap_or("???"),
                                chips_view.chip_codes()[chip.code] as char
                            ));
                            if g.is_regular {
                                buf.push_str("[REG]");
                            }
                            for _ in 0..g.tag_count {
                                buf.push_str("[TAG]");
                            }
                            buf.push('\n');
                            buf
                        })
                        .collect::<Vec<_>>()
                        .join(""),
                );
            }
        });

        let mut tb = egui_extras::TableBuilder::new(ui)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        if state.grouped {
            tb = tb.column(egui_extras::Size::exact(30.0));
        }

        tb.column(egui_extras::Size::exact(28.0))
            .column(egui_extras::Size::remainder())
            .column(egui_extras::Size::exact(28.0))
            .column(egui_extras::Size::exact(30.0))
            .striped(true)
            .body(|body| {
                body.rows(28.0, items.len(), |i, mut row| {
                    let (chip, g) = &items[i];
                    let info = assets.chip(chip.id);
                    if state.grouped {
                        row.col(|ui| {
                            ui.label(format!("{}x", g.count));
                        });
                    }
                    row.col(|ui| {
                        let icon = if let Some(icon) = info.map(|info| &info.icon) {
                            icon
                        } else {
                            return;
                        };

                        ui.image(
                            texture_cache
                                .entry((gui::save_view::CachedAssetType::ChipIcon, chip.id))
                                .or_insert_with(|| {
                                    ui.ctx().load_texture(
                                        format!("chip {}", chip.id),
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
                    });
                    row.col(|ui| {
                        ui.horizontal(|ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.label(
                                    egui::RichText::new(
                                        info.map(|info| info.name.as_str()).unwrap_or("???"),
                                    )
                                    .family(font_families.for_language(&game.language())),
                                );
                                ui.label(format!(
                                    " {}",
                                    chips_view.chip_codes()[chip.code] as char
                                ));
                            });
                            if g.is_regular {
                                egui::Frame::none()
                                    .inner_margin(egui::style::Margin::symmetric(4.0, 0.0))
                                    .rounding(egui::Rounding::same(2.0))
                                    .fill(egui::Color32::from_rgb(0xff, 0x42, 0xa5))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new("REG").color(egui::Color32::WHITE),
                                        );
                                    });
                            }
                            for _ in 0..g.tag_count {
                                egui::Frame::none()
                                    .inner_margin(egui::style::Margin::symmetric(4.0, 0.0))
                                    .rounding(egui::Rounding::same(2.0))
                                    .fill(egui::Color32::from_rgb(0x29, 0xf7, 0x21))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new("TAG").color(egui::Color32::WHITE),
                                        );
                                    });
                            }
                        });
                    });
                    row.col(|ui| {
                        let element = if let Some(element) = info.map(|info| info.element) {
                            element
                        } else {
                            return;
                        };

                        let icon = if let Some(icon) = assets.element_icon(element) {
                            icon
                        } else {
                            return;
                        };

                        ui.image(
                            texture_cache
                                .entry((gui::save_view::CachedAssetType::ElementIcon, element))
                                .or_insert_with(|| {
                                    ui.ctx().load_texture(
                                        format!("element {}", element),
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
                    });
                    row.col(|ui| {
                        let damage = info.map(|info| info.damage).unwrap_or(0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if damage > 0 {
                                ui.label(format!("{}", damage));
                            }
                        });
                    });
                });
            });
    }
}
