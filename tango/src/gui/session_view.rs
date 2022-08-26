use crate::{input, session, video};

struct VBuf {
    buf: Vec<u8>,
    texture: egui::TextureHandle,
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

        let mut scaling_factor = std::cmp::max_by(
            std::cmp::min_by(
                ui.available_width() / mgba::gba::SCREEN_WIDTH as f32,
                ui.available_height() / mgba::gba::SCREEN_HEIGHT as f32,
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
                mgba::gba::SCREEN_WIDTH as f32 * scaling_factor as f32,
                mgba::gba::SCREEN_HEIGHT as f32 * scaling_factor as f32,
            ),
        );
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        input_state: &input::State,
        input_mapping: &input::Mapping,
        session: &session::Session,
        video_filter: &str,
        max_scale: u32,
    ) {
        session.set_joyflags(input_mapping.to_mgba_keys(input_state));

        // If we're in single-player mode, allow speedup.
        if let session::Mode::SinglePlayer = session.mode() {
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
