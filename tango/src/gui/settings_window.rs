use fluent_templates::Loader;

use crate::{config, gui, i18n, input};

#[derive(PartialEq, Eq)]
pub enum State {
    General,
    Input,
    Graphics,
    Audio,
    Netplay,
    About,
}

impl State {
    pub fn new() -> Self {
        Self::General
    }
}

pub struct SettingsWindow {
    font_families: gui::FontFamilies,
    emblem: egui_extras::RetainedImage,
}

impl SettingsWindow {
    pub fn new(font_families: gui::FontFamilies) -> Self {
        Self {
            font_families,
            emblem: egui_extras::RetainedImage::from_image_bytes(
                "emblem",
                include_bytes!("../emblem.png"),
            )
            .unwrap(),
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        show_settings: &mut Option<State>,
        config: &mut config::Config,
        steal_input: &mut Option<gui::steal_input_window::State>,
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
                    ui.selectable_value(
                        show_settings.as_mut().unwrap(),
                        State::About,
                        i18n::LOCALES
                            .lookup(&config.language, "settings.about")
                            .unwrap(),
                    );
                });

                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            match show_settings.as_ref().unwrap() {
                                State::General => self.show_general_tab(ui, config),
                                State::Input => self.show_input_tab(
                                    ui,
                                    &config.language,
                                    &mut config.input_mapping,
                                    steal_input,
                                ),
                                State::Graphics => self.show_graphics_tab(ui, config),
                                State::Audio => self.show_audio_tab(ui, config),
                                State::Netplay => self.show_netplay_tab(ui, config),
                                State::About => self.show_about_tab(ui),
                            };
                        });
                    });
            });
        });
        if !show_settings_bool {
            *show_settings = None;
        }
    }

    fn show_general_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
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

    fn show_input_tab(
        &mut self,
        ui: &mut egui::Ui,
        lang: &unic_langid::LanguageIdentifier,
        input_mapping: &mut input::Mapping,
        steal_input: &mut Option<gui::steal_input_window::State>,
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
                            *steal_input = Some(gui::steal_input_window::State::new(
                                {
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
                                Box::new(label_text_id),
                            ));
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

    fn show_graphics_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
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

    fn show_audio_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
        egui::Grid::new("settings-window-audio-grid")
            .num_columns(2)
            .show(ui, |ui| {});
    }

    fn show_netplay_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
        egui::Grid::new("settings-window-netplay-grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-input-delay")
                        .unwrap(),
                );
                ui.add(egui::Slider::new(&mut config.input_delay, 2..=10));
                ui.end_row();

                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-max-queue-length")
                        .unwrap(),
                );
                ui.add(egui::DragValue::new(&mut config.max_queue_length).speed(1));
                ui.end_row();

                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-matchmaking-endpoint")
                        .unwrap(),
                );
                let matchmaking_endpoint_is_empty = config.matchmaking_endpoint.is_empty();
                ui.add(
                    egui::TextEdit::singleline(&mut config.matchmaking_endpoint)
                        .desired_width(200.0)
                        .hint_text(if matchmaking_endpoint_is_empty {
                            config::DEFAULT_MATCHMAKING_ENDPOINT
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

    pub fn show_about_tab(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.heading(format!(
                    "Tango v{}-{}",
                    env!("CARGO_PKG_VERSION"),
                    git_version::git_version!(),
                ));

                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    self.emblem.show_scaled(ui, 0.5);
                });
                ui.add_space(8.0);

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.hyperlink_to("Tango", "https://tangobattle.com");
                    ui.label(" would not be a reality without the work of the many people who have helped make this possible.",);
                });

                ui.heading("Development");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Emulation: ");
                            ui.hyperlink_to("endrift", "https://twitter.com/endrift");
                            ui.label(" (mGBA)");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Reverse engineering: ");

                            ui.hyperlink_to("pnw_ssbmars", "https://twitter.com/pnw_ssbmars");
                            ui.label(" (BN3)");

                            ui.label(", ");

                            ui.hyperlink_to("XKirby", "https://github.com/XKirby");
                            ui.label(" (BN3)");

                            ui.label(", ");

                            ui.hyperlink_to("luckytyphlosion", "https://github.com/luckytyphlosion");
                            ui.label(" (BN6)");

                            ui.label(", ");

                            ui.hyperlink_to("LanHikari22", "https://github.com/LanHikari22");
                            ui.label(" (BN6)");

                            ui.label(", ");

                            ui.hyperlink_to("GreigaMaster", "https://twitter.com/GreigaMaster");
                            ui.label(" (BN)");

                            ui.label(", ");

                            ui.hyperlink_to("Prof. 9", "https://twitter.com/Prof9");
                            ui.label(" (BN)");

                            ui.label(", ");

                            ui.hyperlink_to("National Security Agency", "https://www.nsa.gov");
                            ui.label(" (Ghidra)");

                            ui.label(", ");


                            ui.hyperlink_to("aldelaro5", "https://twitter.com/aldelaro5");
                            ui.label(" (Ghidra)");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Porting: ");

                            ui.hyperlink_to("ubergeek77", "https://github.com/ubergeek77");
                            ui.label(" (Linux)");

                            ui.label(", ");

                            ui.hyperlink_to("Akatsuki", "https://github.com/Akatsuki");
                            ui.label(" (macOS)");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Game support: ");

                            ui.hyperlink_to("weenie", "https://github.com/bigfarts");
                            ui.label(" (BN)");

                            ui.label(", ");

                            ui.hyperlink_to("GreigaMaster", "https://twitter.com/GreigaMaster");
                            ui.label(" (EXE4.5)");
                        });
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(" • ");
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label("Odds and ends: ");

                            ui.hyperlink_to("sailormoon", "https://github.com/sailormoon");

                            ui.label(", ");

                            ui.hyperlink_to("Shiz", "https://twitter.com/dev_console");

                            ui.label(", ");

                            ui.hyperlink_to("Karate_Bugman", "https://twitter.com/Karate_Bugman");
                        });
                    });
                });

                ui.heading("Translation");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Japanese: ");

                        ui.hyperlink_to("weenie", "https://github.com/bigfarts");

                        ui.label(", ");

                        ui.hyperlink_to("Nonstopmop", "https://twitter.com/seventhfonist42");

                        ui.label(", ");

                        ui.hyperlink_to("dhenva", "https://twitch.tv/dhenva");
                    });
                });

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Simplified Chinese: ");

                        ui.hyperlink_to("weenie", "https://github.com/bigfarts");

                        ui.label(", ");

                        ui.hyperlink_to("Hikari Calyx", "https://twitter.com/Hikari_Calyx");
                    });
                });

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Traditional Chinese: ");

                        ui.hyperlink_to("weenie", "https://github.com/bigfarts");

                        ui.label(", ");

                        ui.hyperlink_to("Hikari Calyx", "https://twitter.com/Hikari_Calyx");
                    });
                });

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Spanish: ");

                        ui.hyperlink_to("Karate_Bugman", "https://twitter.com/Karate_Bugman");
                    });
                });

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Brazilian Portuguese: ");

                        ui.hyperlink_to("Darkgaia", "https://discord.gg/hPrFVaaRrU");

                        ui.label(", ");

                        ui.hyperlink_to("mushiguchi", "https://twitter.com/mushiguchi");
                    });
                });

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("French: ");

                        ui.hyperlink_to("Sheriel Phoenix", "https://twitter.com/Sheriel_Phoenix");

                        ui.label(", ");

                        ui.hyperlink_to("Justplay", "https://twitter.com/justplayfly");
                    });
                });

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("German: ");

                        ui.hyperlink_to("KenDeep", "https://twitch.tv/kendeep_fgc");
                    });
                });

                ui.heading("Art");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Logo: ");

                        ui.hyperlink_to("saladdammit", "https://twitter.com/saladdammit");
                    });
                });


                ui.heading("Special thanks");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("Playtesting: ");

                        ui.hyperlink_to("N1GP", "https://n1gp.net");
                    });
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(" • ");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label("#1 fan: ");

                        ui.hyperlink_to("playerzero", "https://twitter.com/Playerzero_exe");
                    });
                });

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("And, of course, a huge thank you to ");
                    ui.hyperlink_to("CAPCOM", "https://www.capcom.com");
                    ui.label(" for making Mega Man Battle Network!");
                });

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("Tango is licensed under the terms of the ");
                    ui.hyperlink_to("GNU Affero General Public License v3", "https://tldrlegal.com/license/gnu-affero-general-public-license-v3-(agpl-3.0)");
                    ui.label(". That means you’re free to modify the ");
                    ui.hyperlink_to("source code", "https://github.com/tangobattle");
                    ui.label(", as long as you contribute your changes back!");
                });
            });
    }
}
