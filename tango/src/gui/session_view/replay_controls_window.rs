use fluent_templates::Loader;

use crate::{i18n, session};

const HIDE_AFTER: std::time::Duration = std::time::Duration::from_secs(5);

pub fn show(
    ctx: &egui::Context,
    session: &session::Session,
    language: &unic_langid::LanguageIdentifier,
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
                if ui
                    .selectable_label(paused, "⏸️")
                    .on_hover_text(i18n::LOCALES.lookup(language, "replay-viewer-pause").unwrap())
                    .clicked()
                {
                    session.set_paused(!paused);
                }
                if ui
                    .button("⏯️")
                    .on_hover_text(i18n::LOCALES.lookup(language, "replay-viewer-step").unwrap())
                    .clicked()
                {
                    session.frame_step();
                }
                let mut speed = session.fps_target() / session::EXPECTED_FPS;
                ui.add(egui::Separator::default().vertical());
                if ui
                    .button("🐢")
                    .on_hover_text(i18n::LOCALES.lookup(language, "replay-viewer-slow-down").unwrap())
                    .clicked()
                {
                    speed = std::cmp::max_by(speed - 0.25, 0.25, |x, y| x.partial_cmp(y).unwrap());
                }
                ui.add(egui::Slider::new(&mut speed, 0.25..=10.0).step_by(0.25))
                    .on_hover_text(i18n::LOCALES.lookup(language, "replay-viewer-speed-up").unwrap());
                if ui
                    .button("🐇")
                    .on_hover_text(i18n::LOCALES.lookup(language, "replay-viewer-pause").unwrap())
                    .clicked()
                {
                    speed = std::cmp::min_by(speed + 0.25, 10.0, |x, y| x.partial_cmp(y).unwrap());
                }
                session.set_fps_target(speed * session::EXPECTED_FPS);
            });
        });
}
