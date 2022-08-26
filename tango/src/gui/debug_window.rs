use crate::{gui, session};

pub struct DebugWindow {}

impl DebugWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        state: &mut gui::State,
    ) {
        egui::Window::new("")
            .id(egui::Id::new("debug-window"))
            .resizable(false)
            .title_bar(false)
            .open(&mut state.config.show_debug_overlay)
            .show(ctx, |ui| {
                egui::Grid::new("debug-window-grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("FPS");
                        ui.label(
                            egui::RichText::new(format!(
                                "{:3.02}",
                                1.0 / state.fps_counter.lock().mean_duration().as_secs_f32()
                            ))
                            .family(egui::FontFamily::Monospace),
                        );
                        ui.end_row();

                        if let gui::main_view::State::Session(session) = &*state.main_view.lock() {
                            let tps_adjustment = if let session::Mode::PvP(match_) = session.mode()
                            {
                                handle.block_on(async {
                                    if let Some(match_) = &*match_.lock().await {
                                        ui.label("Match active");
                                        ui.end_row();

                                        let round_state = match_.lock_round_state().await;
                                        if let Some(round) = round_state.round.as_ref() {
                                            ui.label("Current tick");
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{:4}",
                                                    round.current_tick()
                                                ))
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
                                    1.0 / state
                                        .emu_tps_counter
                                        .lock()
                                        .mean_duration()
                                        .as_secs_f32(),
                                    tps_adjustment
                                ))
                                .family(egui::FontFamily::Monospace),
                            );
                            ui.end_row();
                        }
                    });
            });
    }
}
