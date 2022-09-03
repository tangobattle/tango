use crate::{gui, input, session, video};

struct VBuf {
    image: egui::ColorImage,
    texture: egui::TextureHandle,
}

impl VBuf {
    fn new(ctx: &egui::Context, width: usize, height: usize) -> Self {
        VBuf {
            image: egui::ColorImage::new([width, height], egui::Color32::BLACK),
            texture: ctx.load_texture(
                "vbuf",
                egui::ColorImage::new([width, height], egui::Color32::BLACK),
                egui::TextureFilter::Nearest,
            ),
        }
    }
}

pub struct SessionView {
    vbuf: Option<VBuf>,
}

impl SessionView {
    pub fn new() -> Self {
        Self { vbuf: None }
    }

    fn show_emulator(
        &mut self,
        ui: &mut egui::Ui,
        session: &session::Session,
        video_filter: &str,
        max_scale: u32,
    ) {
        let video_filter =
            video::filter_by_name(video_filter).unwrap_or(Box::new(video::NullFilter));

        // Apply stupid video scaling filter that only mint wants 🥴
        let (vbuf_width, vbuf_height) = video_filter.output_size((
            mgba::gba::SCREEN_WIDTH as usize,
            mgba::gba::SCREEN_HEIGHT as usize,
        ));

        let vbuf = if !self
            .vbuf
            .as_ref()
            .map(|vbuf| vbuf.texture.size() == [vbuf_width, vbuf_height])
            .unwrap_or(false)
        {
            log::info!("vbuf reallocation: ({}, {})", vbuf_width, vbuf_height);
            self.vbuf
                .insert(VBuf::new(ui.ctx(), vbuf_width, vbuf_height))
        } else {
            self.vbuf.as_mut().unwrap()
        };

        video_filter.apply(
            &session.lock_vbuf(),
            bytemuck::cast_slice_mut(&mut vbuf.image.pixels[..]),
            (
                mgba::gba::SCREEN_WIDTH as usize,
                mgba::gba::SCREEN_HEIGHT as usize,
            ),
        );

        vbuf.texture
            .set(vbuf.image.clone(), egui::TextureFilter::Nearest);

        let mut scaling_factor = std::cmp::max_by(
            std::cmp::min_by(
                ui.available_width() * ui.ctx().pixels_per_point() / mgba::gba::SCREEN_WIDTH as f32,
                ui.available_height() * ui.ctx().pixels_per_point()
                    / mgba::gba::SCREEN_HEIGHT as f32,
                |a, b| a.partial_cmp(b).unwrap(),
            )
            .floor(),
            1.0,
            |a, b| a.partial_cmp(b).unwrap(),
        );
        if max_scale > 0 {
            scaling_factor = std::cmp::min_by(scaling_factor, max_scale as f32, |a, b| {
                a.partial_cmp(b).unwrap()
            });
        }
        ui.image(
            &vbuf.texture,
            egui::Vec2::new(
                mgba::gba::SCREEN_WIDTH as f32 * scaling_factor as f32
                    / ui.ctx().pixels_per_point(),
                mgba::gba::SCREEN_HEIGHT as f32 * scaling_factor as f32
                    / ui.ctx().pixels_per_point(),
            ),
        );
        ui.ctx().request_repaint();
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        input_state: &input::State,
        input_mapping: &input::Mapping,
        session: &session::Session,
        video_filter: &str,
        max_scale: u32,
        show_escape_window: &mut Option<gui::escape_window::State>,
    ) {
        session.set_joyflags(input_mapping.to_mgba_keys(input_state));

        if input_state.is_key_pressed(glutin::event::VirtualKeyCode::Escape) {
            *show_escape_window = if show_escape_window.is_some() {
                None
            } else {
                Some(gui::escape_window::State::new())
            };
        }

        // If we're in single-player or replayer mode, allow speedup.
        match session.mode() {
            session::Mode::SinglePlayer(_) | session::Mode::Replayer => {
                session.set_fps(
                    if input_mapping
                        .speed_up
                        .iter()
                        .any(|c| c.is_active(&input_state))
                    {
                        session::EXPECTED_FPS * 3.0
                    } else {
                        session::EXPECTED_FPS
                    },
                );
            }
            _ => {}
        }

        // If we've crashed, log the error and panic.
        if let Some(thread_handle) = session.has_crashed() {
            // HACK: No better way to lock the core.
            let audio_guard = thread_handle.lock_audio();
            panic!(
                "mgba thread crashed!\nlr = {:08x}, pc = {:08x}",
                audio_guard.core().gba().cpu().gpr(14),
                audio_guard.core().gba().cpu().thumb_pc()
            );
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| {
                        self.show_emulator(ui, session, video_filter, max_scale);
                    },
                );
            });
    }
}
