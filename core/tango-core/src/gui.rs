use crate::{game, session, video};

struct VBuf {
    buf: Vec<u8>,
    texture: egui::TextureHandle,
}

pub struct Gui {
    vbuf: Option<VBuf>,
}

impl Gui {
    pub fn new() -> Self {
        Self { vbuf: None }
    }

    fn draw_debug(&mut self, ctx: &egui::Context, state: &mut game::State) {
        egui::Window::new("Debug")
            .id(egui::Id::new("debug"))
            .open(&mut state.show_debug)
            .show(ctx, |ui| {
                egui::Grid::new("debug_grid").show(ui, |ui| {
                    ui.label("FPS");
                    ui.label(format!(
                        "{:3.02}",
                        1.0 / state.fps_counter.lock().mean_duration().as_secs_f32()
                    ));
                    ui.end_row();

                    ui.label("Emu TPS");
                    ui.label(format!(
                        "{:3.02}",
                        1.0 / state.emu_tps_counter.lock().mean_duration().as_secs_f32()
                    ));
                    ui.end_row();
                });
            });
    }

    fn draw_emulator(
        &mut self,
        ui: &mut egui::Ui,
        session: &session::Session,
        video_filter: &Box<dyn video::Filter>,
    ) {
        // Apply stupid video scaling filter that only mint wants 🥴
        let (vbuf_width, vbuf_height) = video_filter.output_size((
            mgba::gba::SCREEN_WIDTH as usize,
            mgba::gba::SCREEN_HEIGHT as usize,
        ));

        let make_vbuf = || {
            log::info!("vbuf reallocation: ({}, {})", vbuf_width, vbuf_height);
            VBuf {
                buf: vec![0u8; vbuf_width * vbuf_height * 4],
                texture: ui.ctx().load_texture(
                    "vbuf",
                    egui::ColorImage::new([vbuf_width, vbuf_height], egui::Color32::BLACK),
                    egui::TextureFilter::Nearest,
                ),
            }
        };
        let vbuf = self.vbuf.get_or_insert_with(make_vbuf);
        if vbuf.texture.size() != [vbuf_width, vbuf_height] {
            *vbuf = make_vbuf();
        }

        video_filter.apply(
            &session.lock_vbuf(),
            &mut vbuf.buf,
            (
                mgba::gba::SCREEN_WIDTH as usize,
                mgba::gba::SCREEN_HEIGHT as usize,
            ),
        );

        vbuf.texture.set(
            egui::ColorImage::from_rgba_unmultiplied([vbuf_width, vbuf_height], &vbuf.buf),
            egui::TextureFilter::Nearest,
        );

        let scaling_factor = std::cmp::max_by(
            std::cmp::min_by(
                ui.available_width() / mgba::gba::SCREEN_WIDTH as f32,
                ui.available_height() / mgba::gba::SCREEN_HEIGHT as f32,
                |a, b| a.partial_cmp(b).unwrap(),
            )
            .floor(),
            1.0,
            |a, b| a.partial_cmp(b).unwrap(),
        );
        ui.image(
            &vbuf.texture,
            egui::Vec2::new(
                mgba::gba::SCREEN_WIDTH as f32 * scaling_factor as f32,
                mgba::gba::SCREEN_HEIGHT as f32 * scaling_factor as f32,
            ),
        );
    }

    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        window: &glutin::window::Window,
        input_state: &input_helper::State,
        state: &mut game::State,
    ) {
        ctx.set_pixels_per_point(window.scale_factor() as f32);
        if input_state.is_key_pressed(glutin::event::VirtualKeyCode::Grave as usize) {
            state.show_debug = !state.show_debug;
        }
        self.draw_debug(ctx, state);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    if let Some(session) = &state.session {
                        self.draw_emulator(ui, session, &state.video_filter);
                    }
                },
            );
        });
    }
}
