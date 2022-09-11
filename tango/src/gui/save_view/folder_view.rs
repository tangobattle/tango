use fluent_templates::Loader;

use crate::{gui, i18n, rom, save};

pub struct State {
    grouped: bool,
    chip_icon_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
    chip_image_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
    element_icon_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
}

impl State {
    pub fn new() -> Self {
        Self {
            grouped: true,
            chip_icon_texture_cache: std::collections::HashMap::new(),
            chip_image_texture_cache: std::collections::HashMap::new(),
            element_icon_texture_cache: std::collections::HashMap::new(),
        }
    }
}

pub fn show<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    chips_view: &Box<dyn save::ChipsView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
) {
    struct GroupedChip {
        count: usize,
        is_regular: bool,
        tag_count: usize,
    }

    let mut chips = (0..30)
        .map(|i| chips_view.chip(chips_view.equipped_folder_index(), i))
        .collect::<Vec<_>>();

    if !chips_view.regular_chip_is_in_place() {
        if let Some(regular_chip_index) = chips_view.regular_chip_index(chips_view.equipped_folder_index()) {
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
                        is_regular: chips_view.regular_chip_index(chips_view.equipped_folder_index()) == Some(i),
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
                        if let Some(chip) = chip {
                            if state.grouped {
                                buf.push_str(&format!("{}\t", g.count));
                            }
                            let info = assets.chip(chip.id);
                            buf.push_str(&format!(
                                "{}\t{}\t",
                                info.map(|info| info.name.as_str()).unwrap_or("???"),
                                chips_view.chip_codes()[chip.code] as char
                            ));
                        } else {
                            buf.push_str("???");
                        }
                        if g.is_regular {
                            buf.push_str("[REG]");
                        }
                        for _ in 0..g.tag_count {
                            buf.push_str("[TAG]");
                        }
                        buf
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        ui.checkbox(&mut state.grouped, i18n::LOCALES.lookup(lang, "save-group").unwrap());
    });

    egui::ScrollArea::vertical()
        .id_source("dark-ai-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui_extras::StripBuilder::new(ui)
                .sizes(egui_extras::Size::exact(28.0), items.len())
                .vertical(|mut outer_strip| {
                    for (i, (chip, g)) in items.iter().enumerate() {
                        outer_strip.cell(|ui| {
                            let info = chip.as_ref().and_then(|chip| assets.chip(chip.id));

                            let (bg_color, fg_color) = if let Some(info) = info {
                                let bg_color = if info.dark {
                                    Some(if ui.visuals().dark_mode {
                                        egui::Color32::from_rgb(0x31, 0x39, 0x5a)
                                    } else {
                                        egui::Color32::from_rgb(0xb5, 0x8c, 0xd6)
                                    })
                                } else {
                                    match info.class {
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

                            let mut sb = egui_extras::StripBuilder::new(ui)
                                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));

                            if state.grouped {
                                sb = sb.size(egui_extras::Size::exact(30.0));
                            }

                            sb = sb
                                .size(egui_extras::Size::exact(28.0))
                                .size(egui_extras::Size::remainder())
                                .size(egui_extras::Size::exact(28.0))
                                .size(egui_extras::Size::exact(30.0));
                            if chips_view.chips_have_mb() {
                                sb = sb.size(egui_extras::Size::exact(50.0));
                            }

                            sb.horizontal(|mut strip| {
                                if state.grouped {
                                    strip.cell(|ui| {
                                        ui.strong(format!("{}x", g.count));
                                    });
                                }
                                strip.cell(|ui| {
                                    let icon = if let Some(icon) = info.map(|info| &info.icon) {
                                        icon
                                    } else {
                                        return;
                                    };

                                    let chip = if let Some(chip) = chip.as_ref() {
                                        chip
                                    } else {
                                        return;
                                    };

                                    ui.image(
                                        state
                                            .chip_icon_texture_cache
                                            .entry(chip.id)
                                            .or_insert_with(|| {
                                                ui.ctx().load_texture(
                                                    format!("chip icon {}", chip.id),
                                                    egui::ColorImage::from_rgba_unmultiplied(
                                                        [14, 14],
                                                        &image::imageops::crop_imm(icon, 1, 1, 14, 14).to_image(),
                                                    ),
                                                    egui::TextureFilter::Nearest,
                                                )
                                            })
                                            .id(),
                                        egui::Vec2::new(28.0, 28.0),
                                    )
                                    .on_hover_ui(|ui| {
                                        if let Some(image) = info.map(|info| &info.image) {
                                            ui.image(
                                                state
                                                    .chip_image_texture_cache
                                                    .entry(chip.id)
                                                    .or_insert_with(|| {
                                                        ui.ctx().load_texture(
                                                            format!("chip image {}", chip.id),
                                                            egui::ColorImage::from_rgba_unmultiplied(
                                                                [image.width() as usize, image.height() as usize],
                                                                &image,
                                                            ),
                                                            egui::TextureFilter::Nearest,
                                                        )
                                                    })
                                                    .id(),
                                                egui::Vec2::new(
                                                    image.width() as f32 * 2.0,
                                                    image.height() as f32 * 2.0,
                                                ),
                                            );
                                        }
                                    });
                                });
                                strip.cell(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 0.0;
                                            if let Some(chip) = chip.as_ref() {
                                                let mut layout_job = egui::text::LayoutJob::default();
                                                let mut name_style =
                                                    ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone();
                                                name_style.family = font_families.for_language(game_lang);
                                                layout_job.append(
                                                    info.map(|info| info.name.as_str()).unwrap_or("???"),
                                                    0.0,
                                                    egui::TextFormat::simple(name_style, ui.visuals().text_color()),
                                                );
                                                layout_job.append(
                                                    &format!(" {}", chips_view.chip_codes()[chip.code] as char),
                                                    0.0,
                                                    egui::TextFormat::simple(
                                                        ui.style()
                                                            .text_styles
                                                            .get(&egui::TextStyle::Body)
                                                            .unwrap()
                                                            .clone(),
                                                        ui.visuals().text_color(),
                                                    ),
                                                );

                                                ui.label(layout_job).on_hover_text(
                                                    egui::RichText::new(
                                                        info.map(|info| info.description.as_str()).unwrap_or("???"),
                                                    )
                                                    .color(fg_color.unwrap_or(ui.visuals().text_color()))
                                                    .family(font_families.for_language(game_lang)),
                                                );
                                            } else {
                                                ui.label("???");
                                            };
                                        });
                                        if g.is_regular {
                                            egui::Frame::none()
                                                .inner_margin(egui::style::Margin::symmetric(4.0, 0.0))
                                                .rounding(egui::Rounding::same(2.0))
                                                .fill(egui::Color32::from_rgb(0xff, 0x42, 0xa5))
                                                .show(ui, |ui| {
                                                    ui.label(egui::RichText::new("REG").color(egui::Color32::WHITE));
                                                });
                                        }
                                        for _ in 0..g.tag_count {
                                            egui::Frame::none()
                                                .inner_margin(egui::style::Margin::symmetric(4.0, 0.0))
                                                .rounding(egui::Rounding::same(2.0))
                                                .fill(egui::Color32::from_rgb(0x29, 0xf7, 0x21))
                                                .show(ui, |ui| {
                                                    ui.label(egui::RichText::new("TAG").color(egui::Color32::WHITE));
                                                });
                                        }
                                    });
                                });
                                strip.cell(|ui| {
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
                                        state
                                            .element_icon_texture_cache
                                            .entry(element)
                                            .or_insert_with(|| {
                                                ui.ctx().load_texture(
                                                    format!("element {}", element),
                                                    egui::ColorImage::from_rgba_unmultiplied(
                                                        [14, 14],
                                                        &image::imageops::crop_imm(icon, 1, 1, 14, 14).to_image(),
                                                    ),
                                                    egui::TextureFilter::Nearest,
                                                )
                                            })
                                            .id(),
                                        egui::Vec2::new(28.0, 28.0),
                                    );
                                });
                                strip.cell(|ui| {
                                    let damage = info.map(|info| info.damage).unwrap_or(0);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if damage > 0 {
                                            ui.strong(format!("{}", damage));
                                        }
                                    });
                                });
                                if chips_view.chips_have_mb() {
                                    strip.cell(|ui| {
                                        let mb = info.map(|info| info.mb).unwrap_or(0);
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if mb > 0 {
                                                ui.label(egui::RichText::new(format!("{}MB", mb)).color(
                                                    if bg_color.is_some() {
                                                        ui.visuals().strong_text_color()
                                                    } else {
                                                        ui.visuals().text_color()
                                                    },
                                                ));
                                            }
                                        });
                                    });
                                }
                            });
                        });
                    }
                });
        });
}
