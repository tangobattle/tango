use crate::{game, input, session, video};

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

    fn draw_debug(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        state: &mut game::State,
    ) {
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

                    if let Some(session) = &state.session {
                        let tps_adjustment = if let Some(match_) = session.match_().as_ref() {
                            handle.block_on(async {
                                if let Some(match_) = &*match_.lock().await {
                                    ui.label("Match active");
                                    ui.end_row();

                                    let round_state = match_.lock_round_state().await;
                                    if let Some(round) = round_state.round.as_ref() {
                                        ui.label("Current tick");
                                        ui.label(format!("{:4}", round.current_tick()));
                                        ui.end_row();

                                        ui.label("Local player index");
                                        ui.label(format!("{:1}", round.local_player_index()));
                                        ui.end_row();

                                        ui.label("Queue length");
                                        ui.label(format!(
                                            "{:2} vs {:2} (delay = {:1})",
                                            round.local_queue_length(),
                                            round.remote_queue_length(),
                                            round.local_delay(),
                                        ));
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

                        ui.label("Emu TPS");
                        ui.label(format!(
                            "{:3.02} ({:+1.02})",
                            1.0 / state.emu_tps_counter.lock().mean_duration().as_secs_f32(),
                            tps_adjustment
                        ));
                        ui.end_row();
                    }
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

    pub fn draw_session(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        window: &glutin::window::Window,
        input_state: &input::State,
        input_mapping: &input::Mapping,
        session: &session::Session,
        title_prefix: &str,
        video_filter: &Box<dyn video::Filter>,
    ) {
        session.set_joyflags(input_mapping.to_mgba_keys(input_state));

        // If we're in single-player mode, allow speedup.
        if session.match_().is_none() {
            session.set_fps(
                if input_mapping
                    .speed_up
                    .iter()
                    .any(|c| c.is_active(&input_state))
                {
                    game::EXPECTED_FPS * 3.0
                } else {
                    game::EXPECTED_FPS
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

        // Update title to show P1/P2 state.
        let mut title = title_prefix.to_string();
        if let Some(match_) = session.match_().as_ref() {
            handle.block_on(async {
                if let Some(match_) = &*match_.lock().await {
                    let round_state = match_.lock_round_state().await;
                    if let Some(round) = round_state.round.as_ref() {
                        title = format!("{} [P{}]", title, round.local_player_index() + 1);
                    }
                }
            });
        }

        window.set_title(&title);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    self.draw_emulator(ui, session, video_filter);
                },
            );
        });
    }

    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        window: &glutin::window::Window,
        input_state: &input::State,
        input_mapping: &input::Mapping,
        state: &mut game::State,
    ) {
        ctx.set_pixels_per_point(window.scale_factor() as f32);

        if let Some(session) = &state.session {
            self.draw_session(
                ctx,
                handle.clone(),
                window,
                input_state,
                input_mapping,
                session,
                &state.title_prefix,
                &state.video_filter,
            );
        }

        if input_state.is_key_pressed(glutin::event::VirtualKeyCode::Grave) {
            state.show_debug = !state.show_debug;
        }
        self.draw_debug(ctx, handle.clone(), state);
    }
}
