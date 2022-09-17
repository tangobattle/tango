use fluent_templates::Loader;

use crate::{config, game, gui, i18n, input, patch, rom, save, version};

#[derive(PartialEq, Eq)]
enum Tab {
    General,
    Input,
    Graphics,
    Audio,
    Netplay,
    Patches,
    Advanced,
    About,
}

pub struct State {
    tab: Tab,
    emblem: egui_extras::RetainedImage,
}

impl State {
    pub fn new() -> Self {
        Self {
            tab: Tab::General,
            emblem: egui_extras::RetainedImage::from_image_bytes("emblem", include_bytes!("../emblem.png")).unwrap(),
        }
    }
}

pub fn show(
    ctx: &egui::Context,
    state: &mut Option<State>,
    font_families: &gui::FontFamilies,
    config: &mut config::Config,
    roms_scanner: rom::Scanner,
    saves_scanner: save::Scanner,
    patches_scanner: patch::Scanner,
    window: &winit::window::Window,
    steal_input: &mut Option<gui::steal_input_window::State>,
) {
    let mut open = state.is_some();
    egui::Window::new(format!(
        "⚙️ {}",
        i18n::LOCALES.lookup(&config.language, "settings").unwrap()
    ))
    .open(&mut open)
    .id(egui::Id::new("settings-window"))
    .show(ctx, |ui| {
        let state = state.as_mut().unwrap();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut state.tab,
                    Tab::General,
                    i18n::LOCALES.lookup(&config.language, "settings-tab-general").unwrap(),
                );
                ui.selectable_value(
                    &mut state.tab,
                    Tab::Input,
                    i18n::LOCALES.lookup(&config.language, "settings-tab-input").unwrap(),
                );
                ui.selectable_value(
                    &mut state.tab,
                    Tab::Graphics,
                    i18n::LOCALES.lookup(&config.language, "settings-tab-graphics").unwrap(),
                );
                ui.selectable_value(
                    &mut state.tab,
                    Tab::Audio,
                    i18n::LOCALES.lookup(&config.language, "settings-tab-audio").unwrap(),
                );
                ui.selectable_value(
                    &mut state.tab,
                    Tab::Netplay,
                    i18n::LOCALES.lookup(&config.language, "settings-tab-netplay").unwrap(),
                );
                ui.selectable_value(
                    &mut state.tab,
                    Tab::Patches,
                    i18n::LOCALES.lookup(&config.language, "settings-tab-patches").unwrap(),
                );
                ui.selectable_value(
                    &mut state.tab,
                    Tab::Advanced,
                    i18n::LOCALES.lookup(&config.language, "settings-tab-advanced").unwrap(),
                );
                ui.selectable_value(
                    &mut state.tab,
                    Tab::About,
                    i18n::LOCALES.lookup(&config.language, "settings-tab-about").unwrap(),
                );
            });

            ui.separator();

            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    match state.tab {
                        Tab::General => show_general_tab(ui, config, font_families),
                        Tab::Input => show_input_tab(ui, &config.language, &mut config.input_mapping, steal_input),
                        Tab::Graphics => show_graphics_tab(ui, config, window),
                        Tab::Audio => show_audio_tab(ui, config),
                        Tab::Netplay => show_netplay_tab(ui, config),
                        Tab::Patches => show_patches_tab(ui, config),
                        Tab::Advanced => show_advanced_tab(
                            ui,
                            config,
                            roms_scanner.clone(),
                            saves_scanner.clone(),
                            patches_scanner.clone(),
                        ),
                        Tab::About => show_about_tab(ui, &state.emblem),
                    };
                });
            });
        });
    });
    if !open {
        *state = None;
    }
}

