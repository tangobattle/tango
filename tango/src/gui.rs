use std::str::FromStr;

use fluent_templates::Loader;

use crate::{config, i18n, input, session, stats, video};

const DISCORD_APP_ID: u64 = 974089681333534750;

pub struct State {
    pub config: config::Config,
    pub fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    pub emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    pub session: Option<session::Session>,
    pub steal_input: Option<StealInputState>,
    pub show_menubar: bool,
    pub show_settings: Option<SettingsTab>,
    pub show_debug: bool,
    pub show_about: bool,
    pub drpc: discord_rpc_client::Client,
}

impl State {
    pub fn new(
        config: config::Config,
        fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
        emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    ) -> Self {
        let mut drpc = discord_rpc_client::Client::new(DISCORD_APP_ID);
        drpc.start();

        Self {
            config,
            fps_counter: fps_counter.clone(),
            emu_tps_counter: emu_tps_counter.clone(),
            session: None,
            steal_input: None,
            show_menubar: false,
            show_settings: None,
            show_debug: false,
            show_about: false,
            drpc,
        }
    }
}

#[derive(PartialEq, Eq)]
pub enum SettingsTab {
    General,
    InputMapping,
}

pub struct StealInputState {
    callback: Box<dyn Fn(input::PhysicalInput, &mut input::Mapping)>,
    userdata: Box<dyn std::any::Any>,
}

impl StealInputState {
    pub fn run_callback(&self, phy: input::PhysicalInput, mapping: &mut input::Mapping) {
        (self.callback)(phy, mapping)
    }
}

struct VBuf {
    buf: Vec<u8>,
    texture: egui::TextureHandle,
}

struct Themes {
    light: egui::style::Visuals,
    dark: egui::style::Visuals,
}

struct FontFamilies {
    latn: egui::FontFamily,
    jpan: egui::FontFamily,
    hans: egui::FontFamily,
    hant: egui::FontFamily,
}

pub struct Gui {
    vbuf: Option<VBuf>,
    emblem: egui_extras::RetainedImage,
    font_data: std::collections::BTreeMap<String, egui::FontData>,
    font_families: FontFamilies,
    themes: Themes,
    current_language: Option<unic_langid::LanguageIdentifier>,
}

