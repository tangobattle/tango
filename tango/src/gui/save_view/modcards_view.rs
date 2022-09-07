use fluent_templates::Loader;

use crate::{gui, i18n, rom, save};

pub struct State {}

impl State {
    pub fn new() -> Self {
        Self {}
    }
}

fn show_effect(ui: &mut egui::Ui, name: egui::RichText, is_enabled: bool, is_debuff: bool) {
    egui::Frame::none()
        .inner_margin(egui::style::Margin::symmetric(4.0, 0.0))
        .rounding(egui::Rounding::same(2.0))
        .fill(if is_enabled {
            if is_debuff {
                egui::Color32::from_rgb(0xb5, 0x5a, 0xde)
            } else {
                egui::Color32::from_rgb(0xff, 0xbd, 0x18)
            }
        } else {
            egui::Color32::from_rgb(0xbd, 0xbd, 0xbd)
        })
        .show(ui, |ui| {
            ui.label(name.color(egui::Color32::BLACK));
        });
}

pub fn show_modcard4s<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    modcard4s_view: &Box<dyn save::Modcard4sView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    _state: &mut State,
) {
    ui.horizontal(|ui| {
        if ui
            .button(format!(
                "📋 {}",
                i18n::LOCALES.lookup(lang, "copy-to-clipboard").unwrap(),
            ))
            .clicked()
        {
            let _ = clipboard.set_text(
                (0..6)
                    .map(|i| {
                        let modcard = modcard4s_view.modcard(i);
                        if let Some(modcard) = modcard {
                            if modcard.enabled {
                                format!("{:03}", modcard.id)
                            } else {
                                "---".to_owned()
                            }
                        } else {
                            "---".to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    });

    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let spacing_y = ui.spacing().item_spacing.y;
    egui_extras::TableBuilder::new(ui)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(egui_extras::Size::remainder())
        .column(egui_extras::Size::exact(250.0))
        .striped(true)
        .body(|body| {
            body.rows(row_height * 2.0 + spacing_y, 6, |i, mut row| {
                let modcard = modcard4s_view.modcard(i);
                if let Some((modcard, info)) = modcard
                    .as_ref()
                    .and_then(|modcard| assets.modcard4(modcard.id).map(|info| (modcard, info)))
                {
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            let mut name_label = egui::RichText::new(format!("#{:03} {}", modcard.id, info.name))
                                .family(font_families.for_language(game_lang));
                            if !modcard.enabled {
                                name_label = name_label.strikethrough();
                            }

                            let mut slot_label =
                                egui::RichText::new(format!("0{}", ['A', 'B', 'C', 'D', 'E', 'F'][i])).small();
                            if !modcard.enabled {
                                slot_label = slot_label.strikethrough();
                            }

                            ui.label(name_label);
                            ui.label(slot_label);
                        });
                    });
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                                show_effect(
                                    ui,
                                    egui::RichText::new(info.effect).family(font_families.for_language(game_lang)),
                                    modcard.enabled,
                                    false,
                                );

                                if let Some(bug) = info.bug {
                                    show_effect(
                                        ui,
                                        egui::RichText::new(bug).family(font_families.for_language(game_lang)),
                                        modcard.enabled,
                                        true,
                                    );
                                }
                            });
                        });
                    });
                } else {
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            ui.label("---");
                            ui.label(
                                egui::RichText::new(format!("0{}", ['A', 'B', 'C', 'D', 'E', 'F'][i]))
                                    .small()
                                    .strikethrough(),
                            );
                        });
                    });
                    row.col(|_ui| {});
                }
            });
        });
}

pub fn show_modcard56s<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    modcard56s_view: &Box<dyn save::Modcard56sView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    _state: &mut State,
) {
    let items = (0..modcard56s_view.count())
        .map(|slot| {
            let modcard = modcard56s_view.modcard(slot);
            let effects = modcard
                .as_ref()
                .and_then(|item| assets.modcard56(item.id))
                .map(|info| info.effects.as_slice())
                .unwrap_or(&[][..]);
            (modcard, effects)
        })
        .collect::<Vec<_>>();

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
                    .flat_map(|(modcard, _)| {
                        let modcard = if let Some(modcard) = modcard.as_ref() {
                            modcard
                        } else {
                            return vec![];
                        };

                        if !modcard.enabled {
                            return vec![];
                        }

                        let modcard = if let Some(modcard) = assets.modcard56(modcard.id) {
                            modcard
                        } else {
                            return vec![];
                        };

                        vec![format!("{}\t{}", modcard.name, modcard.mb)]
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    });

    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let spacing_y = ui.spacing().item_spacing.y;
    egui_extras::TableBuilder::new(ui)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(egui_extras::Size::remainder())
        .column(egui_extras::Size::exact(150.0))
        .column(egui_extras::Size::exact(150.0))
        .striped(true)
        .body(|body| {
            body.heterogeneous_rows(
                items.iter().map(|(_, effects)| {
                    let num_effects = std::cmp::max(
                        effects.iter().filter(|effect| effect.is_ability).count(),
                        effects.iter().filter(|effect| !effect.is_ability).count(),
                    );
                    num_effects as f32 * row_height + num_effects as f32 * spacing_y - spacing_y * 0.5
                }),
                |i, mut row| {
                    let (modcard, effects) = &items[i];
                    row.col(|ui| {
                        if let Some((modcard, enabled)) = modcard
                            .as_ref()
                            .and_then(|modcard| assets.modcard56(modcard.id).map(|m| (m, modcard.enabled)))
                        {
                            let mut text =
                                egui::RichText::new(&modcard.name).family(font_families.for_language(game_lang));
                            if !enabled {
                                text = text.strikethrough();
                            }
                            ui.vertical(|ui| {
                                ui.label(text);
                                ui.small(format!("{}MB", modcard.mb));
                            });
                        }
                    });

                    row.col(|ui| {
                        ui.vertical(|ui| {
                            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                                for effect in *effects {
                                    if effect.is_ability {
                                        continue;
                                    }

                                    show_effect(
                                        ui,
                                        egui::RichText::new(&effect.name).family(font_families.for_language(game_lang)),
                                        modcard.as_ref().map(|modcard| modcard.enabled).unwrap_or(false),
                                        effect.is_debuff,
                                    );
                                }
                            });
                        });
                    });
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                                for effect in *effects {
                                    if !effect.is_ability {
                                        continue;
                                    }

                                    show_effect(
                                        ui,
                                        egui::RichText::new(&effect.name).family(font_families.for_language(game_lang)),
                                        modcard.as_ref().map(|modcard| modcard.enabled).unwrap_or(false),
                                        effect.is_debuff,
                                    );
                                }
                            });
                        });
                    });
                },
            );
        });
}

pub fn show<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    modcards_view: &save::ModcardsView,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
) {
    match modcards_view {
        save::ModcardsView::Modcard4s(modcard4s_view) => show_modcard4s(
            ui,
            clipboard,
            font_families,
            lang,
            game_lang,
            modcard4s_view,
            assets,
            state,
        ),
        save::ModcardsView::Modcard56s(modcard56s_view) => show_modcard56s(
            ui,
            clipboard,
            font_families,
            lang,
            game_lang,
            modcard56s_view,
            assets,
            state,
        ),
    }
}
