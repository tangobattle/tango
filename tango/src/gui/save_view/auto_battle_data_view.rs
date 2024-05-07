use fluent_templates::Loader;
use itertools::Itertools;

use crate::{gui, i18n};

pub struct State {
    chip_icon_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
    element_icon_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
    materialized: Option<tango_dataview::auto_battle_data::MaterializedAutoBattleData>,
}

impl State {
    pub fn new() -> Self {
        Self {
            chip_icon_texture_cache: std::collections::HashMap::new(),
            element_icon_texture_cache: std::collections::HashMap::new(),
            materialized: None,
        }
    }
}

fn show_table(
    ui: &mut egui::Ui,
    chips: &[Option<usize>],
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    chip_icon_texture_cache: &mut std::collections::HashMap<usize, egui::TextureHandle>,
    element_icon_texture_cache: &mut std::collections::HashMap<usize, egui::TextureHandle>,
) {
    let groups = chips.iter().group_by(|k| **k);
    let groups = groups.into_iter().enumerate().collect::<Vec<_>>();
    egui_extras::StripBuilder::new(ui)
        .sizes(egui_extras::Size::exact(28.0), groups.len())
        .vertical(|mut outer_strip| {
            for (i, (id, g)) in groups {
                outer_strip.cell(|ui| {
                    let info = id.and_then(|id| assets.chip(id));

                    let (bg_color, fg_color) = if let Some(info) = info.as_ref() {
                        let bg_color = if info.dark() {
                            Some(if ui.visuals().dark_mode {
                                egui::Color32::from_rgb(0x31, 0x39, 0x5a)
                            } else {
                                egui::Color32::from_rgb(0xb5, 0x8c, 0xd6)
                            })
                        } else {
                            match info.class() {
                                tango_dataview::rom::ChipClass::Standard => None,
                                tango_dataview::rom::ChipClass::Mega => Some(if ui.visuals().dark_mode {
                                    egui::Color32::from_rgb(0x52, 0x84, 0x9c)
                                } else {
                                    egui::Color32::from_rgb(0xad, 0xef, 0xef)
                                }),
                                tango_dataview::rom::ChipClass::Giga => Some(if ui.visuals().dark_mode {
                                    egui::Color32::from_rgb(0x8c, 0x31, 0x52)
                                } else {
                                    egui::Color32::from_rgb(0xf7, 0xce, 0xe7)
                                }),
                                tango_dataview::rom::ChipClass::None => None,
                                tango_dataview::rom::ChipClass::ProgramAdvance => None,
                            }
                        };
                        (
                            bg_color,
                            if bg_color.is_some() && ui.visuals().dark_mode {
                                Some(ui.visuals().strong_text_color())
                            } else {
                                None
                            },
                        )
                    } else {
                        (None, None)
                    };

                    let rect = ui.available_rect_before_wrap().expand(ui.spacing().item_spacing.y);
                    if let Some(bg_color) = bg_color {
                        ui.painter().rect_filled(rect, 0.0, bg_color);
                    } else if i % 2 == 0 {
                        ui.painter().rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                    }

                    egui_extras::StripBuilder::new(ui)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .size(egui_extras::Size::exact(30.0))
                        .size(egui_extras::Size::exact(28.0))
                        .size(egui_extras::Size::remainder())
                        .size(egui_extras::Size::exact(28.0))
                        .size(egui_extras::Size::exact(30.0))
                        .horizontal(|mut strip| {
                            strip.cell(|ui| {
                                ui.strong(format!("{}x", g.count()));
                            });
                            if let Some(id) = id {
                                strip.cell(|ui| {
                                    match chip_icon_texture_cache.entry(id) {
                                        std::collections::hash_map::Entry::Occupied(_) => {}
                                        std::collections::hash_map::Entry::Vacant(e) => {
                                            if let Some(image) = info.as_ref().map(|info| info.icon()) {
                                                e.insert(ui.ctx().load_texture(
                                                    format!("chip icon {}", id),
                                                    egui::ColorImage::from_rgba_unmultiplied(
                                                        [14, 14],
                                                        &image::imageops::crop_imm(&image, 1, 1, 14, 14).to_image(),
                                                    ),
                                                    egui::TextureOptions::NEAREST,
                                                ));
                                            }
                                        }
                                    }

                                    if let Some(texture_handle) = chip_icon_texture_cache.get(&id) {
                                        ui.image((texture_handle.id(), egui::Vec2::new(28.0, 28.0)));
                                    }
                                });
                                strip.cell(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(
                                                    info.as_ref()
                                                        .and_then(|info| info.name())
                                                        .unwrap_or_else(|| "???".to_string()),
                                                )
                                                .color(fg_color.unwrap_or(ui.visuals().text_color()))
                                                .family(font_families.for_language(game_lang)),
                                            );
                                        });
                                    });
                                });
                                strip.cell(|ui| {
                                    let element = if let Some(element) = info.as_ref().map(|info| info.element()) {
                                        element
                                    } else {
                                        return;
                                    };

                                    match element_icon_texture_cache.entry(element) {
                                        std::collections::hash_map::Entry::Occupied(_) => {}
                                        std::collections::hash_map::Entry::Vacant(e) => {
                                            if let Some(image) = assets.element_icon(element) {
                                                e.insert(ui.ctx().load_texture(
                                                    format!("element {}", element),
                                                    egui::ColorImage::from_rgba_unmultiplied(
                                                        [14, 14],
                                                        &image::imageops::crop_imm(&image, 1, 1, 14, 14).to_image(),
                                                    ),
                                                    egui::TextureOptions::NEAREST,
                                                ));
                                            }
                                        }
                                    }

                                    if let Some(texture_handle) = element_icon_texture_cache.get(&element) {
                                        ui.image((texture_handle.id(), egui::Vec2::new(28.0, 28.0)));
                                    }
                                });
                                strip.cell(|ui| {
                                    let attack_power = info.as_ref().map(|info| info.attack_power()).unwrap_or(0);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if attack_power > 0 {
                                            ui.strong(format!("{}", attack_power));
                                        }
                                    });
                                });
                            } else {
                                strip.cell(|_ui| {});
                                strip.cell(|ui| {
                                    ui.weak(i18n::LOCALES.lookup(lang, "auto-battle-data-unset").unwrap());
                                });
                                strip.cell(|_ui| {});
                                strip.cell(|_ui| {});
                            }
                        });
                });
            }
        });
}

