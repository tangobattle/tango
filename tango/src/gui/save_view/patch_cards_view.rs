use crate::gui::SharedRootState;
use crate::{config, fonts, i18n};
use fluent_templates::Loader;

pub struct State {}

impl State {
    pub fn new() -> Self {
        Self {}
    }
}

fn show_effect(ui: &mut egui::Ui, name: egui::RichText, is_enabled: bool, is_debuff: bool) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(4, 0))
        .corner_radius(egui::CornerRadius::same(2))
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

fn patch_card4s_string(patch_card4s_view: &dyn tango_dataview::save::PatchCard4sView) -> String {
    (0..6)
        .map(|i| {
            let patch_card = patch_card4s_view.patch_card(i);

            if let Some(patch_card) = patch_card {
                if patch_card.enabled {
                    format!("{:03}", if patch_card.id != 133 { patch_card.id } else { 0 })
                } else {
                    "---".to_owned()
                }
            } else {
                "---".to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn show_patch_card4s(
    ui: &mut egui::Ui,
    font_families: &fonts::FontFamilies,
    game_lang: &unic_langid::LanguageIdentifier,
    patch_card4s_view: &dyn tango_dataview::save::PatchCard4sView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    _state: &mut State,
) {
    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let spacing = ui.spacing_mut();
    let spacing_y = spacing.item_spacing.y;
    spacing.item_spacing.y = 0.0;

    egui_extras::StripBuilder::new(ui)
        .sizes(egui_extras::Size::exact(row_height * 2.0 + spacing_y * 3.0), 6)
        .vertical(|mut outer_strip| {
            for i in 0..6 {
                let patch_card = patch_card4s_view.patch_card(i);
                outer_strip.cell(|ui| {
                    let rect = ui
                        .available_rect_before_wrap()
                        .expand2(egui::Vec2::new(ui.spacing().item_spacing.x, 0.0));

                    if i % 2 == 0 {
                        ui.painter().rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                    }

                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(0, spacing_y as _))
                        .show(ui, |ui| {
                            let spacing = ui.spacing_mut();
                            spacing.item_spacing.y = spacing_y;

                            egui_extras::StripBuilder::new(ui)
                                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                .size(egui_extras::Size::remainder())
                                .size(egui_extras::Size::exact(250.0))
                                .horizontal(|mut strip| {
                                    if let Some((patch_card, info)) = patch_card.as_ref().and_then(|patch_card| {
                                        assets.patch_card4(patch_card.id).map(|info| (patch_card, info))
                                    }) {
                                        strip.cell(|ui| {
                                            ui.vertical(|ui| {
                                                let mut name_label = egui::RichText::new(format!(
                                                    "#{:03} {}",
                                                    if patch_card.id != 133 { patch_card.id } else { 0 },
                                                    info.name().unwrap_or_else(|| "???".to_string())
                                                ))
                                                .family(font_families.for_language(game_lang));
                                                if !patch_card.enabled {
                                                    name_label = name_label.strikethrough();
                                                }

                                                let mut slot_label = egui::RichText::new(format!(
                                                    "0{}",
                                                    ['A', 'B', 'C', 'D', 'E', 'F'][i]
                                                ))
                                                .small();
                                                if !patch_card.enabled {
                                                    slot_label = slot_label.strikethrough();
                                                }

                                                ui.label(name_label);
                                                ui.label(slot_label);
                                            });
                                        });
                                        strip.cell(|ui| {
                                            ui.vertical(|ui| {
                                                ui.with_layout(
                                                    egui::Layout::top_down_justified(egui::Align::Min),
                                                    |ui| {
                                                        show_effect(
                                                            ui,
                                                            egui::RichText::new(
                                                                info.effect().unwrap_or_else(|| "???".to_string()),
                                                            )
                                                            .family(font_families.for_language(game_lang)),
                                                            patch_card.enabled,
                                                            false,
                                                        );

                                                        if let Some(bug) = info.bug() {
                                                            show_effect(
                                                                ui,
                                                                egui::RichText::new(bug)
                                                                    .family(font_families.for_language(game_lang)),
                                                                patch_card.enabled,
                                                                true,
                                                            );
                                                        }
                                                    },
                                                );
                                            });
                                        });
                                    } else {
                                        strip.cell(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label("---");
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "0{}",
                                                        ['A', 'B', 'C', 'D', 'E', 'F'][i]
                                                    ))
                                                    .small()
                                                    .strikethrough(),
                                                );
                                            });
                                        });
                                        strip.cell(|_ui| {});
                                    }
                                });
                        });
                });
            }
        });
}