fn show_general_tab(ui: &mut egui::Ui, config: &mut config::Config, font_families: &gui::FontFamilies) {
    egui::Grid::new("settings-window-general-grid")
        .num_columns(2)
        .show(ui, |ui| {
            {
                let mut nickname = config.nickname.clone().unwrap_or_else(|| "".to_string());
                ui.strong(i18n::LOCALES.lookup(&config.language, "settings-nickname").unwrap());
                ui.add(egui::TextEdit::singleline(&mut nickname).desired_width(100.0));
                config.nickname = Some(nickname.chars().take(20).collect());
                ui.end_row();
            }

            {
                ui.strong(i18n::LOCALES.lookup(&config.language, "settings-theme").unwrap());

                let system_label = i18n::LOCALES.lookup(&config.language, "settings-theme.system").unwrap();
                let light_label = i18n::LOCALES.lookup(&config.language, "settings-theme.light").unwrap();
                let dark_label = i18n::LOCALES.lookup(&config.language, "settings-theme.dark").unwrap();

                egui::ComboBox::from_id_source("settings-window-general-theme")
                    .selected_text(match config.theme {
                        config::Theme::System => &system_label,
                        config::Theme::Light => &light_label,
                        config::Theme::Dark => &dark_label,
                    })
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.theme, config::Theme::System, &system_label);
                        ui.selectable_value(&mut config.theme, config::Theme::Light, &light_label);
                        ui.selectable_value(&mut config.theme, config::Theme::Dark, &dark_label);
                    });
                ui.end_row();
            }

            {
                ui.strong(i18n::LOCALES.lookup(&config.language, "settings-language").unwrap());
                gui::language_select::show(ui, font_families, &mut config.language);
                ui.end_row();
            }

            {
                ui.strong(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-streamer-mode")
                        .unwrap(),
                );
                ui.checkbox(&mut config.streamer_mode, "").on_hover_text(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-streamer-mode.tooltip")
                        .unwrap(),
                );
                ui.end_row();
            }

            {
                ui.strong(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-show-own-setup")
                        .unwrap(),
                );
                ui.checkbox(&mut config.show_own_setup, "");
                ui.end_row();
            }
        });
}

fn show_input_tab(
    ui: &mut egui::Ui,
    lang: &unic_langid::LanguageIdentifier,
    input_mapping: &mut input::Mapping,
    steal_input: &mut Option<gui::steal_input_window::State>,
) {
    egui::Grid::new("settings-window-input-mapping-grid")
        .num_columns(2)
        .show(ui, |ui| {
            let mut add_row =
                |label_text_id, steal_axes, get_mapping: fn(&mut input::Mapping) -> &mut Vec<input::PhysicalInput>| {
                    ui.strong(i18n::LOCALES.lookup(lang, label_text_id).unwrap());
                    ui.horizontal_wrapped(|ui| {
                        let mapping = get_mapping(input_mapping);
                        for (i, c) in mapping.clone().iter().enumerate() {
                            ui.group(|ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.label(egui::RichText::new(match c {
                                        input::PhysicalInput::Key(_) => "⌨️",
                                        input::PhysicalInput::Button(_) | input::PhysicalInput::Axis { .. } => "🎮",
                                    }));
                                    ui.label(match c {
                                        input::PhysicalInput::Key(key) => {
                                            let raw = serde_plain::to_string(key).unwrap();
                                            i18n::LOCALES
                                                .lookup(lang, &format!("physical-input-key-{}", raw))
                                                .unwrap_or(raw)
                                        }
                                        input::PhysicalInput::Button(button) => {
                                            let raw = button.string();
                                            i18n::LOCALES
                                                .lookup(lang, &format!("physical-input-button-{}", raw))
                                                .unwrap_or(raw)
                                        }
                                        input::PhysicalInput::Axis { axis, direction } => {
                                            let raw = format!(
                                                "{}-{}",
                                                axis.string(),
                                                match direction {
                                                    input::AxisDirection::Positive => "plus",
                                                    input::AxisDirection::Negative => "minus",
                                                }
                                            );
                                            i18n::LOCALES
                                                .lookup(lang, &format!("physical-input-axis-motion-{}", raw))
                                                .unwrap_or(raw)
                                        }
                                    });
                                    if ui.add(egui::Button::new("×").small()).clicked() {
                                        mapping.remove(i);
                                    }
                                });
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
                                            input::PhysicalInput::Button(button) => (1, *button as usize, 0),
                                            input::PhysicalInput::Axis { axis, direction } => {
                                                (2, *axis as usize, *direction as usize)
                                            }
                                        });
                                        mapping.dedup();
                                    })
                                },
                                steal_axes,
                                Box::new(label_text_id),
                            ));
                        }
                    });
                    ui.end_row();
                };

            add_row("input-button-left", true, |input_mapping| &mut input_mapping.left);
            add_row("input-button-right", true, |input_mapping| &mut input_mapping.right);
            add_row("input-button-up", true, |input_mapping| &mut input_mapping.up);
            add_row("input-button-down", true, |input_mapping| &mut input_mapping.down);
            add_row("input-button-a", false, |input_mapping| &mut input_mapping.a);
            add_row("input-button-b", false, |input_mapping| &mut input_mapping.b);
            add_row("input-button-l", false, |input_mapping| &mut input_mapping.l);
            add_row("input-button-r", false, |input_mapping| &mut input_mapping.r);
            add_row("input-button-start", false, |input_mapping| &mut input_mapping.start);
            add_row("input-button-select", false, |input_mapping| &mut input_mapping.select);
            add_row("input-button-speed-up", false, |input_mapping| {
                &mut input_mapping.speed_up
            });
            add_row("input-button-menu", false, |input_mapping| &mut input_mapping.menu);
        });
}

