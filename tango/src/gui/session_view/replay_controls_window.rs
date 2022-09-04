use crate::session;

const HIDE_AFTER: std::time::Duration = std::time::Duration::from_secs(5);

pub fn show(
    ctx: &egui::Context,
    session: &session::Session,
    last_mouse_motion_time: &Option<std::time::Instant>,
) {
    let paused = session.is_paused();
    egui::Window::new("")
        .id(egui::Id::new("replay-controls-window"))
        .resizable(false)
        .title_bar(false)
        .open(&mut {
            paused
                || last_mouse_motion_time
                    .map(|t| std::time::Instant::now() - t < HIDE_AFTER)
                    .unwrap_or(false)
        })
        .anchor(egui::Align2::CENTER_BOTTOM, egui::Vec2::new(0.0, -50.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(paused, "⏸️").clicked() {
                    session.set_paused(!paused);
                }
                let mut speed = session.fps_target() / session::EXPECTED_FPS;
                ui.add(egui::Separator::default().vertical());
                ui.label("🐢");
                ui.add(egui::Slider::new(&mut speed, 0.5..=10.0).step_by(0.25));
                ui.label("🐇");
                session.set_fps_target(speed * session::EXPECTED_FPS);
            });
        });
}