fn patch_card56s_string(
    patch_card56s_view: &dyn tango_dataview::save::PatchCard56sView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
) -> String {
    (0..patch_card56s_view.count())
        .flat_map(|slot| {
            let Some(patch_card) = patch_card56s_view.patch_card(slot) else {
                return vec![];
            };

            if !patch_card.enabled {
                return vec![];
            }

            let Some(patch_card) = assets.patch_card56(patch_card.id) else {
                return vec![];
            };

            vec![format!(
                "{}\t{}",
                patch_card.name().unwrap_or_else(|| "???".to_string()),
                patch_card.mb()
            )]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn show_patch_card56s(
    ui: &mut egui::Ui,
    font_families: &fonts::FontFamilies,
    game_lang: &unic_langid::LanguageIdentifier,
    patch_card56s_view: &dyn tango_dataview::save::PatchCard56sView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    _state: &mut State,
) {
    let items = (0..patch_card56s_view.count())
        .map(|slot| {
            let patch_card = patch_card56s_view.patch_card(slot);
            let effects = patch_card
                .as_ref()
                .and_then(|item| assets.patch_card56(item.id))
                .map(|info| info.effects())
                .unwrap_or_default();
            (patch_card, effects)
        })
        .collect::<Vec<_>>();

    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let spacing = ui.spacing_mut();
    let spacing_y = spacing.item_spacing.y;
    spacing.item_spacing.y = 0.0;

    let mut strip_builder = egui_extras::StripBuilder::new(ui);
    for (patch_card, effects) in items.iter() {
        let Some(patch_card) = patch_card else {
            continue;
        };

        let num_text_lines = assets
            .patch_card56(patch_card.id)
            .and_then(|p| p.name())
            .map(|s| s.chars().filter(|c| *c == '\n').count() + 1)
            .unwrap_or(1);

        let num_effects = std::cmp::max(
            effects.iter().filter(|effect| effect.is_ability).count(),
            effects.iter().filter(|effect| !effect.is_ability).count(),
        );

        let num_rows = num_effects.max(num_text_lines + 1);
        strip_builder = strip_builder.size(egui_extras::Size::exact(
            num_rows as f32 * row_height + (num_rows + 1) as f32 * spacing_y,
        ));
    }

    strip_builder.vertical(|mut outer_strip| {
        for (i, (patch_card, effects)) in items.iter().enumerate() {
            outer_strip.cell(|ui| {
                let spacing = ui.spacing_mut();
                spacing.item_spacing.y = spacing_y;

                let rect = ui
                    .available_rect_before_wrap()
                    .expand2(egui::Vec2::new(ui.spacing().item_spacing.x, 0.0));

                if i % 2 == 0 {
                    ui.painter().rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                }

                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(0, spacing_y as _))
                    .show(ui, |ui| {
                        egui_extras::StripBuilder::new(ui)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .size(egui_extras::Size::remainder())
                            .size(egui_extras::Size::exact(150.0))
                            .size(egui_extras::Size::exact(150.0))
                            .horizontal(|mut strip| {
                                strip.cell(|ui| {
                                    if let Some((patch_card, enabled)) = patch_card.as_ref().and_then(|patch_card| {
                                        assets.patch_card56(patch_card.id).map(|m| (m, patch_card.enabled))
                                    }) {
                                        let mut text =
                                            egui::RichText::new(patch_card.name().unwrap_or_else(|| "???".to_string()))
                                                .family(font_families.for_language(game_lang));
                                        if !enabled {
                                            text = text.strikethrough();
                                        }
                                        ui.vertical(|ui| {
                                            ui.label(text);
                                            ui.small(format!("{}MB", patch_card.mb()));
                                        });
                                    }
                                });

                                strip.cell(|ui| {
                                    ui.vertical(|ui| {
                                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                                            for effect in effects.iter().filter(|effect| effect.is_ability) {
                                                show_effect(
                                                    ui,
                                                    egui::RichText::new(
                                                        effect.name.clone().unwrap_or_else(|| "???".to_string()),
                                                    )
                                                    .family(font_families.for_language(game_lang)),
                                                    patch_card
                                                        .as_ref()
                                                        .map(|patch_card| patch_card.enabled)
                                                        .unwrap_or(false),
                                                    effect.is_debuff,
                                                );
                                            }
                                        });
                                    });
                                });
                                strip.cell(|ui| {
                                    ui.vertical(|ui| {
                                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                                            for effect in effects.iter().filter(|effect| !effect.is_ability) {
                                                show_effect(
                                                    ui,
                                                    egui::RichText::new(
                                                        effect.name.clone().unwrap_or_else(|| "???".to_string()),
                                                    )
                                                    .family(font_families.for_language(game_lang)),
                                                    patch_card
                                                        .as_ref()
                                                        .map(|patch_card| patch_card.enabled)
                                                        .unwrap_or(false),
                                                    effect.is_debuff,
                                                );
                                            }
                                        });
                                    });
                                });
                            });
                    });
            });
        }
    });
}

