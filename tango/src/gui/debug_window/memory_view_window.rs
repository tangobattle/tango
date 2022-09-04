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

pub fn show(
    ctx: &egui::Context,
    session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    state: &mut Option<State>,
) {
    let mut open = state.is_some();
    egui::Window::new("Memory viewer")
        .id(egui::Id::new("memory-viewer"))
        .open(&mut open)
        .show(ctx, |ui| {
            const FONT_WIDTH: f32 = 10.0;
            let state = state.as_mut().unwrap();

            let mut jumping = false;
            ui.horizontal(|ui| {
                let input_resp = ui.add(
                    egui::TextEdit::singleline(&mut state.jump_to)
                        .desired_width(8.0 * FONT_WIDTH)
                        .hint_text("Jump to")
                        .font(egui::TextStyle::Monospace),
                );
                state.jump_to = state
                    .jump_to
                    .chars()
                    .filter(|c| "0123456789abcdefABCDEF".chars().any(|c2| c2 == *c))
                    .collect();
                if input_resp.lost_focus() && ui.ctx().input().key_pressed(egui::Key::Enter) {
                    jumping = true;
                }

                if ui.button("Go!").clicked() {
                    jumping = true;
                }
            });
            let session = session.lock();
            let session = if let Some(session) = session.as_ref() {
                session
            } else {
                ui.label("No session in progress.");
                return;
            };

            let thread_handle = session.thread_handle();
            let mut audio_guard = thread_handle.lock_audio();

            const ROW_HEIGHT: f32 = 18.0;
            let mut sa = egui::ScrollArea::vertical().auto_shrink([true, false]);
            if jumping {
                if let Ok(jump_to) = u32::from_str_radix(&state.jump_to, 16) {
                    sa = sa.vertical_scroll_offset(
                        (ROW_HEIGHT + ui.spacing().item_spacing.y) * (jump_to / 0x10) as f32,
                    );
                }
            }

            sa.show_rows(ui, ROW_HEIGHT, 0x0fffffff / 0x10, |ui, range| {
                ui.vertical(|ui| {
                    for i in range {
                        ui.horizontal_top(|ui| {
                            ui.set_height(ROW_HEIGHT);
                            let offset = i * 16;
                            ui.label(
                                egui::RichText::new(format!("{:08x}", offset))
                                    .monospace()
                                    .weak(),
                            );
                            let bs = audio_guard
                                .core_mut()
                                .raw_read_range::<16>(offset as u32, -1);
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut bs
                                        .iter()
                                        .map(|b| format!("{:02x}", b))
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                )
                                .frame(false)
                                .font(egui::TextStyle::Monospace),
                            );
                            ui.monospace(
                                bs.map(|b| if b >= 32 && b < 127 { b as char } else { '.' })
                                    .iter()
                                    .collect::<String>(),
                            );
                        });
                    }
                });
            });
        });
    if !open {
        *state = None;
    }
}
