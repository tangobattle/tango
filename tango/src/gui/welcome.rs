use crate::{config, game, gui, rom, save};

pub struct State {
    nickname: String,
}

impl State {
    pub fn new() -> Self {
        Self {
            nickname: "".to_string(),
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
        gui::language_select::show(ui, font_families, &mut config.language);

        if roms_scanner.read().is_empty() {
            ui.label("no roms lol");
            if ui.button("rescan lol").clicked() {
                let roms_path = config.roms_path();
                let roms_scanner = roms_scanner.clone();
                tokio::runtime::Handle::current().spawn_blocking(move || {
                    roms_scanner.rescan(|| Some(game::scan_roms(&roms_path)));
                });
            }
        }

        if saves_scanner.read().is_empty() {
            ui.label("no saves lol");
            if ui.button("rescan lol").clicked() {
                let saves_path = config.saves_path();
                let saves_scanner = saves_scanner.clone();
                tokio::runtime::Handle::current().spawn_blocking(move || {
                    saves_scanner.rescan(|| Some(save::scan_saves(&saves_path)));
                });
            }
        }

        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut state.nickname);
            if ui.button("save lol").clicked() {
                config.nickname = Some(state.nickname.clone());
            }
        });
    });
}
