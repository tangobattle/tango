use crate::{gui, input, session, video};

pub struct State {
    vbuf: Option<VBuf>,
}

impl State {
    pub fn new() -> State {
        Self { vbuf: None }
    }
}

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

fn show_emulator(
    ui: &mut egui::Ui,
    session: &session::Session,
    video_filter: &str,
    max_scale: u32,
    vbuf: &mut Option<VBuf>,
) {
    let video_filter = video::filter_by_name(video_filter).unwrap_or(Box::new(video::NullFilter));

    // Apply stupid video scaling filter that only mint wants 🥴
    let (vbuf_width, vbuf_height) = video_filter.output_size((
        mgba::gba::SCREEN_WIDTH as usize,
        mgba::gba::SCREEN_HEIGHT as usize,
    ));

    let vbuf = if !vbuf
        .as_ref()
        .map(|vbuf| vbuf.texture.size() == [vbuf_width, vbuf_height])
        .unwrap_or(false)
    {
        log::info!("vbuf reallocation: ({}, {})", vbuf_width, vbuf_height);
        vbuf.insert(VBuf::new(ui.ctx(), vbuf_width, vbuf_height))
    } else {
        vbuf.as_mut().unwrap()
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
            ui.available_height() * ui.ctx().pixels_per_point() / mgba::gba::SCREEN_HEIGHT as f32,
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
            mgba::gba::SCREEN_WIDTH as f32 * scaling_factor as f32 / ui.ctx().pixels_per_point(),
            mgba::gba::SCREEN_HEIGHT as f32 * scaling_factor as f32 / ui.ctx().pixels_per_point(),
        ),
    );
    ui.ctx().request_repaint();
}

pub fn show(
    ctx: &egui::Context,
    input_state: &input::State,
    input_mapping: &input::Mapping,
    session: &session::Session,
    video_filter: &str,
    max_scale: u32,
    crashstates_path: &std::path::Path,
    show_escape_window: &mut Option<gui::escape_window::State>,
    state: &mut State,
) {
    session.set_joyflags(input_mapping.to_mgba_keys(input_state));

    if ctx
        .input_mut()
        .consume_key(egui::Modifiers::NONE, egui::Key::Escape)
    {
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
        let mut audio_guard = thread_handle.lock_audio();
        let core = audio_guard.core_mut();
        log::error!(
            r#"mgba thread crashed @ thumb pc = {:08x}!
 r0 = {:08x},  r1 = {:08x},  r2 = {:08x},  r3 = {:08x},
 r4 = {:08x},  r5 = {:08x},  r6 = {:08x},  r7 = {:08x},
 r8 = {:08x},  r9 = {:08x}, r10 = {:08x}, r11 = {:08x},
r12 = {:08x}, r13 = {:08x}, r14 = {:08x}, r15 = {:08x}"#,
            core.as_ref().gba().cpu().thumb_pc(),
            core.as_ref().gba().cpu().gpr(0),
            core.as_ref().gba().cpu().gpr(1),
            core.as_ref().gba().cpu().gpr(2),
            core.as_ref().gba().cpu().gpr(3),
            core.as_ref().gba().cpu().gpr(4),
            core.as_ref().gba().cpu().gpr(5),
            core.as_ref().gba().cpu().gpr(6),
            core.as_ref().gba().cpu().gpr(7),
            core.as_ref().gba().cpu().gpr(8),
            core.as_ref().gba().cpu().gpr(9),
            core.as_ref().gba().cpu().gpr(10),
            core.as_ref().gba().cpu().gpr(11),
            core.as_ref().gba().cpu().gpr(12),
            core.as_ref().gba().cpu().gpr(13),
            core.as_ref().gba().cpu().gpr(14),
            core.as_ref().gba().cpu().gpr(15),
        );
        let state = core.save_state().unwrap();
        let crashstate_path = crashstates_path.join(format!(
                "{}.state",
                time::OffsetDateTime::from(std::time::SystemTime::now())
                    .format(time::macros::format_description!(
                        "[year padding:zero][month padding:zero repr:numerical][day padding:zero][hour padding:zero][minute padding:zero][second padding:zero]"
                    ))
                    .expect("format time"),
            ));
        log::error!("writing crashstate to {}", crashstate_path.display());
        std::fs::write(crashstate_path, state.as_slice()).unwrap();
        panic!("not possible to proceed any further! aborting!");
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(egui::Color32::BLACK))
        .show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    show_emulator(ui, session, video_filter, max_scale, &mut state.vbuf);
                },
            );
        });
}
