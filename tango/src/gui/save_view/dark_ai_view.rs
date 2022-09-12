use fluent_templates::Loader;
use itertools::Itertools;

use crate::{gui, i18n, rom, save};

pub struct State {
    chip_icon_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
    element_icon_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
    materialized: Option<MaterializedDarkAI>,
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

const SECONDARY_STANDARD_CHIP_COUNTS: &[usize; 3] = &[1, 1, 1];
const STANDARD_CHIP_COUNTS: &[usize; 16] = &[4, 4, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
const MEGA_CHIP_COUNTS: &[usize; 5] = &[1, 1, 1, 1, 1];
const GIGA_CHIP_COUNTS: &[usize; 1] = &[1];
const COMBO_COUNTS: &[usize; 8] = &[1, 1, 1, 1, 1, 1, 1, 1];
const PROGRAM_ADVANCE_COUNTS: &[usize; 1] = &[1];

struct MaterializedDarkAI {
    secondary_standard_chips: [Option<usize>; 3],
    standard_chips: [Option<usize>; 16],
    mega_chips: [Option<usize>; 5],
    giga_chip: Option<usize>,
    #[allow(dead_code)]
    combos: [Option<usize>; 8],
    program_advance: Option<usize>,
}

impl MaterializedDarkAI {
    fn new(dark_ai_view: &Box<dyn save::DarkAIView + '_>, assets: &Box<dyn rom::Assets + Send + Sync>) -> Self {
        let mut use_counts = vec![];
        loop {
            if let Some(count) = dark_ai_view.chip_use_count(use_counts.len()) {
                use_counts.push(count);
            } else {
                break;
            }
        }

        let mut secondary_use_counts = vec![];
        loop {
            if let Some(count) = dark_ai_view.secondary_chip_use_count(secondary_use_counts.len()) {
                secondary_use_counts.push(count);
            } else {
                break;
            }
        }

        MaterializedDarkAI {
            secondary_standard_chips: secondary_use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == rom::ChipClass::Standard)
                        .unwrap_or(false)
                        && **count > 0
                })
                .sorted_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| Some(id))
                .chain(std::iter::repeat(None))
                .take(3)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            standard_chips: use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == rom::ChipClass::Standard)
                        .unwrap_or(false)
                        && **count > 0
                })
                .sorted_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| Some(id))
                .chain(std::iter::repeat(None))
                .take(16)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            mega_chips: use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == rom::ChipClass::Mega)
                        .unwrap_or(false)
                        && **count > 0
                })
                .sorted_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| Some(id))
                .chain(std::iter::repeat(None))
                .take(5)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            giga_chip: use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == rom::ChipClass::Giga)
                        .unwrap_or(false)
                        && **count > 0
                })
                .min_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| id),
            combos: [None; 8],
            program_advance: use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == rom::ChipClass::ProgramAdvance)
                        .unwrap_or(false)
                        && **count > 0
                })
                .min_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| id),
        }
    }
}

fn show_table<const N: usize>(
    ui: &mut egui::Ui,
    chips: &[Option<usize>; N],
    counts: &[usize; N],
    assets: &Box<dyn rom::Assets + Send + Sync>,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    chip_icon_texture_cache: &mut std::collections::HashMap<usize, egui::TextureHandle>,
    element_icon_texture_cache: &mut std::collections::HashMap<usize, egui::TextureHandle>,
) {
    egui_extras::StripBuilder::new(ui)
        .sizes(egui_extras::Size::exact(28.0), chips.len())
        .vertical(|mut outer_strip| {
            for (i, (id, count)) in std::iter::zip(chips, counts).enumerate() {
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
                                rom::ChipClass::Standard => None,
                                rom::ChipClass::Mega => Some(if ui.visuals().dark_mode {
                                    egui::Color32::from_rgb(0x52, 0x84, 0x9c)
                                } else {
                                    egui::Color32::from_rgb(0xad, 0xef, 0xef)
                                }),
                                rom::ChipClass::Giga => Some(if ui.visuals().dark_mode {
                                    egui::Color32::from_rgb(0x8c, 0x31, 0x52)
                                } else {
                                    egui::Color32::from_rgb(0xf7, 0xce, 0xe7)
                                }),
                                rom::ChipClass::None => None,
                                rom::ChipClass::ProgramAdvance => None,
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
                                ui.strong(format!("{}x", count));
                            });
                            if let Some(id) = id {
                                strip.cell(|ui| {
                                    match chip_icon_texture_cache.entry(*id) {
                                        std::collections::hash_map::Entry::Occupied(_) => {}
                                        std::collections::hash_map::Entry::Vacant(e) => {
                                            if let Some(image) = info.as_ref().map(|info| info.icon()) {
                                                e.insert(ui.ctx().load_texture(
                                                    format!("chip icon {}", id),
                                                    egui::ColorImage::from_rgba_unmultiplied(
                                                        [14, 14],
                                                        &image::imageops::crop_imm(&image, 1, 1, 14, 14).to_image(),
                                                    ),
                                                    egui::TextureFilter::Nearest,
                                                ));
                                            }
                                        }
                                    }

                                    if let Some(texture_handle) = chip_icon_texture_cache.get(&id) {
                                        ui.image(texture_handle.id(), egui::Vec2::new(28.0, 28.0));
                                    }
                                });
                                strip.cell(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(
                                                    info.as_ref()
                                                        .map(|info| info.name())
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
                                                    egui::TextureFilter::Nearest,
                                                ));
                                            }
                                        }
                                    }

                                    if let Some(texture_handle) = element_icon_texture_cache.get(&element) {
                                        ui.image(texture_handle.id(), egui::Vec2::new(28.0, 28.0));
                                    }
                                });
                                strip.cell(|ui| {
                                    let damage = info.as_ref().map(|info| info.damage()).unwrap_or(0);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if damage > 0 {
                                            ui.strong(format!("{}", damage));
                                        }
                                    });
                                });
                            } else {
                                strip.cell(|_ui| {});
                                strip.cell(|ui| {
                                    ui.weak(i18n::LOCALES.lookup(lang, "dark-ai-unset").unwrap());
                                });
                                strip.cell(|_ui| {});
                                strip.cell(|_ui| {});
                            }
                        });
                });
            }
        });
}

