use fluent_templates::Loader;

use crate::{config, game, gui, i18n, rom};

pub struct State {
    nickname: String,
    emblem: egui_extras::RetainedImage,
}

impl State {
    pub fn new() -> Self {
        Self {
            nickname: "".to_string(),
            emblem: egui_extras::RetainedImage::from_image_bytes("emblem", include_bytes!("../emblem.png")).unwrap(),
        }
    }
}

pub fn show(
    ctx: &egui::Context,
    font_families: &gui::FontFamilies,
    config: &mut config::Config,
    roms_scanner: rom::Scanner,
    state: &mut State,
) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.horizontal_centered(|ui| {
            ui.add_space(8.0);
            state.emblem.show_scaled(ui, 0.5);

            ui.add_space(8.0);
            ui.add(egui::Separator::default().vertical());
            ui.add_space(8.0);

            let has_roms = !roms_scanner.read().is_empty();

            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        gui::language_select::show(ui, font_families, &mut config.language);
                    });
                });

                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.heading(i18n::LOCALES.lookup(&config.language, "welcome-heading").unwrap());
                    ui.label(i18n::LOCALES.lookup(&config.language, "welcome-description").unwrap());

                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        if has_roms {
                            ui.label(egui::RichText::new("✅").color(egui::Color32::from_rgb(0x4c, 0xaf, 0x50)));
                        } else {
                            ui.label(egui::RichText::new("⌛").color(egui::Color32::from_rgb(0xf4, 0xba, 0x51)));
                        }
                        ui.strong(i18n::LOCALES.lookup(&config.language, "welcome-step-1").unwrap());
                    });
                    if !has_roms {
                        ui.label(
                            i18n::LOCALES
                                .lookup(&config.language, "welcome-step-1-description")
                                .unwrap(),
                        );
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(!roms_scanner.is_scanning(), |ui| {
                                if ui
                                    .button(i18n::LOCALES.lookup(&config.language, "welcome-continue").unwrap())
                                    .clicked()
                                {
                                    let roms_path = config.roms_path();
                                    let allow_detached_roms = config.allow_detached_roms;
                                    let roms_scanner = roms_scanner.clone();
                                    let egui_ctx = ui.ctx().clone();
                                    tokio::task::spawn_blocking(move || {
                                        roms_scanner.rescan(|| Some(game::scan_roms(&roms_path, allow_detached_roms)));
                                        egui_ctx.request_repaint();
                                    });
                                }
                            });
                        });
                    }
                });

                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("⌛").color(egui::Color32::from_rgb(0xf4, 0xba, 0x51)));
                        ui.strong(i18n::LOCALES.lookup(&config.language, "welcome-step-3").unwrap());
                    });
                    if has_roms {
                        ui.label(
                            i18n::LOCALES
                                .lookup(&config.language, "welcome-step-3-description")
                                .unwrap(),
                        );
                        ui.horizontal(|ui| {
                            let mut submitted = false;
                            let input_resp = ui.add(
                                egui::TextEdit::singleline(&mut state.nickname)
                                    .hint_text(i18n::LOCALES.lookup(&config.language, "settings-nickname").unwrap())
                                    .desired_width(200.0),
                            );
                            if input_resp.lost_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
                                submitted = true;
                            }
                            state.nickname = state.nickname.chars().take(20).collect::<String>().trim().to_string();

                            if ui
                                .button(i18n::LOCALES.lookup(&config.language, "welcome-continue").unwrap())
                                .clicked()
                            {
                                submitted = true;
                            }

                            if submitted && !state.nickname.is_empty() {
                                config.nickname = Some(state.nickname.clone());
                            }
                        });
                    }
                });
            });
        });
    });
}