fn make_string(chips: &[Option<usize>], assets: &(dyn tango_dataview::rom::Assets + Send + Sync)) -> String {
    chips
        .iter()
        .group_by(|k| **k)
        .into_iter()
        .map(|(id, g)| {
            let name = if let Some(id) = id {
                if let Some(info) = assets.chip(id) {
                    info.name().unwrap_or_else(|| "???".to_string())
                } else {
                    "-".to_string()
                }
            } else {
                "-".to_string()
            };
            format!("{}\t{}", g.count(), name)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn show(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    auto_battle_data_view: &dyn tango_dataview::save::AutoBattleDataView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    state: &mut State,
) {
    let materialized = state
        .materialized
        .get_or_insert_with(|| auto_battle_data_view.materialized());

    ui.horizontal(|ui| {
        if ui
            .button(format!(
                "📋 {}",
                i18n::LOCALES.lookup(lang, "copy-to-clipboard").unwrap(),
            ))
            .clicked()
        {
            let _ = clipboard.set_text(
                [
                    make_string(materialized.secondary_standard_chips(), assets),
                    make_string(materialized.standard_chips(), assets),
                    make_string(materialized.mega_chips(), assets),
                    make_string(&[materialized.giga_chip()], assets),
                    make_string(&[None; 8], assets),
                    make_string(&[materialized.program_advance()], assets),
                ]
                .join("\n"),
            );
        }
    });

    egui::ScrollArea::vertical()
        .id_source("auto-battle-data-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.push_id(egui::Id::new("auto-battle-data-view-secondary-standard-chips"), |ui| {
                ui.strong(
                    i18n::LOCALES
                        .lookup(lang, "auto-battle-data-secondary-standard-chips")
                        .unwrap(),
                );
                show_table(
                    ui,
                    materialized.secondary_standard_chips(),
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("auto-battle-data-view-standard-chips"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "auto-battle-data-standard-chips").unwrap());
                show_table(
                    ui,
                    materialized.standard_chips(),
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("auto-battle-data-view-mega-chips"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "auto-battle-data-mega-chips").unwrap());
                show_table(
                    ui,
                    materialized.mega_chips(),
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("auto-battle-data-view-giga-chip"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "auto-battle-data-giga-chip").unwrap());
                show_table(
                    ui,
                    &[materialized.giga_chip()],
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("auto-battle-data-view-combos"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "auto-battle-data-combos").unwrap());
                show_table(
                    ui,
                    &[None; 8],
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("auto-battle-data-view-program-advance"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "auto-battle-data-program-advance").unwrap());
                show_table(
                    ui,
                    &[materialized.program_advance()],
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });
        });
}