fn make_string<'a, const N: usize>(
    chips: &'a [Option<usize>; N],
    counts: &'a [usize; N],
    assets: &'a Box<dyn rom::Assets + Send + Sync>,
) -> impl std::iter::Iterator<Item = String> + 'a {
    std::iter::zip(chips, counts).map(|(id, count)| {
        let name = if let Some(id) = id {
            if let Some(info) = assets.chip(*id) {
                info.name()
            } else {
                "-".to_string()
            }
        } else {
            "-".to_string()
        };
        format!("{}\t{}", count, name)
    })
}

pub fn show<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    dark_ai_view: &Box<dyn save::DarkAIView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
) {
    let materialized = state
        .materialized
        .get_or_insert_with(|| MaterializedDarkAI::new(dark_ai_view, assets));

    ui.horizontal(|ui| {
        if ui
            .button(format!(
                "📋 {}",
                i18n::LOCALES.lookup(lang, "copy-to-clipboard").unwrap(),
            ))
            .clicked()
        {
            let _ = clipboard.set_text(
                make_string(
                    &materialized.secondary_standard_chips,
                    SECONDARY_STANDARD_CHIP_COUNTS,
                    assets,
                )
                .chain(make_string(&materialized.standard_chips, STANDARD_CHIP_COUNTS, assets))
                .chain(make_string(&materialized.mega_chips, MEGA_CHIP_COUNTS, assets))
                .chain(make_string(&[materialized.giga_chip], GIGA_CHIP_COUNTS, assets))
                .chain(make_string(&[None; 8], COMBO_COUNTS, assets))
                .chain(make_string(
                    &[materialized.program_advance],
                    PROGRAM_ADVANCE_COUNTS,
                    assets,
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            );
        }
    });

    egui::ScrollArea::vertical()
        .id_source("dark-ai-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.push_id(egui::Id::new("dark-ai-view-secondary-standard-chips"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "dark-ai-secondary-standard-chips").unwrap());
                show_table(
                    ui,
                    &materialized.secondary_standard_chips,
                    SECONDARY_STANDARD_CHIP_COUNTS,
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("dark-ai-view-standard-chips"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "dark-ai-standard-chips").unwrap());
                show_table(
                    ui,
                    &materialized.standard_chips,
                    STANDARD_CHIP_COUNTS,
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("dark-ai-view-mega-chips"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "dark-ai-mega-chips").unwrap());
                show_table(
                    ui,
                    &materialized.mega_chips,
                    MEGA_CHIP_COUNTS,
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("dark-ai-view-giga-chip"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "dark-ai-giga-chip").unwrap());
                show_table(
                    ui,
                    &[materialized.giga_chip],
                    GIGA_CHIP_COUNTS,
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("dark-ai-view-combos"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "dark-ai-combos").unwrap());
                show_table(
                    ui,
                    &[None; 8],
                    COMBO_COUNTS,
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("dark-ai-view-program-advance"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "dark-ai-program-advance").unwrap());
                show_table(
                    ui,
                    &[materialized.program_advance],
                    PROGRAM_ADVANCE_COUNTS,
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