fn patch_cards_string(
    patch_cards_view: &tango_dataview::save::PatchCardsView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
) -> String {
    match patch_cards_view {
        tango_dataview::save::PatchCardsView::PatchCard4s(patch_card4s_view) => {
            patch_card4s_string(patch_card4s_view.as_ref())
        }
        tango_dataview::save::PatchCardsView::PatchCard56s(patch_card56s_view) => {
            patch_card56s_string(patch_card56s_view.as_ref(), assets)
        }
    }
}

fn show_patch_cards(
    ui: &mut egui::Ui,
    font_families: &fonts::FontFamilies,
    game_lang: &unic_langid::LanguageIdentifier,
    patch_cards_view: &tango_dataview::save::PatchCardsView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    state: &mut State,
) {
    match patch_cards_view {
        tango_dataview::save::PatchCardsView::PatchCard4s(patch_card4s_view) => {
            show_patch_card4s(ui, font_families, game_lang, patch_card4s_view.as_ref(), assets, state)
        }
        tango_dataview::save::PatchCardsView::PatchCard56s(patch_card56s_view) => {
            show_patch_card56s(ui, font_families, game_lang, patch_card56s_view.as_ref(), assets, state)
        }
    }
}

pub fn show(
    ui: &mut egui::Ui,
    config: &config::Config,
    shared_root_state: &mut SharedRootState,
    game_lang: &unic_langid::LanguageIdentifier,
    patch_cards_view: &tango_dataview::save::PatchCardsView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    state: &mut State,
) {
    let lang = &config.language;
    let font_families = &shared_root_state.font_families;
    let clipboard = &mut shared_root_state.clipboard;

    ui.horizontal(|ui| {
        ui.menu_button(
            format!("📋 {}", i18n::LOCALES.lookup(lang, "copy-to-clipboard").unwrap(),),
            |ui| {
                let fluent_args = [(
                    "name",
                    i18n::LOCALES.lookup(lang, "save-tab-patch-cards").unwrap().into(),
                )]
                .into();
                let as_image_text = i18n::LOCALES
                    .lookup_with_args(lang, "copy-to-clipboard.named-as-image", &fluent_args)
                    .unwrap();
                let as_text_text = i18n::LOCALES
                    .lookup_with_args(lang, "copy-to-clipboard.named-as-text", &fluent_args)
                    .unwrap();

                if ui.button(as_image_text).clicked() {
                    ui.close_menu();

                    shared_root_state.offscreen_ui.resize(500, 0);
                    shared_root_state.offscreen_ui.run(|ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(8, 0))
                            .fill(ui.style().visuals.panel_fill)
                            .show(ui, |ui| {
                                show_patch_cards(ui, font_families, game_lang, patch_cards_view, assets, state);
                            });
                    });
                    shared_root_state.offscreen_ui.copy_to_clipboard();
                    shared_root_state.offscreen_ui.sweep();
                }

                if ui.button(as_text_text).clicked() {
                    ui.close_menu();
                    let text = patch_cards_string(patch_cards_view, assets);
                    let _ = clipboard.set_text(text);
                }
            },
        );
    });

    ui.style_mut().visuals.clip_rect_margin = 0.0;

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        show_patch_cards(ui, font_families, game_lang, patch_cards_view, assets, state)
    });
}
