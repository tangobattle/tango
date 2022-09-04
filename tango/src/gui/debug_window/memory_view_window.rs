use crate::session;

pub struct State {
    jump_to: String,
}

impl State {
    pub fn new() -> Self {
        Self {
            jump_to: "".to_string(),
        }
    }
}

pub struct MemoryViewWindow {}

impl MemoryViewWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
        state: &mut Option<State>,
    ) {
        let mut open = state.is_some();
        egui::Window::new("Memory viewer")
            .id(egui::Id::new("memory-viewer"))
            .open(&mut open)
            .show(ctx, |ui| {
                let state = state.as_mut().unwrap();

                ui.horizontal(|ui| {
                    let mut submitted = false;
                    let input_resp = ui.add(
                        egui::TextEdit::singleline(&mut state.jump_to)
                            .desired_width(8.0 * 12.0)
                            .hint_text("Jump to")
                            .font(egui::TextStyle::Monospace),
                    );
                    if input_resp.lost_focus() && ui.ctx().input().key_pressed(egui::Key::Enter) {
                        submitted = true;
                    }

                    if ui.button("Go!").clicked() {
                        submitted = true;
                    }

                    if submitted {
                        log::info!("Submitted");
                    }
                });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let session = session.lock();
                        let session = if let Some(session) = session.as_ref() {
                            session
                        } else {
                            ui.label("No session in progress.");
                            return;
                        };

                        let thread_handle = session.thread_handle();
                        let audio_guard = thread_handle.lock_audio();
                    });
            });
        if !open {
            *state = None;
        }
    }
}
