use crate::{config, game, gui, rom, save};

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
    saves_scanner: save::Scanner,
    state: &mut State,
) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                gui::language_select::show(ui, font_families, &mut config.language);
            });
        });

        ui.add_space(16.0);

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading("Welcome to Tango!");
                state.emblem.show_scaled(ui, 0.5);
            });

            ui.add_space(8.0);

            let has_roms = !roms_scanner.read().is_empty();
            let has_saves = !saves_scanner.read().is_empty();

            ui.vertical(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if has_roms {
                            ui.label(egui::RichText::new("✅").color(egui::Color32::from_rgb(0x4c, 0xaf, 0x50)));
                        } else {
                            ui.label(egui::RichText::new("⌛").color(egui::Color32::from_rgb(0xf4, 0xba, 0x51)));
                        }
                        ui.strong("step 1 lol");
                    });
                    ui.label("some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text");
                    if ui.button("rescan lol").clicked() {
                        let roms_path = config.roms_path();
                        let roms_scanner = roms_scanner.clone();
                        tokio::runtime::Handle::current().spawn_blocking(move || {
                            roms_scanner.rescan(|| Some(game::scan_roms(&roms_path)));
                        });
                    }
                });

                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if has_saves {
                            ui.label(egui::RichText::new("✅").color(egui::Color32::from_rgb(0x4c, 0xaf, 0x50)));
                        } else {
                            ui.label(egui::RichText::new("⌛").color(egui::Color32::from_rgb(0xf4, 0xba, 0x51)));
                        }
                        ui.strong("step 2 lol");
                    });
                    ui.label("some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text");
                    if ui.button("rescan lol").clicked() {
                        let saves_path = config.saves_path();
                        let saves_scanner = saves_scanner.clone();
                        tokio::runtime::Handle::current().spawn_blocking(move || {
                            saves_scanner.rescan(|| Some(save::scan_saves(&saves_path)));
                        });
                    }
                });

                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("⌛").color(egui::Color32::from_rgb(0xf4, 0xba, 0x51)));
                        ui.strong("step 3 lol");
                    });
                    ui.label("some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text some placeholder text");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut state.nickname);
                        if ui.button("save lol").clicked() {
                            config.nickname = Some(state.nickname.clone());
                        }
                    });
                });
            });
        });
    });
}