impl Gui {
    pub fn new(ctx: &egui::Context) -> Self {
        let font_families = FontFamilies {
            latn: egui::FontFamily::Name("Latn".into()),
            jpan: egui::FontFamily::Name("Jpan".into()),
            hans: egui::FontFamily::Name("Hans".into()),
            hant: egui::FontFamily::Name("Hant".into()),
        };

        ctx.set_fonts(egui::FontDefinitions {
            font_data: std::collections::BTreeMap::default(),
            families: std::collections::BTreeMap::from([
                (egui::FontFamily::Proportional, vec![]),
                (egui::FontFamily::Monospace, vec![]),
                (font_families.latn.clone(), vec![]),
                (font_families.jpan.clone(), vec![]),
                (font_families.hans.clone(), vec![]),
                (font_families.hant.clone(), vec![]),
            ]),
        });

        Self {
            vbuf: None,
            emblem: egui_extras::RetainedImage::from_image_bytes(
                "emblem",
                include_bytes!("emblem.png"),
            )
            .unwrap(),
            font_data: std::collections::BTreeMap::from([
                (
                    "NotoSans-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSans-Regular.ttf")),
                ),
                (
                    "NotoSansJP-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSansJP-Regular.otf")),
                ),
                (
                    "NotoSansSC-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSansSC-Regular.otf")),
                ),
                (
                    "NotoSansTC-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSansTC-Regular.otf")),
                ),
                (
                    "NotoSansMono-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSansMono-Regular.ttf")),
                ),
                (
                    "NotoEmoji-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoEmoji-Regular.ttf")),
                ),
            ]),
            font_families,
            themes: Themes {
                light: {
                    let mut visuals = egui::style::Visuals::light();
                    visuals.selection.bg_fill = egui::Color32::from_rgb(0x4c, 0xaf, 0x50);
                    visuals.selection.stroke.color = egui::Color32::BLACK;
                    visuals
                },
                dark: {
                    let mut visuals = egui::style::Visuals::dark();
                    visuals.selection.bg_fill = egui::Color32::from_rgb(0x4c, 0xaf, 0x50);
                    visuals.selection.stroke.color = egui::Color32::WHITE;
                    visuals
                },
            },
            current_language: None,
        }
    }

    fn draw_settings_window(
        &mut self,
        ctx: &egui::Context,
        show_settings: &mut Option<SettingsTab>,
        config: &mut config::Config,
        steal_input: &mut Option<StealInputState>,
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
                        SettingsTab::General,
                        i18n::LOCALES
                            .lookup(&config.language, "settings.general")
                            .unwrap(),
                    );
                    ui.selectable_value(
                        show_settings.as_mut().unwrap(),
                        SettingsTab::InputMapping,
                        i18n::LOCALES
                            .lookup(&config.language, "settings.input-mapping")
                            .unwrap(),
                    );
                });

                ui.separator();

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        match show_settings.as_ref().unwrap() {
                            SettingsTab::General => self.draw_settings_general_tab(ui, config),
                            SettingsTab::InputMapping => self.draw_settings_input_mapping_tab(
                                ui,
                                &config.language,
                                &mut config.input_mapping,
                                steal_input,
                            ),
                        };
                    });
            });
        });
        if !show_settings_bool {
            *show_settings = None;
        }
    }

    fn draw_settings_general_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
        egui::Grid::new("settings-window-general-grid").show(ui, |ui| {
            {
                let mut nickname = config.nickname.clone().unwrap_or_else(|| "".to_string());
                ui.label(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-nickname")
                        .unwrap(),
                );
                ui.text_edit_singleline(&mut nickname);
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
                        ui.selectable_value(&mut config.theme, config::Theme::Light, &light_label);
                        ui.selectable_value(&mut config.theme, config::Theme::Dark, &dark_label);
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
        });
    }

    fn draw_settings_input_mapping_tab(
        &mut self,
        ui: &mut egui::Ui,
        lang: &unic_langid::LanguageIdentifier,
        input_mapping: &mut input::Mapping,
        steal_input: &mut Option<StealInputState>,
    ) {
        egui::Grid::new("settings-window-input-mapping-grid").show(ui, |ui| {
            let mut add_row = |label_text_id,
                               get_mapping: fn(
                &mut input::Mapping,
            ) -> &mut Vec<input::PhysicalInput>| {
                ui.label(i18n::LOCALES.lookup(lang, label_text_id).unwrap());
                ui.horizontal(|ui| {
                    let mapping = get_mapping(input_mapping);
                    for (i, c) in mapping.clone().iter().enumerate() {
                        ui.group(|ui| {
                            ui.label(egui::RichText::new(match c {
                                input::PhysicalInput::Key(_) => "⌨️",
                                input::PhysicalInput::Button(_)
                                | input::PhysicalInput::Axis { .. } => "🎮",
                            }));
                            ui.label(match c {
                                input::PhysicalInput::Key(key) => {
                                    let raw = serde_plain::to_string(key).unwrap();
                                    i18n::LOCALES
                                        .lookup(lang, &format!("physical-input-keys.{}", raw))
                                        .unwrap_or(raw)
                                }
                                input::PhysicalInput::Button(button) => {
                                    let raw = button.string();
                                    i18n::LOCALES
                                        .lookup(lang, &format!("physical-input-buttons.{}", raw))
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
                                        .lookup(lang, &format!("physical-input-axes.{}", raw))
                                        .unwrap_or(raw)
                                }
                            });
                            if ui.add(egui::Button::new("×").small()).clicked() {
                                mapping.remove(i);
                            }
                        });
                    }
                    if ui.add(egui::Button::new("➕")).clicked() {
                        *steal_input = Some(StealInputState {
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

    fn draw_debug_window(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        state: &mut State,
    ) {
        egui::Window::new(format!(
            "🐛 {}",
            i18n::LOCALES
                .lookup(&state.config.language, "debug")
                .unwrap()
        ))
        .id(egui::Id::new("debug-window"))
        .open(&mut state.show_debug)
        .show(ctx, |ui| {
            egui::Grid::new("debug-window-grid").show(ui, |ui| {
                ui.label("FPS");
                ui.label(
                    egui::RichText::new(format!(
                        "{:3.02}",
                        1.0 / state.fps_counter.lock().mean_duration().as_secs_f32()
                    ))
                    .family(egui::FontFamily::Monospace),
                );
                ui.end_row();

                if let Some(session) = &state.session {
                    let tps_adjustment = if let session::Mode::PvP(match_) = session.mode() {
                        handle.block_on(async {
                            if let Some(match_) = &*match_.lock().await {
                                ui.label("Match active");
                                ui.end_row();

                                let round_state = match_.lock_round_state().await;
                                if let Some(round) = round_state.round.as_ref() {
                                    ui.label("Current tick");
                                    ui.label(
                                        egui::RichText::new(format!("{:4}", round.current_tick()))
                                            .family(egui::FontFamily::Monospace),
                                    );
                                    ui.end_row();

                                    ui.label("Local player index");
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{:1}",
                                            round.local_player_index()
                                        ))
                                        .family(egui::FontFamily::Monospace),
                                    );
                                    ui.end_row();

                                    ui.label("Queue length");
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{:2} vs {:2} (delay = {:1})",
                                            round.local_queue_length(),
                                            round.remote_queue_length(),
                                            round.local_delay(),
                                        ))
                                        .family(egui::FontFamily::Monospace),
                                    );
                                    ui.end_row();
                                    round.tps_adjustment()
                                } else {
                                    0.0
                                }
                            } else {
                                0.0
                            }
                        })
                    } else {
                        0.0
                    };

                    ui.label("Emu TPS");
                    ui.label(
                        egui::RichText::new(format!(
                            "{:3.02} ({:+1.02})",
                            1.0 / state.emu_tps_counter.lock().mean_duration().as_secs_f32(),
                            tps_adjustment
                        ))
                        .family(egui::FontFamily::Monospace),
                    );
                    ui.end_row();
                }
            });
        });
    }

    fn draw_about_window(
        &mut self,
        ctx: &egui::Context,
        lang: &unic_langid::LanguageIdentifier,
        show_about: &mut bool,
    ) {
        egui::Window::new(format!(
            "❓ {}",
            i18n::LOCALES.lookup(lang, "about").unwrap()
        ))
        .id(egui::Id::new("about-window"))
        .default_width(320.0)
        .open(show_about)
        .show(ctx, |ui| {
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
        });
    }

    fn draw_emulator(&mut self, ui: &mut egui::Ui, session: &session::Session, video_filter: &str) {
        let video_filter =
            video::filter_by_name(video_filter).unwrap_or(Box::new(video::NullFilter));

        // Apply stupid video scaling filter that only mint wants 🥴
        let (vbuf_width, vbuf_height) = video_filter.output_size((
            mgba::gba::SCREEN_WIDTH as usize,
            mgba::gba::SCREEN_HEIGHT as usize,
        ));

        let make_vbuf = || {
            log::info!("vbuf reallocation: ({}, {})", vbuf_width, vbuf_height);
            VBuf {
                buf: vec![0u8; vbuf_width * vbuf_height * 4],
                texture: ui.ctx().load_texture(
                    "vbuf",
                    egui::ColorImage::new([vbuf_width, vbuf_height], egui::Color32::BLACK),
                    egui::TextureFilter::Nearest,
                ),
            }
        };
        let vbuf = self.vbuf.get_or_insert_with(make_vbuf);
        if vbuf.texture.size() != [vbuf_width, vbuf_height] {
            *vbuf = make_vbuf();
        }

        video_filter.apply(
            &session.lock_vbuf(),
            &mut vbuf.buf,
            (
                mgba::gba::SCREEN_WIDTH as usize,
                mgba::gba::SCREEN_HEIGHT as usize,
            ),
        );

        vbuf.texture.set(
            egui::ColorImage::from_rgba_unmultiplied([vbuf_width, vbuf_height], &vbuf.buf),
            egui::TextureFilter::Nearest,
        );

        let scaling_factor = std::cmp::max_by(
            std::cmp::min_by(
                ui.available_width() / mgba::gba::SCREEN_WIDTH as f32,
                ui.available_height() / mgba::gba::SCREEN_HEIGHT as f32,
                |a, b| a.partial_cmp(b).unwrap(),
            )
            .floor(),
            1.0,
            |a, b| a.partial_cmp(b).unwrap(),
        );
        ui.image(
            &vbuf.texture,
            egui::Vec2::new(
                mgba::gba::SCREEN_WIDTH as f32 * scaling_factor as f32,
                mgba::gba::SCREEN_HEIGHT as f32 * scaling_factor as f32,
            ),
        );
    }

    pub fn draw_session(
        &mut self,
        ctx: &egui::Context,
        input_state: &input::State,
        input_mapping: &input::Mapping,
        session: &session::Session,
        video_filter: &str,
    ) {
        session.set_joyflags(input_mapping.to_mgba_keys(input_state));

        // If we're in single-player mode, allow speedup.
        if let session::Mode::SinglePlayer = session.mode() {
            session.set_fps(
                if input_mapping
                    .speed_up
                    .iter()
                    .any(|c| c.is_active(&input_state))
                {
                    session::EXPECTED_FPS * 3.0
                } else {
                    session::EXPECTED_FPS
                },
            );
        }

        // If we've crashed, log the error and panic.
        if let Some(thread_handle) = session.has_crashed() {
            // HACK: No better way to lock the core.
            let audio_guard = thread_handle.lock_audio();
            panic!(
                "mgba thread crashed!\nlr = {:08x}, pc = {:08x}",
                audio_guard.core().gba().cpu().gpr(14),
                audio_guard.core().gba().cpu().thumb_pc()
            );
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| {
                        self.draw_emulator(ui, session, video_filter);
                    },
                );
            });
    }

    pub fn draw_steal_input(
        &mut self,
        ctx: &egui::Context,
        language: &unic_langid::LanguageIdentifier,
        steal_input: &mut Option<StealInputState>,
    ) {
        let mut steal_input_open = steal_input.is_some();
        if let Some(inner_response) = egui::Window::new("")
            .id(egui::Id::new("input-capture-window"))
            .open(&mut steal_input_open)
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::top_down_justified(egui::Align::Center),
                    |ui| {
                        egui::Frame::none()
                            .inner_margin(egui::style::Margin::symmetric(32.0, 16.0))
                            .show(ui, |ui| {
                                let userdata =
                                    if let Some(StealInputState { userdata, .. }) = &steal_input {
                                        userdata
                                    } else {
                                        unreachable!();
                                    };

                                ui.label(
                                    egui::RichText::new(
                                        i18n::LOCALES
                                            .lookup_with_args(
                                                &language,
                                                "input-mapping.prompt",
                                                &std::collections::HashMap::from([(
                                                    "key",
                                                    i18n::LOCALES
                                                        .lookup(
                                                            &language,
                                                            userdata
                                                                .downcast_ref::<&str>()
                                                                .unwrap(),
                                                        )
                                                        .unwrap()
                                                        .into(),
                                                )]),
                                            )
                                            .unwrap(),
                                    )
                                    .size(32.0),
                                );
                            });
                    },
                );
            })
        {
            ctx.move_to_top(inner_response.response.layer_id);
        }
        if !steal_input_open {
            *steal_input = None;
        }
    }

    pub fn draw_menubar(&mut self, ctx: &egui::Context, state: &mut State) {
        egui::Window::new("")
            .id(egui::Id::new("menubar"))
            .open(&mut state.show_menubar)
            .frame(egui::Frame::window(&ctx.style()).rounding(egui::Rounding::none()))
            .title_bar(false)
            .resizable(false)
            .min_width(ctx.available_rect().width())
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(
                            state.show_settings.is_some(),
                            format!(
                                "⚙️ {}",
                                i18n::LOCALES
                                    .lookup(&state.config.language, "settings")
                                    .unwrap()
                            ),
                        )
                        .clicked()
                    {
                        state.show_settings = if state.show_settings.is_some() {
                            None
                        } else {
                            Some(SettingsTab::General)
                        };
                    }

                    if ui
                        .selectable_label(
                            state.show_debug,
                            format!(
                                "🐛 {}",
                                i18n::LOCALES
                                    .lookup(&state.config.language, "debug")
                                    .unwrap()
                            ),
                        )
                        .clicked()
                    {
                        state.show_debug = !state.show_debug;
                    }

                    if ui
                        .selectable_label(
                            state.show_about,
                            format!(
                                "❓ {}",
                                i18n::LOCALES
                                    .lookup(&state.config.language, "about")
                                    .unwrap()
                            ),
                        )
                        .clicked()
                    {
                        state.show_about = !state.show_about;
                    }
                })
            });
    }

    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        input_state: &input::State,
        state: &mut State,
    ) {
        if self.current_language.as_ref() != Some(&state.config.language) {
            let mut language = state.config.language.clone();
            language.maximize();

            ctx.set_fonts(egui::FontDefinitions {
                font_data: self.font_data.clone(),
                families: std::collections::BTreeMap::from([
                    (
                        egui::FontFamily::Proportional,
                        vec![
                            match language.script {
                                Some(s)
                                    if s == unic_langid::subtags::Script::from_str("Jpan")
                                        .unwrap() =>
                                {
                                    "NotoSansJP-Regular"
                                }
                                Some(s)
                                    if s == unic_langid::subtags::Script::from_str("Hans")
                                        .unwrap() =>
                                {
                                    "NotoSansSC-Regular"
                                }
                                Some(s)
                                    if s == unic_langid::subtags::Script::from_str("Hant")
                                        .unwrap() =>
                                {
                                    "NotoSansTC-Regular"
                                }
                                _ => "NotoSans-Regular",
                            }
                            .to_string(),
                            "NotoSans-Regular".to_string(),
                            "NotoSansJP-Regular".to_string(),
                            "NotoSansSC-Regular".to_string(),
                            "NotoSansTC-Regular".to_string(),
                            "NotoEmoji-Regular".to_string(),
                        ],
                    ),
                    (
                        egui::FontFamily::Monospace,
                        vec![
                            "NotoSansMono-Regular".to_string(),
                            "NotoEmoji-Regular".to_string(),
                        ],
                    ),
                    (
                        self.font_families.jpan.clone(),
                        vec!["NotoSansJP-Regular".to_string()],
                    ),
                    (
                        self.font_families.hans.clone(),
                        vec!["NotoSansSC-Regular".to_string()],
                    ),
                    (
                        self.font_families.hant.clone(),
                        vec!["NotoSansTC-Regular".to_string()],
                    ),
                    (
                        self.font_families.latn.clone(),
                        vec!["NotoSans-Regular".to_string()],
                    ),
                ]),
            });
            self.current_language = Some(state.config.language.clone());
            log::info!(
                "language was changed to {}",
                self.current_language.as_ref().unwrap()
            );
        }

        ctx.set_visuals(match state.config.theme {
            config::Theme::System => match dark_light::detect() {
                dark_light::Mode::Light => self.themes.light.clone(),
                dark_light::Mode::Dark => self.themes.dark.clone(),
            },
            config::Theme::Light => self.themes.light.clone(),
            config::Theme::Dark => self.themes.dark.clone(),
        });

        if input_state.is_key_pressed(glutin::event::VirtualKeyCode::Escape) {
            state.show_menubar = !state.show_menubar;
        }

        if let Some(session) = &state.session {
            self.draw_session(
                ctx,
                input_state,
                &state.config.input_mapping,
                session,
                &state.config.video_filter,
            );
        } else {
            state.show_menubar = true;
        }

        self.draw_menubar(ctx, state);
        self.draw_debug_window(ctx, handle.clone(), state);
        self.draw_settings_window(
            ctx,
            &mut state.show_settings,
            &mut state.config,
            &mut state.steal_input,
        );
        self.draw_about_window(ctx, &state.config.language, &mut state.show_about);
        self.draw_steal_input(ctx, &state.config.language, &mut state.steal_input);
    }
}