fn show_graphics_tab(ui: &mut egui::Ui, config: &mut config::Config, window: &winit::window::Window) {
    egui::Grid::new("settings-window-graphics-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.strong(i18n::LOCALES.lookup(&config.language, "settings-max-scale").unwrap());
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

            ui.strong(
                i18n::LOCALES
                    .lookup(&config.language, "settings-integer-scaling")
                    .unwrap(),
            );
            ui.checkbox(&mut config.integer_scaling, "");
            ui.end_row();

            ui.strong(i18n::LOCALES.lookup(&config.language, "settings-ui-scale").unwrap());
            egui::ComboBox::from_id_source("settings-ui-scale")
                .selected_text(format!("{}%", config.ui_scale_percent))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut config.ui_scale_percent, 50, "50%");
                    ui.selectable_value(&mut config.ui_scale_percent, 75, "75%");
                    ui.selectable_value(&mut config.ui_scale_percent, 100, "100%");
                    ui.selectable_value(&mut config.ui_scale_percent, 125, "125%");
                    ui.selectable_value(&mut config.ui_scale_percent, 150, "150%");
                    ui.selectable_value(&mut config.ui_scale_percent, 175, "175%");
                    ui.selectable_value(&mut config.ui_scale_percent, 200, "200%");
                });
            ui.end_row();

            ui.strong(i18n::LOCALES.lookup(&config.language, "settings-full-screen").unwrap());
            ui.add(egui::Checkbox::new(&mut config.full_screen, ""));
            if config.full_screen && window.fullscreen().is_none() {
                window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
            } else if !config.full_screen && window.fullscreen().is_some() {
                window.set_fullscreen(None);
            }

            ui.end_row();

            {
                ui.strong(i18n::LOCALES.lookup(&config.language, "settings-video-filter").unwrap());

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
                    .width(200.0)
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
                        ui.selectable_value(&mut config.video_filter, "hq2x".to_string(), &hq2x_label);
                        ui.selectable_value(&mut config.video_filter, "hq3x".to_string(), &hq3x_label);
                        ui.selectable_value(&mut config.video_filter, "hq4x".to_string(), &hq4x_label);
                        ui.selectable_value(&mut config.video_filter, "mmpx".to_string(), &mmpx_label);
                    });
                ui.end_row();
            }

            {
                ui.strong(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-graphics-backend")
                        .unwrap(),
                );

                #[cfg(feature = "glutin")]
                let glutin_label = i18n::LOCALES
                    .lookup(&config.language, "settings-graphics-backend.glutin")
                    .unwrap();
                #[cfg(feature = "wgpu")]
                let wgpu_label = i18n::LOCALES
                    .lookup(&config.language, "settings-graphics-backend.wgpu")
                    .unwrap();

                egui::ComboBox::from_id_source("settings-window-general-graphics-backend")
                    .width(200.0)
                    .selected_text(match config.graphics_backend {
                        #[cfg(feature = "glutin")]
                        config::GraphicsBackend::Glutin => &glutin_label,
                        #[cfg(feature = "wgpu")]
                        config::GraphicsBackend::Wgpu => &wgpu_label,
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut config.graphics_backend,
                            config::GraphicsBackend::Glutin,
                            &glutin_label,
                        );
                        ui.selectable_value(&mut config.graphics_backend, config::GraphicsBackend::Wgpu, &wgpu_label);
                    });
                ui.end_row();
            }
        });
}

