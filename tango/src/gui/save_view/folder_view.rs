use crate::{config, fonts, gui::SharedRootState, i18n};
use fluent_templates::Loader;

pub struct State {
    grouped: bool,
    chip_icon_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
    chip_image_texture_cache: std::collections::HashMap<usize, (egui::TextureHandle, [u32; 2])>,
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

struct GroupedChip {
    count: usize,
    is_regular: bool,
    has_tag1: bool,
    has_tag2: bool,
}

pub fn show(
    ui: &mut egui::Ui,
    config: &config::Config,
    shared_root_state: &mut SharedRootState,
    game_lang: &unic_langid::LanguageIdentifier,
    chips_view: &dyn tango_dataview::save::ChipsView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    state: &mut State,
) {
    let lang = &config.language;
    let font_families = &shared_root_state.font_families;
    let clipboard = &mut shared_root_state.clipboard;

    let mut chips = (0..30)
        .map(|i| chips_view.chip(chips_view.equipped_folder_index(), i))
        .collect::<Vec<_>>();

    if !assets.regular_chip_is_in_place() {
        if let Some(regular_chip_index) = chips_view.regular_chip_index(chips_view.equipped_folder_index()) {
            let chip = chips.remove(0);
            chips.insert(regular_chip_index, chip);
        }
    }

    let items = if state.grouped {
        let mut grouped = indexmap::IndexMap::new();

        for (i, chip) in chips.iter().enumerate() {
            let g = grouped.entry(chip).or_insert(GroupedChip {
                count: 0,
                is_regular: false,
                has_tag1: false,
                has_tag2: false,
            });

            g.count += 1;

            if chips_view.regular_chip_index(chips_view.equipped_folder_index()) == Some(i) {
                g.is_regular = true;
            }

            if let Some(tag_indices) = chips_view.tag_chip_indexes(chips_view.equipped_folder_index()) {
                g.has_tag1 |= tag_indices[0] == i;
                g.has_tag2 |= tag_indices[1] == i;
            }
        }

        grouped.into_iter().collect::<Vec<_>>()
    } else {
        chips
            .iter()
            .enumerate()
            .map(|(i, chip)| {
                let [has_tag1, has_tag2] = chips_view
                    .tag_chip_indexes(chips_view.equipped_folder_index())
                    .map(|tag_indices| [tag_indices[0] == i, tag_indices[1] == i])
                    .unwrap_or_default();

                (
                    chip,
                    GroupedChip {
                        count: 1,
                        is_regular: chips_view.regular_chip_index(chips_view.equipped_folder_index()) == Some(i),
                        has_tag1,
                        has_tag2,
                    },
                )
            })
            .collect::<Vec<_>>()
    };

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        ui.checkbox(&mut state.grouped, i18n::LOCALES.lookup(lang, "save-group").unwrap());

        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            let as_text_text = i18n::LOCALES.lookup(lang, "copy-to-clipboard.as-text").unwrap();
            let as_image_text = i18n::LOCALES.lookup(lang, "copy-to-clipboard.as-image").unwrap();

            if ui.button(as_text_text).clicked() {
                ui.close_menu();

                let _ = clipboard.set_text(chips_to_string(assets, &items, state.grouped));
            }

            if ui.button(as_image_text).clicked() {
                ui.close_menu();

                let mut state = State {
                    grouped: state.grouped,
                    ..State::new()
                };

                shared_root_state.offscreen_ui.resize(400, 0);
                shared_root_state.offscreen_ui.run(|ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(8, 0))
                        .fill(ui.style().visuals.panel_fill)
                        .show(ui, |ui| {
                            show_chips(ui, assets, font_families, game_lang, &items, &mut state);
                        });
                });
                shared_root_state.offscreen_ui.copy_to_clipboard();
                shared_root_state.offscreen_ui.sweep();
            }
        });
    });

    ui.style_mut().visuals.clip_rect_margin = 0.0;

    egui::ScrollArea::vertical()
        .id_salt("folder-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            show_chips(ui, assets, font_families, game_lang, &items, state);
        });
}

