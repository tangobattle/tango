use crate::{config, session, stats};

mod memory_view_window;

pub struct State {
    memory_view_window: Option<memory_view_window::State>,
}

impl State {
    pub fn new() -> Self {
        Self {
            memory_view_window: None,
        }
    }
}

pub fn show(
    ctx: &egui::Context,
    config: &mut config::Config,
    handle: tokio::runtime::Handle,
    session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    state: &mut State,
) {
    egui::Window::new("")
        .id(egui::Id::new("debug-window"))
        .resizable(false)
        .title_bar(false)
        .open(&mut config.show_debug_overlay)
        .show(ctx, |ui| {
            if ui.button("Open memory viewer").clicked() {
                state.memory_view_window = if state.memory_view_window.is_none() {
                    Some(memory_view_window::State::new())
                } else {
                    None
                };
            }

            egui::Grid::new("debug-window-grid").num_columns(2).show(ui, |ui| {
                ui.strong("FPS");
                ui.label(
                    egui::RichText::new(format!(
                        "{:3.02}",
                        1.0 / fps_counter.lock().mean_duration().as_secs_f32()
                    ))
                    .monospace(),
                );
                ui.end_row();

                if let Some(session) = session.lock().as_ref() {
                    let tps_adjustment = if let session::Mode::PvP(pvp) = session.mode() {
                        handle.block_on(async {
                            if let Some(match_) = &*pvp.match_.lock().await {
                                ui.label("Match active");
                                ui.end_row();

                                let round_state = match_.lock_round_state().await;
                                if let Some(round) = round_state.round.as_ref() {
                                    ui.strong("Current tick");
                                    ui.label(egui::RichText::new(format!("{:4}", round.current_tick())).monospace());
                                    ui.end_row();

                                    ui.strong("Local player index");
                                    ui.label(
                                        egui::RichText::new(format!("{:1}", round.local_player_index())).monospace(),
                                    );
                                    ui.end_row();

                                    ui.strong("Queue length");
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{:2} vs {:2} (delay = {:1})",
                                            round.local_queue_length(),
                                            round.remote_queue_length(),
                                            round.local_delay(),
                                        ))
                                        .monospace(),
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

                    ui.strong("Emu TPS");
                    ui.label(
                        egui::RichText::new(format!(
                            "{:3.02} ({:+1.02})",
                            1.0 / emu_tps_counter.lock().mean_duration().as_secs_f32(),
                            tps_adjustment
                        ))
                        .monospace(),
                    );
                    ui.end_row();
                }
            });
        });

    memory_view_window::show(ctx, session.clone(), &mut state.memory_view_window);
}