fn show_audio_tab(ui: &mut egui::Ui, config: &mut config::Config) {
    egui::Grid::new("settings-window-audio-grid")
        .num_columns(2)
        .show(ui, |ui| {
            let mut volume = (config.volume as f32 * 100.0 / 256.0).round() as i32;
            ui.strong(i18n::LOCALES.lookup(&config.language, "settings-volume").unwrap());
            ui.add(egui::Slider::new(&mut volume, 0..=100).suffix("%"));
            config.volume = volume * 0x100 / 100;
            ui.end_row();

            {
                ui.strong(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-audio-backend")
                        .unwrap(),
                );

                #[cfg(feature = "sdl2-audio")]
                let sdl2_label = i18n::LOCALES
                    .lookup(&config.language, "settings-audio-backend.sdl2")
                    .unwrap();
                #[cfg(feature = "cpal")]
                let cpal_label = i18n::LOCALES
                    .lookup(&config.language, "settings-audio-backend.cpal")
                    .unwrap();

                egui::ComboBox::from_id_source("settings-window-general-audio-backend")
                    .width(200.0)
                    .selected_text(match config.audio_backend {
                        #[cfg(feature = "sdl2-audio")]
                        config::AudioBackend::Sdl2 => &sdl2_label,
                        #[cfg(feature = "cpal")]
                        config::AudioBackend::Cpal => &cpal_label,
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.audio_backend, config::AudioBackend::Sdl2, &sdl2_label);
                        ui.selectable_value(&mut config.audio_backend, config::AudioBackend::Cpal, &cpal_label);
                    });
                ui.end_row();
            }
        });
}

fn show_netplay_tab(ui: &mut egui::Ui, config: &mut config::Config) {
    egui::Grid::new("settings-window-netplay-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.strong(i18n::LOCALES.lookup(&config.language, "settings-input-delay").unwrap());
            ui.add(egui::Slider::new(&mut config.input_delay, 2..=10));
            ui.end_row();

            ui.strong(
                i18n::LOCALES
                    .lookup(&config.language, "settings-max-queue-length")
                    .unwrap(),
            );
            ui.add(egui::DragValue::new(&mut config.max_queue_length).speed(1));
            ui.end_row();

            ui.strong(
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

            ui.strong(
                i18n::LOCALES
                    .lookup(&config.language, "settings-replaycollector-endpoint")
                    .unwrap(),
            );
            ui.add(egui::TextEdit::singleline(&mut config.replaycollector_endpoint).desired_width(200.0));
            ui.end_row();
        });
}

fn show_patches_tab(ui: &mut egui::Ui, config: &mut config::Config) {
    egui::Grid::new("settings-window-patches-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.strong(i18n::LOCALES.lookup(&config.language, "settings-patch-repo").unwrap());
            let patch_repo = config.patch_repo.is_empty();
            ui.add(
                egui::TextEdit::singleline(&mut config.patch_repo)
                    .desired_width(200.0)
                    .hint_text(if patch_repo { config::DEFAULT_PATCH_REPO } else { "" }),
            );
            ui.end_row();

            ui.strong(
                i18n::LOCALES
                    .lookup(&config.language, "settings-enable-patch-autoupdate")
                    .unwrap(),
            );
            ui.checkbox(&mut config.enable_patch_autoupdate, "");
            ui.end_row();
        });
}

