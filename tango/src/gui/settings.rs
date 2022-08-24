use fluent_templates::Loader;

use crate::{config, gui, i18n, input};

#[derive(PartialEq, Eq)]
pub enum State {
    General,
    Input,
    Graphics,
    Audio,
    Netplay,
}

pub struct Settings {
    font_families: gui::FontFamilies,
}

impl Settings {
    pub fn new(font_families: gui::FontFamilies) -> Self {
        Self { font_families }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        show_settings: &mut Option<State>,
        config: &mut config::Config,
        steal_input: &mut Option<gui::StealInputState>,
    ) {
        let mut show_settings_bool = show_settings.is_some();
        egui::Window::new(format!(
            "⚙️ {}",
            i18n::LOCALES.lookup(&config.language, "settings").unwrap()
        ))
        .open(&mut show_settings_bool)
        .id(egui::Id::new("settings-window"))
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        show_settings.as_mut().unwrap(),
                        State::General,
                        i18n::LOCALES
                            .lookup(&config.language, "settings.general")
                            .unwrap(),
                    );
                    ui.selectable_value(
                        show_settings.as_mut().unwrap(),
                        State::Input,
                        i18n::LOCALES
                            .lookup(&config.language, "settings.input")
                            .unwrap(),
                    );
                    ui.selectable_value(
                        show_settings.as_mut().unwrap(),
                        State::Graphics,
                        i18n::LOCALES
                            .lookup(&config.language, "settings.graphics")
                            .unwrap(),
                    );
                    ui.selectable_value(
                        show_settings.as_mut().unwrap(),
                        State::Audio,
                        i18n::LOCALES
                            .lookup(&config.language, "settings.audio")
                            .unwrap(),
                    );
                    ui.selectable_value(
                        show_settings.as_mut().unwrap(),
                        State::Netplay,
                        i18n::LOCALES
                            .lookup(&config.language, "settings.netplay")
                            .unwrap(),
                    );
                });

                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            match show_settings.as_ref().unwrap() {
                                State::General => self.draw_settings_general_tab(ui, config),
                                State::Input => self.draw_settings_input_tab(
                                    ui,
                                    &config.language,
                                    &mut config.input_mapping,
                                    steal_input,
                                ),
                                State::Graphics => self.draw_settings_graphics_tab(ui, config),
                                State::Audio => self.draw_settings_audio_tab(ui, config),
                                State::Netplay => self.draw_settings_netplay_tab(ui, config),
                            };
                        });
                    });
            });
        });
        if !show_settings_bool {
            *show_settings = None;
        }
    }

    fn draw_settings_general_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
        egui::Grid::new("settings-window-general-grid")
            .num_columns(2)
            .show(ui, |ui| {
                {
                    let mut nickname = config.nickname.clone().unwrap_or_else(|| "".to_string());
                    ui.label(
                        i18n::LOCALES
                            .lookup(&config.language, "settings-nickname")
                            .unwrap(),
                    );
                    ui.add(egui::TextEdit::singleline(&mut nickname).desired_width(100.0));
                    config.nickname = Some(nickname);
                    ui.end_row();
                }

                {
                    ui.label(
                        i18n::LOCALES
                            .lookup(&config.language, "settings-theme")
                            .unwrap(),
                    );

                    let system_label = i18n::LOCALES
                        .lookup(&config.language, "settings-theme.system")
                        .unwrap();
                    let light_label = i18n::LOCALES
                        .lookup(&config.language, "settings-theme.light")
                        .unwrap();
                    let dark_label = i18n::LOCALES
                        .lookup(&config.language, "settings-theme.dark")
                        .unwrap();

                    egui::ComboBox::from_id_source("settings-window-general-theme")
                        .selected_text(match config.theme {
                            config::Theme::System => &system_label,
                            config::Theme::Light => &light_label,
                            config::Theme::Dark => &dark_label,
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut config.theme,
                                config::Theme::System,
                                &system_label,
                            );
                            ui.selectable_value(
                                &mut config.theme,
                                config::Theme::Light,
                                &light_label,
                            );
                            ui.selectable_value(
                                &mut config.theme,
                                config::Theme::Dark,
                                &dark_label,
                            );
                        });
                    ui.end_row();
                }

                {
                    ui.label(
                        i18n::LOCALES
                            .lookup(&config.language, "settings-language")
                            .unwrap(),
                    );

                    let en_label =
                        egui::RichText::new("English").family(self.font_families.latn.clone());
                    let ja_label =
                        egui::RichText::new("日本語").family(self.font_families.jpan.clone());
                    let zh_hans_label =
                        egui::RichText::new("简体中文").family(self.font_families.hans.clone());
                    let zh_hant_label =
                        egui::RichText::new("繁體中文").family(self.font_families.hant.clone());

                    egui::ComboBox::from_id_source("settings-window-general-language")
                        .selected_text(match &config.language {
                            lang if lang.matches(&unic_langid::langid!("en"), false, true) => {
                                en_label.clone()
                            }
                            lang if lang.matches(&unic_langid::langid!("ja"), false, true) => {
                                ja_label.clone()
                            }
                            lang if lang.matches(&unic_langid::langid!("zh-Hans"), false, true) => {
                                zh_hans_label.clone()
                            }
                            lang if lang.matches(&unic_langid::langid!("zh-Hant"), false, true) => {
                                zh_hant_label.clone()
                            }
                            _ => egui::RichText::new(""),
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut config.language,
                                unic_langid::langid!("en"),
                                en_label.clone(),
                            );
                            ui.selectable_value(
                                &mut config.language,
                                unic_langid::langid!("ja"),
                                ja_label.clone(),
                            );
                            ui.selectable_value(
                                &mut config.language,
                                unic_langid::langid!("zh-Hans"),
                                zh_hans_label.clone(),
                            );
                            ui.selectable_value(
                                &mut config.language,
                                unic_langid::langid!("zh-Hant"),
                                zh_hant_label.clone(),
                            );
                        });
                    ui.end_row();
                }

                {
                    ui.label(
                        i18n::LOCALES
                            .lookup(&config.language, "settings-debug-overlay")
                            .unwrap(),
                    );
                    ui.checkbox(&mut config.show_debug_overlay, "");
                    ui.end_row();
                }
            });
    }

    fn draw_settings_input_tab(
        &mut self,
        ui: &mut egui::Ui,
        lang: &unic_langid::LanguageIdentifier,
        input_mapping: &mut input::Mapping,
        steal_input: &mut Option<gui::StealInputState>,
    ) {
        egui::Grid::new("settings-window-input-mapping-grid")
            .num_columns(2)
            .show(ui, |ui| {
                let mut add_row = |label_text_id,
                                   get_mapping: fn(
                    &mut input::Mapping,
                )
                    -> &mut Vec<input::PhysicalInput>| {
                    ui.label(i18n::LOCALES.lookup(lang, label_text_id).unwrap());
                    ui.horizontal_wrapped(|ui| {
                        let mapping = get_mapping(input_mapping);
                        for (i, c) in mapping.clone().iter().enumerate() {
                            ui.group(|ui| {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.label(egui::RichText::new(match c {
                                            input::PhysicalInput::Key(_) => "⌨️",
                                            input::PhysicalInput::Button(_)
                                            | input::PhysicalInput::Axis { .. } => "🎮",
                                        }));
                                        ui.label(match c {
                                            input::PhysicalInput::Key(key) => {
                                                let raw = serde_plain::to_string(key).unwrap();
                                                i18n::LOCALES
                                                    .lookup(
                                                        lang,
                                                        &format!("physical-input-keys.{}", raw),
                                                    )
                                                    .unwrap_or(raw)
                                            }
                                            input::PhysicalInput::Button(button) => {
                                                let raw = button.string();
                                                i18n::LOCALES
                                                    .lookup(
                                                        lang,
                                                        &format!("physical-input-buttons.{}", raw),
                                                    )
                                                    .unwrap_or(raw)
                                            }
                                            input::PhysicalInput::Axis { axis, direction } => {
                                                let raw = format!(
                                                    "{}{}",
                                                    axis.string(),
                                                    match direction {
                                                        input::AxisDirection::Positive => "plus",
                                                        input::AxisDirection::Negative => "minus",
                                                    }
                                                );
                                                i18n::LOCALES
                                                    .lookup(
                                                        lang,
                                                        &format!("physical-input-axes.{}", raw),
                                                    )
                                                    .unwrap_or(raw)
                                            }
                                        });
                                        if ui.add(egui::Button::new("×").small()).clicked() {
                                            mapping.remove(i);
                                        }
                                    },
                                );
                            });
                        }
                        if ui.add(egui::Button::new("➕")).clicked() {
                            *steal_input = Some(gui::StealInputState {
                                callback: {
                                    let get_mapping = get_mapping.clone();
                                    Box::new(move |phy, input_mapping| {
                                        let mapping = get_mapping(input_mapping);
                                        mapping.push(phy);
                                        mapping.sort_by_key(|c| match c {
                                            input::PhysicalInput::Key(key) => (0, *key as usize, 0),
                                            input::PhysicalInput::Button(button) => {
                                                (1, *button as usize, 0)
                                            }
                                            input::PhysicalInput::Axis { axis, direction } => {
                                                (2, *axis as usize, *direction as usize)
                                            }
                                        });
                                        mapping.dedup();
                                    })
                                },
                                userdata: Box::new(label_text_id),
                            });
                        }
                    });
                    ui.end_row();
                };

                add_row("input-button.left", |input_mapping| &mut input_mapping.left);
                add_row("input-button.right", |input_mapping| {
                    &mut input_mapping.right
                });
                add_row("input-button.up", |input_mapping| &mut input_mapping.up);
                add_row("input-button.down", |input_mapping| &mut input_mapping.down);
                add_row("input-button.a", |input_mapping| &mut input_mapping.a);
                add_row("input-button.b", |input_mapping| &mut input_mapping.b);
                add_row("input-button.l", |input_mapping| &mut input_mapping.l);
                add_row("input-button.r", |input_mapping| &mut input_mapping.r);
                add_row("input-button.start", |input_mapping| {
                    &mut input_mapping.start
                });
                add_row("input-button.select", |input_mapping| {
                    &mut input_mapping.select
                });
            });
    }

    fn draw_settings_graphics_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
        egui::Grid::new("settings-window-graphics-grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-max-scale")
                        .unwrap(),
                );
                ui.add(
                    egui::DragValue::new(&mut config.max_scale)
                        .custom_formatter(|n, _| {
                            if n > 0.0 {
                                format!("{}", n)
                            } else {
                                i18n::LOCALES
                                    .lookup(&config.language, "settings-max-scale.unset")
                                    .unwrap()
                            }
                        })
                        .speed(1)
                        .clamp_range(0..=10),
                );
                ui.end_row();

                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-ui-scale")
                        .unwrap(),
                );
                ui.add(
                    egui::DragValue::new(&mut config.ui_scale_percent)
                        .speed(10)
                        .suffix("%")
                        .clamp_range(50..=400),
                );
                ui.end_row();

                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-video-filter")
                        .unwrap(),
                );

                let null_label = i18n::LOCALES
                    .lookup(&config.language, "settings-video-filter.null")
                    .unwrap();
                let hq2x_label = i18n::LOCALES
                    .lookup(&config.language, "settings-video-filter.hq2x")
                    .unwrap();
                let hq3x_label = i18n::LOCALES
                    .lookup(&config.language, "settings-video-filter.hq3x")
                    .unwrap();
                let hq4x_label = i18n::LOCALES
                    .lookup(&config.language, "settings-video-filter.hq4x")
                    .unwrap();
                let mmpx_label = i18n::LOCALES
                    .lookup(&config.language, "settings-video-filter.mmpx")
                    .unwrap();

                egui::ComboBox::from_id_source("settings-window-general-video-filter")
                    .selected_text(match config.video_filter.as_str() {
                        "" => &null_label,
                        "hq2x" => &hq2x_label,
                        "hq3x" => &hq3x_label,
                        "hq4x" => &hq4x_label,
                        "mmpx" => &mmpx_label,
                        _ => "",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.video_filter, "".to_string(), &null_label);
                        ui.selectable_value(
                            &mut config.video_filter,
                            "hq2x".to_string(),
                            &hq2x_label,
                        );
                        ui.selectable_value(
                            &mut config.video_filter,
                            "hq3x".to_string(),
                            &hq3x_label,
                        );
                        ui.selectable_value(
                            &mut config.video_filter,
                            "hq4x".to_string(),
                            &hq4x_label,
                        );
                        ui.selectable_value(
                            &mut config.video_filter,
                            "mmpx".to_string(),
                            &mmpx_label,
                        );
                    });
                ui.end_row();
            });
    }

    fn draw_settings_audio_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
        egui::Grid::new("settings-window-audio-grid")
            .num_columns(2)
            .show(ui, |ui| {});
    }

    fn draw_settings_netplay_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
        egui::Grid::new("settings-window-netplay-grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-max-queue-length")
                        .unwrap(),
                );
                ui.add(egui::DragValue::new(&mut config.max_queue_length).speed(1));
                ui.end_row();

                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-signaling-endpoint")
                        .unwrap(),
                );
                let signaling_endpoint_is_empty = config.signaling_endpoint.is_empty();
                ui.add(
                    egui::TextEdit::singleline(&mut config.signaling_endpoint)
                        .desired_width(200.0)
                        .hint_text(if signaling_endpoint_is_empty {
                            config::DEFAULT_SIGNALING_ENDPOINT
                        } else {
                            ""
                        }),
                );
                ui.end_row();

                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-iceconfig-endpoint")
                        .unwrap(),
                );
                let iceconfig_endpoint_is_empty = config.iceconfig_endpoint.is_empty();
                ui.add(
                    egui::TextEdit::singleline(&mut config.iceconfig_endpoint)
                        .desired_width(200.0)
                        .hint_text(if iceconfig_endpoint_is_empty {
                            config::DEFAULT_ICECONFIG_ENDPOINT
                        } else {
                            ""
                        }),
                );
                ui.end_row();

                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-replaycollector-endpoint")
                        .unwrap(),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut config.replaycollector_endpoint)
                        .desired_width(200.0),
                );
                ui.end_row();
            });
    }
}
