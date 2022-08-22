use fluent_templates::Loader;

use crate::{game, i18n, input, session, video};

struct VBuf {
    buf: Vec<u8>,
    texture: egui::TextureHandle,
}

pub struct Icons {
    pub sports_esports: egui_extras::RetainedImage,
    pub keyboard: egui_extras::RetainedImage,
}

pub struct Gui {
    vbuf: Option<VBuf>,
    icons: Icons,
}

impl Gui {
    pub fn new() -> Self {
        Self {
            vbuf: None,
            icons: Icons {
                sports_esports: egui_extras::RetainedImage::from_image_bytes(
                    "icons.sports_esports",
                    include_bytes!("icons/sports_esports.png"),
                )
                .unwrap(),
                keyboard: egui_extras::RetainedImage::from_image_bytes(
                    "icons.keyboard",
                    include_bytes!("icons/keyboard.png"),
                )
                .unwrap(),
            },
        }
    }

    fn draw_input_mapping_window(
        &mut self,
        ctx: &egui::Context,
        lang: &unic_langid::LanguageIdentifier,
        input_mapping: &mut input::Mapping,
        show_input_capture: &mut bool,
    ) {
        egui::Window::new("")
            .id(egui::Id::new("input-capture-window"))
            .open(show_input_capture)
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        i18n::LOCALES
                            .lookup_with_args(
                                lang,
                                "input-mapping.prompt",
                                &std::collections::HashMap::from([("key", "TODO".into())]),
                            )
                            .unwrap(),
                    )
                    .size(32.0),
                )
            });

        egui::Window::new(i18n::LOCALES.lookup(lang, "input-mapping").unwrap())
            .id(egui::Id::new("input-mapping-window"))
            .show(ctx, |ui| {
                egui::Grid::new("input-mapping-window-grid").show(ui, |ui| {
                    let mut add_row = |label_text_id, mapping: &mut Vec<input::PhysicalInput>| {
                        ui.label(
                            egui::RichText::new(i18n::LOCALES.lookup(lang, label_text_id).unwrap())
                                .strong(),
                        );
                        ui.horizontal(|ui| {
                            for (i, c) in mapping.clone().iter().enumerate() {
                                ui.group(|ui| {
                                    ui.add(
                                        egui::Image::new(
                                            match c {
                                                input::PhysicalInput::Key(_) => {
                                                    &self.icons.keyboard
                                                }
                                                input::PhysicalInput::Button(_)
                                                | input::PhysicalInput::Axis(_, _) => {
                                                    &self.icons.sports_esports
                                                }
                                            }
                                            .texture_id(ctx),
                                            egui::Vec2::new(
                                                ui.text_style_height(&egui::TextStyle::Body),
                                                ui.text_style_height(&egui::TextStyle::Body),
                                            ),
                                        )
                                        .tint(
                                            ui.style()
                                                .visuals
                                                .widgets
                                                .noninteractive
                                                .fg_stroke
                                                .color,
                                        ),
                                    );
                                    ui.label(format!("{:?}", c)); // TODO
                                    if ui.add(egui::Button::new("×").small()).clicked() {
                                        mapping.remove(i);
                                    }
                                });
                            }
                            if ui.add(egui::Button::new("+")).clicked() {
                                *show_input_capture = true;
                            }
                        });
                        ui.end_row();
                    };

                    add_row("input-button.left", &mut input_mapping.left);
                    add_row("input-button.right", &mut input_mapping.right);
                    add_row("input-button.up", &mut input_mapping.up);
                    add_row("input-button.down", &mut input_mapping.down);
                    add_row("input-button.a", &mut input_mapping.a);
                    add_row("input-button.b", &mut input_mapping.b);
                    add_row("input-button.l", &mut input_mapping.l);
                    add_row("input-button.r", &mut input_mapping.r);
                    add_row("input-button.start", &mut input_mapping.start);
                    add_row("input-button.select", &mut input_mapping.select);
                });
            });
    }

    fn draw_debug_window(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        state: &mut game::State,
    ) {
        egui::Window::new("Debug")
            .id(egui::Id::new("debug-window"))
            .open(&mut state.show_debug)
            .show(ctx, |ui| {
                egui::Grid::new("debug-window-grid").show(ui, |ui| {
                    ui.label("FPS");
                    ui.label(
                        egui::RichText::new(format!(
                            "{:3.02}",
                            1.0 / state.fps_counter.lock().mean_duration().as_secs_f32()
                        ))
                        .family(egui::FontFamily::Monospace),
                    );
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
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{:4}",
                                                round.current_tick()
                                            ))
                                            .family(egui::FontFamily::Monospace),
                                        );
                                        ui.end_row();

                                        ui.label("Local player index");
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{:1}",
                                                round.local_player_index()
                                            ))
                                            .family(egui::FontFamily::Monospace),
                                        );
                                        ui.end_row();

                                        ui.label("Queue length");
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{:2} vs {:2} (delay = {:1})",
                                                round.local_queue_length(),
                                                round.remote_queue_length(),
                                                round.local_delay(),
                                            ))
                                            .family(egui::FontFamily::Monospace),
                                        );
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
                        ui.label(
                            egui::RichText::new(format!(
                                "{:3.02} ({:+1.02})",
                                1.0 / state.emu_tps_counter.lock().mean_duration().as_secs_f32(),
                                tps_adjustment
                            ))
                            .family(egui::FontFamily::Monospace),
                        );
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

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
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
        state: &mut game::State,
    ) {
        ctx.set_visuals(egui::style::Visuals::light());

        if let Some(session) = &state.session {
            self.draw_session(
                ctx,
                handle.clone(),
                window,
                input_state,
                &state.input_mapping,
                session,
                &state.title_prefix,
                &state.video_filter,
            );
        }

        if input_state.is_key_pressed(glutin::event::VirtualKeyCode::Grave) {
            state.show_debug = !state.show_debug;
        }
        self.draw_debug_window(ctx, handle.clone(), state);
        self.draw_input_mapping_window(
            ctx,
            &state.lang,
            &mut state.input_mapping,
            &mut state.show_input_capture,
        );
    }
}