fn show_advanced_tab(
    ui: &mut egui::Ui,
    config: &mut config::Config,
    roms_scanner: rom::Scanner,
    saves_scanner: save::Scanner,
    patches_scanner: patch::Scanner,
) {
    egui::Grid::new("settings-window-general-grid")
        .num_columns(2)
        .show(ui, |ui| {
            {
                ui.strong(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-enable-updater")
                        .unwrap(),
                );
                ui.checkbox(&mut config.enable_updater, "");
                ui.end_row();
            }

            {
                ui.strong(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-allow-prerelease-upgrades")
                        .unwrap(),
                );
                ui.checkbox(&mut config.allow_prerelease_upgrades, "");
                ui.end_row();
            }

            {
                ui.strong(i18n::LOCALES.lookup(&config.language, "settings-data-path").unwrap());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut format!("{}", config.data_path.display())).interactive(false),
                    );

                    if ui
                        .button(
                            i18n::LOCALES
                                .lookup(&config.language, "settings-data-path.open")
                                .unwrap(),
                        )
                        .clicked()
                    {
                        let _ = open::that(&config.data_path);
                    }

                    if ui
                        .button(
                            i18n::LOCALES
                                .lookup(&config.language, "settings-data-path.change")
                                .unwrap(),
                        )
                        .clicked()
                    {
                        if let Some(data_path) = rfd::FileDialog::new().set_directory(&config.data_path).pick_folder() {
                            config.data_path = data_path;
                            let _ = config.ensure_dirs();
                            tokio::task::spawn_blocking({
                                let egui_ctx = ui.ctx().clone();
                                let roms_scanner = roms_scanner.clone();
                                let saves_scanner = saves_scanner.clone();
                                let patches_scanner = patches_scanner.clone();
                                let roms_path = config.roms_path();
                                let saves_path = config.saves_path();
                                let patches_path = config.patches_path();
                                move || {
                                    roms_scanner.rescan(move || Some(game::scan_roms(&roms_path)));
                                    saves_scanner.rescan(move || Some(save::scan_saves(&saves_path)));
                                    patches_scanner
                                        .rescan(move || Some(patch::scan(&patches_path).unwrap_or_default()));
                                    egui_ctx.request_repaint();
                                }
                            });
                        }
                    }
                });
                ui.end_row();
            }

            {
                ui.strong(
                    i18n::LOCALES
                        .lookup(&config.language, "settings-debug-overlay")
                        .unwrap(),
                );
                ui.checkbox(&mut config.show_debug_overlay, "");
                ui.end_row();
            }
        });
}

fn show_about_tab(ui: &mut egui::Ui, emblem: &egui_extras::RetainedImage) {
    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        ui.heading(format!("Tango {}", version::VERSION));

        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            emblem.show_scaled(ui, 0.5);
        });
        ui.add_space(8.0);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.hyperlink_to("Tango", "https://tango.n1gp.net");
            ui.label(" would not be a reality without the work of the many people who have helped make this possible.");
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
                    ui.label(" (BN1-6)");

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

                    ui.hyperlink_to("zachristmas", "https://github.com/zachristmas");

                    ui.label(", ");

                    ui.hyperlink_to("Akatsuki", "https://github.com/Akatsuki");

                    ui.label(", ");

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
                ui.label("Mandarin (mainland China): ");

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
                ui.label("Mandarin (Taiwan): ");

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
                ui.label("Spanish (Latin America): ");

                ui.hyperlink_to("Karate_Bugman", "https://twitter.com/Karate_Bugman");
            });
        });

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(" • ");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label("Portuguese (Brazil): ");

                ui.hyperlink_to("Darkgaia", "https://ayo.so/darkgaiagames");

                ui.label(", ");

                ui.hyperlink_to("mushiguchi", "https://twitter.com/mushiguchi");
            });
        });

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(" • ");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label("French (France): ");

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
                ui.label("German (Germany): ");

                ui.hyperlink_to("KenDeep", "https://twitch.tv/kendeep_fgc");
            });
        });

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(" • ");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label("Vietnamese: ");

                ui.hyperlink_to("ExeDesmond", "https://twitter.com/exedesmond");

                ui.label(", ");

                ui.hyperlink_to("ShironaNep", "https://www.youtube.com/user/minhduc1411vip");
            });
        });

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(" • ");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label("Russian (Russia): ");

                ui.label("Passbyword");

                ui.label(", ");

                ui.hyperlink_to(
                    "Sest0E1emento5",
                    "https://www.youtube.com/channel/UCwpjuY9bYqNzsUG1QP50PLQ",
                );
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
            ui.hyperlink_to(
                "GNU Affero General Public License v3",
                "https://tldrlegal.com/license/gnu-affero-general-public-license-v3-(agpl-3.0)",
            );
            ui.label(". That means you’re free to modify the ");
            ui.hyperlink_to("source code", "https://github.com/tangobattle");
            ui.label(", as long as you contribute your changes back!");
        });
    });
}