fn chips_to_string(
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    items: &[(&Option<tango_dataview::save::Chip>, GroupedChip)],
    grouped: bool,
) -> String {
    items
        .iter()
        .map(|(chip, g)| {
            let mut buf = String::new();
            if let Some(chip) = chip {
                if grouped {
                    buf.push_str(&format!("{}\t", g.count));
                }
                let info = assets.chip(chip.id);
                buf.push_str(&format!(
                    "{}\t{}\t",
                    info.and_then(|info| info.as_ref().name())
                        .unwrap_or_else(|| "???".to_string()),
                    chip.code
                ));
            } else {
                buf.push_str("???");
            }
            if g.is_regular {
                buf.push_str("[REG]");
            }
            if g.has_tag1 {
                buf.push_str("[TAG1]");
            }
            if g.has_tag2 {
                buf.push_str("[TAG2]");
            }
            buf
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn show_chips(
    ui: &mut egui::Ui,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    font_families: &fonts::FontFamilies,
    game_lang: &unic_langid::LanguageIdentifier,
    items: &[(&Option<tango_dataview::save::Chip>, GroupedChip)],
    state: &mut State,
) {
    let spacing = ui.spacing_mut();
    spacing.item_spacing.y = 0.0;

    egui_extras::StripBuilder::new(ui)
        .sizes(egui_extras::Size::exact(32.0), items.len())
        .vertical(|mut outer_strip| {
            for (i, (chip, g)) in items.iter().enumerate() {
                outer_strip.cell(|ui| {
                    let info = chip.as_ref().and_then(|chip| assets.chip(chip.id));

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

                    let rect = ui
                        .available_rect_before_wrap()
                        .expand2(egui::Vec2::new(ui.spacing().item_spacing.x, 0.0));

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
                    if assets.chips_have_mb() {
                        sb = sb.size(egui_extras::Size::exact(50.0));
                    }

                    sb.horizontal(|mut strip| {
                        if state.grouped {
                            strip.cell(|ui| {
                                ui.strong(format!("{}x", g.count));
                            });
                        }
                        strip.cell(|ui| {
                            let Some(chip) = chip.as_ref() else {
                                return;
                            };

                            match state.chip_icon_texture_cache.entry(chip.id) {
                                std::collections::hash_map::Entry::Occupied(_) => {}
                                std::collections::hash_map::Entry::Vacant(e) => {
                                    if let Some(image) = info.as_ref().map(|info| info.icon()) {
                                        e.insert(ui.ctx().load_texture(
                                            format!("chip icon {}", chip.id),
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [14, 14],
                                                &image::imageops::crop_imm(&image, 1, 1, 14, 14).to_image(),
                                            ),
                                            egui::TextureOptions::NEAREST,
                                        ));
                                    }
                                }
                            }

                            if let Some(texture_handle) = state.chip_icon_texture_cache.get(&chip.id) {
                                ui.image((texture_handle.id(), egui::Vec2::new(28.0, 28.0)))
                                    .on_hover_ui(|ui| {
                                        match state.chip_image_texture_cache.entry(chip.id) {
                                            std::collections::hash_map::Entry::Occupied(_) => {}
                                            std::collections::hash_map::Entry::Vacant(e) => {
                                                if let Some(image) = info.as_ref().map(|info| info.image()) {
                                                    e.insert((
                                                        ui.ctx().load_texture(
                                                            format!("chip image {}", chip.id),
                                                            egui::ColorImage::from_rgba_unmultiplied(
                                                                [image.width() as usize, image.height() as usize],
                                                                &image,
                                                            ),
                                                            egui::TextureOptions::NEAREST,
                                                        ),
                                                        [image.width(), image.height()],
                                                    ));
                                                }
                                            }
                                        }

                                        if let Some((texture_handle, [width, height])) =
                                            state.chip_image_texture_cache.get(&chip.id)
                                        {
                                            ui.image((
                                                texture_handle.id(),
                                                egui::Vec2::new(*width as f32 * 2.0, *height as f32 * 2.0),
                                            ));
                                        }
                                    });
                            }
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
                                            &info
                                                .as_ref()
                                                .and_then(|info| info.name())
                                                .unwrap_or_else(|| "???".to_string()),
                                            0.0,
                                            egui::TextFormat::simple(
                                                name_style,
                                                fg_color.unwrap_or(ui.visuals().text_color()),
                                            ),
                                        );
                                        layout_job.append(
                                            &format!(" {}", chip.code),
                                            0.0,
                                            egui::TextFormat::simple(
                                                ui.style().text_styles.get(&egui::TextStyle::Body).unwrap().clone(),
                                                fg_color.unwrap_or(ui.visuals().text_color()),
                                            ),
                                        );

                                        ui.label(layout_job).on_hover_text(
                                            egui::RichText::new(
                                                info.as_ref()
                                                    .and_then(|info| info.description())
                                                    .unwrap_or_else(|| "???".to_string()),
                                            )
                                            .family(font_families.for_language(game_lang)),
                                        );
                                    } else {
                                        ui.label("???");
                                    };
                                });

                                // regular chip label
                                if g.is_regular {
                                    egui::Frame::new()
                                        .inner_margin(egui::Margin::symmetric(4, 0))
                                        .corner_radius(egui::CornerRadius::same(2))
                                        .fill(egui::Color32::from_rgb(0xff, 0x42, 0xa5))
                                        .show(ui, |ui| {
                                            ui.label(egui::RichText::new("REG").color(egui::Color32::WHITE));
                                        });
                                }

                                // tag chip labels
                                let mut tag_start = 1;
                                let mut tag_end = 3;

                                if !g.has_tag1 {
                                    tag_start += 1;
                                }

                                if !g.has_tag2 {
                                    tag_end -= 1;
                                }

                                for n in tag_start..tag_end {
                                    egui::Frame::new()
                                        .inner_margin(egui::Margin::symmetric(4, 0))
                                        .corner_radius(egui::CornerRadius::same(2))
                                        .fill(egui::Color32::from_rgb(0x29, 0xf7, 0x21))
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new(format!("TAG{}", n)).color(egui::Color32::WHITE),
                                            );
                                        });
                                }
                            });
                        });
                        strip.cell(|ui| {
                            let element = if let Some(element) = info.as_ref().map(|info| info.element()) {
                                element
                            } else {
                                return;
                            };

                            match state.element_icon_texture_cache.entry(element) {
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

                            if let Some(texture_handle) = state.element_icon_texture_cache.get(&element) {
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
                        if assets.chips_have_mb() {
                            strip.cell(|ui| {
                                let mb = info.as_ref().map(|info| info.mb()).unwrap_or(0);
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
}
