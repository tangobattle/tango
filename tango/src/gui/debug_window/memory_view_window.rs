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

            let row_height = ui.text_style_height(&egui::TextStyle::Body);
            let mut sa = egui::ScrollArea::vertical().auto_shrink([true, false]);
            if jumping {
                if let Ok(jump_to) = u32::from_str_radix(&state.jump_to, 16) {
                    sa = sa.vertical_scroll_offset(
                        (row_height + ui.spacing().item_spacing.y) * (jump_to / 0x10) as f32,
                    );
                }
            }

            const FONT_WIDTH: f32 = 8.0;
            sa.show_rows(ui, row_height, 0x0fffffff / 0x10, |ui, range| {
                egui_extras::StripBuilder::new(ui)
                    .sizes(egui_extras::Size::exact(row_height), range.len())
                    .vertical(|mut outer_strip| {
                        for i in range {
                            outer_strip.cell(|ui| {
                                let rect = ui
                                    .available_rect_before_wrap()
                                    .expand(ui.spacing().item_spacing.y);
                                if i % 2 == 0 {
                                    ui.painter().rect_filled(
                                        rect,
                                        0.0,
                                        ui.visuals().faint_bg_color,
                                    );
                                }

                                egui_extras::StripBuilder::new(ui)
                                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                    .size(egui_extras::Size::exact(8.0 * FONT_WIDTH))
                                    .size(egui_extras::Size::exact(48.0 * FONT_WIDTH))
                                    .size(egui_extras::Size::remainder())
                                    .horizontal(|mut strip| {
                                        let offset = i * 16;
                                        strip.cell(|ui| {
                                            ui.label(
                                                egui::RichText::new(format!("{:08x}", offset))
                                                    .monospace()
                                                    .weak(),
                                            );
                                        });
                                        let bs = audio_guard
                                            .core_mut()
                                            .raw_read_range::<16>(offset as u32, -1);
                                        strip.cell(|ui| {
                                            ui.add(
                                                egui::TextEdit::singleline(
                                                    &mut bs
                                                        .iter()
                                                        .map(|b| format!("{:02x}", b))
                                                        .collect::<Vec<_>>()
                                                        .join(" "),
                                                )
                                                .desired_width(ui.available_width())
                                                .frame(false)
                                                .font(egui::TextStyle::Monospace),
                                            );
                                        });

                                        strip.cell(|ui| {
                                            ui.monospace(
                                                bs.map(|b| {
                                                    if b >= 32 && b < 127 {
                                                        b as char
                                                    } else {
                                                        '.'
                                                    }
                                                })
                                                .iter()
                                                .collect::<String>(),
                                            );
                                        });
                                    });
                            });
                        }
                    });
            });
        });
    if !open {
        *state = None;
    }
}
