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

struct MaterializedDarkAI {
    secondary_standard_chips: [Option<usize>; 3],
    standard_chips: [Option<usize>; 16],
    mega_chips: [Option<usize>; 5],
    giga_chip: Option<usize>,
    combos: [Option<usize>; 8],
    program_advance: Option<usize>,
}

impl MaterializedDarkAI {
    fn new(
        dark_ai_view: &Box<dyn save::DarkAIView + '_>,
        assets: &Box<dyn rom::Assets + Send + Sync>,
    ) -> Self {
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
                        .map(|c| c.class == rom::ChipClass::Standard)
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
                        .map(|c| c.class == rom::ChipClass::Standard)
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
                        .map(|c| c.class == rom::ChipClass::Mega)
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
                        .map(|c| c.class == rom::ChipClass::Giga)
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
                        .map(|c| c.class == rom::ChipClass::ProgramAdvance)
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
    egui_extras::TableBuilder::new(ui)
        .scroll(false)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(egui_extras::Size::exact(30.0))
        .column(egui_extras::Size::exact(28.0))
        .column(egui_extras::Size::remainder())
        .column(egui_extras::Size::exact(28.0))
        .column(egui_extras::Size::exact(30.0))
        .body(|mut body| {
            for (id, count) in std::iter::zip(chips, counts) {
                body.row(28.0, |mut row| {
                    row.col(|ui| {
                        ui.strong(format!("{}x", count));
                    });
                    if let Some(id) = id {
                        let info = assets.chip(*id);
                        row.col(|ui| {
                            let icon = if let Some(icon) = info.map(|info| &info.icon) {
                                icon
                            } else {
                                return;
                            };

                            ui.image(
                                chip_icon_texture_cache
                                    .entry(*id)
                                    .or_insert_with(|| {
                                        ui.ctx().load_texture(
                                            format!("chip {}", id),
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
                                    ui.label(
                                        egui::RichText::new(
                                            info.map(|info| info.name.as_str()).unwrap_or("???"),
                                        )
                                        .family(font_families.for_language(game_lang)),
                                    );
                                });
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
                                element_icon_texture_cache
                                    .entry(element)
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
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if damage > 0 {
                                        ui.strong(format!("{}", damage));
                                    }
                                },
                            );
                        });
                    } else {
                        row.col(|_ui| {});
                        row.col(|ui| {
                            ui.weak(i18n::LOCALES.lookup(lang, "dark-ai.unset").unwrap());
                        });
                        row.col(|_ui| {});
                        row.col(|_ui| {});
                    }
                });
            }
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
    egui::ScrollArea::vertical()
        .id_source("dark-ai-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.push_id(
                egui::Id::new("dark-ai-view-secondary-standard-chips"),
                |ui| {
                    ui.strong(
                        i18n::LOCALES
                            .lookup(lang, "dark-ai.secondary-standard-chips")
                            .unwrap(),
                    );
                    show_table(
                        ui,
                        &materialized.secondary_standard_chips,
                        &[1, 1, 1],
                        assets,
                        font_families,
                        lang,
                        game_lang,
                        &mut state.chip_icon_texture_cache,
                        &mut state.element_icon_texture_cache,
                    );
                },
            );

            ui.push_id(egui::Id::new("dark-ai-view-standard-chips"), |ui| {
                ui.strong(
                    i18n::LOCALES
                        .lookup(lang, "dark-ai.standard-chips")
                        .unwrap(),
                );
                show_table(
                    ui,
                    &materialized.standard_chips,
                    &[4, 4, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("dark-ai-view-mega-chips"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "dark-ai.mega-chips").unwrap());
                show_table(
                    ui,
                    &materialized.mega_chips,
                    &[1, 1, 1, 1, 1],
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("dark-ai-view-giga-chip"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "dark-ai.giga-chip").unwrap());
                show_table(
                    ui,
                    &[materialized.giga_chip],
                    &[1],
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("dark-ai-view-combos"), |ui| {
                ui.strong(i18n::LOCALES.lookup(lang, "dark-ai.combos").unwrap());
                show_table(
                    ui,
                    &[None; 8],
                    &[1; 8],
                    assets,
                    font_families,
                    lang,
                    game_lang,
                    &mut state.chip_icon_texture_cache,
                    &mut state.element_icon_texture_cache,
                );
            });

            ui.push_id(egui::Id::new("dark-ai-view-program-advance"), |ui| {
                ui.strong(
                    i18n::LOCALES
                        .lookup(lang, "dark-ai.program-advance")
                        .unwrap(),
                );
                show_table(
                    ui,
                    &[materialized.program_advance],
                    &[1],
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
