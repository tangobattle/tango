use fluent_templates::Loader;

use crate::{discord, gui, i18n, input, session, stats, sync, video};

mod replay_controls_window;

pub struct State {
    vbuf: Option<VBuf>,
    opponent_save_view: gui::save_view::State,
    own_save_view: gui::save_view::State,
    debug_window: Option<gui::debug_window::State>,
}

impl State {
    pub fn new() -> State {
        Self {
            vbuf: None,
            opponent_save_view: gui::save_view::State::new(),
            own_save_view: gui::save_view::State::new(),
            debug_window: None,
        }
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
                egui::TextureOptions::NEAREST,
            ),
        }
    }
}

fn show_emulator(
    ui: &mut egui::Ui,
    session: &session::Session,
    video_filter: &str,
    max_scale: u32,
    integer_scaling: bool,
    vbuf: &mut Option<VBuf>,
) {
    let video_filter = video::filter_by_name(video_filter).unwrap_or(Box::new(video::NullFilter));

    // Apply stupid video scaling filter that only mint wants 🥴
    let [vbuf_width, vbuf_height] =
        video_filter.output_size([mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize]);

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
        [mgba::gba::SCREEN_WIDTH as usize, mgba::gba::SCREEN_HEIGHT as usize],
    );

    vbuf.texture.set(vbuf.image.clone(), egui::TextureOptions::NEAREST);

    let mut scaling_factor = std::cmp::min_by(
        ui.available_width() * ui.ctx().pixels_per_point() / mgba::gba::SCREEN_WIDTH as f32,
        ui.available_height() * ui.ctx().pixels_per_point() / mgba::gba::SCREEN_HEIGHT as f32,
        |a, b| a.partial_cmp(b).unwrap(),
    );

    if integer_scaling {
        scaling_factor = scaling_factor.floor();
    }

    scaling_factor = std::cmp::max_by(scaling_factor, 1.0, |a, b| a.partial_cmp(b).unwrap());
    if max_scale > 0 {
        scaling_factor = std::cmp::min_by(scaling_factor, max_scale as f32, |a, b| a.partial_cmp(b).unwrap());
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
    language: &unic_langid::LanguageIdentifier,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    input_state: &input::State,
    input_mapping: &input::Mapping,
    session: &session::Session,
    video_filter: &str,
    integer_scaling: bool,
    max_scale: u32,
    speed_change_factor: f32,
    show_own_setup: bool,
    crashstates_path: &std::path::Path,
    last_mouse_motion_time: &Option<std::time::Instant>,
    show_escape_window: &mut Option<gui::escape_window::State>,
    fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    show_debug: bool,
    always_show_status_bar: Option<bool>,
    state: &mut State,
    discord_client: &mut discord::Client,
) {
    if input_mapping.menu.iter().any(|c| c.is_pressed(input_state)) {
        *show_escape_window = if show_escape_window.is_some() {
            None
        } else {
            Some(gui::escape_window::State::new())
        };
    }

    let game_info = session.game_info();
    match session.mode() {
        session::Mode::SinglePlayer(_) => {
            discord_client.set_current_activity(Some(discord::make_single_player_activity(
                session.start_time(),
                language,
                Some(discord::make_game_info(
                    game_info.game,
                    game_info.patch.as_ref().map(|(name, version)| (name.as_str(), version)),
                    language,
                )),
            )));
        }
        session::Mode::PvP(_) => {
            discord_client.set_current_activity(Some(discord::make_in_progress_activity(
                session.start_time(),
                language,
                Some(discord::make_game_info(
                    game_info.game,
                    game_info.patch.as_ref().map(|(name, version)| (name.as_str(), version)),
                    language,
                )),
            )));
        }
        session::Mode::Replayer => {
            discord_client.set_current_activity(Some(discord::make_base_activity(None)));
        }
    }

    match session.mode() {
        session::Mode::SinglePlayer(_) => {
            session.set_fps_target(
                if input_mapping.speed_change.iter().any(|c| c.is_active(&input_state)) {
                    session::EXPECTED_FPS * speed_change_factor
                } else {
                    session::EXPECTED_FPS
                },
            );
        }
        session::Mode::Replayer => {
            replay_controls_window::show(ctx, session, language, last_mouse_motion_time);
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
r12 = {:08x}, r13 = {:08x}, r14 = {:08x}, r15 = {:08x},
cpsr = {:08x}"#,
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
            core.as_ref().gba().cpu().cpsr(),
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

    if show_own_setup {
        if let Some(own_setup) = session.own_setup().as_ref() {
            egui::SidePanel::left("own-setup-panel").show(ctx, |ui| {
                egui::ScrollArea::horizontal()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.heading(i18n::LOCALES.lookup(language, "own-setup").unwrap());
                        gui::save_view::show(
                            ui,
                            false,
                            clipboard,
                            font_families,
                            language,
                            &own_setup.game_lang,
                            own_setup.save.as_ref(),
                            own_setup.assets.as_ref(),
                            &mut state.own_save_view,
                            true,
                        )
                    });
            });
        }
    }

    if let Some(opponent_setup) = session.opponent_setup().as_ref() {
        egui::SidePanel::right("opponent-setup-panel").show(ctx, |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.heading(i18n::LOCALES.lookup(language, "opponent-setup").unwrap());
                    gui::save_view::show(
                        ui,
                        false,
                        clipboard,
                        font_families,
                        language,
                        &opponent_setup.game_lang,
                        opponent_setup.save.as_ref(),
                        opponent_setup.assets.as_ref(),
                        &mut state.opponent_save_view,
                        true,
                    );
                });
        });
    }

    if always_show_status_bar.unwrap_or(false) {
        // This shows the status bar on top of everything.
        show_status_bar(
            ctx,
            language,
            session,
            show_debug,
            &mut state.debug_window,
            fps_counter.clone(),
            emu_tps_counter.clone(),
        );
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(egui::Color32::BLACK))
        .show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    show_emulator(ui, session, video_filter, max_scale, integer_scaling, &mut state.vbuf);
                },
            );
        });

    const HIDE_AFTER: std::time::Duration = std::time::Duration::from_secs(3);
    if always_show_status_bar.is_none()
        && last_mouse_motion_time
            .map(|t| std::time::Instant::now() - t < HIDE_AFTER)
            .unwrap_or(false)
    {
        // This adjusts the layout.
        show_status_bar(
            ctx,
            language,
            session,
            show_debug,
            &mut state.debug_window,
            fps_counter.clone(),
            emu_tps_counter.clone(),
        );
    }
    gui::debug_window::show(ctx, language, session, &mut state.debug_window);
}

fn show_status_bar(
    ctx: &egui::Context,
    language: &unic_langid::LanguageIdentifier,
    session: &session::Session,
    show_debug: bool,
    debug_window: &mut Option<gui::debug_window::State>,
    fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
) {
    egui::TopBottomPanel::bottom("session-status-bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (tps_adjustment, latency, round_info) = (|| {
                    let pvp = if let session::Mode::PvP(pvp) = session.mode() {
                        pvp
                    } else {
                        return (0.0, None, None);
                    };

                    let match_ = sync::block_on(pvp.match_.lock());
                    let match_ = if let Some(match_) = &*match_ {
                        match_
                    } else {
                        return (0.0, None, None);
                    };

                    let latency = sync::block_on(pvp.latency());

                    let round_state = sync::block_on(match_.lock_round_state());
                    let round = if let Some(round) = round_state.round.as_ref() {
                        round
                    } else {
                        return (0.0, Some(latency), None);
                    };

                    (
                        round.tps_adjustment(),
                        Some(latency),
                        Some((
                            round.local_queue_length(),
                            round.remote_queue_length(),
                            round.local_delay(),
                            round.current_tick(),
                            round.local_player_index(),
                        )),
                    )
                })();

                if show_debug {
                    let debug_window_open = debug_window.is_some();
                    if ui
                        .selectable_label(debug_window_open, "🪲")
                        .on_hover_text(i18n::LOCALES.lookup(language, "debug").unwrap())
                        .clicked()
                    {
                        *debug_window = if debug_window.is_some() {
                            None
                        } else {
                            Some(gui::debug_window::State::new())
                        };
                    }
                }

                if show_debug {
                    ui.add(egui::Separator::default().vertical());
                    ui.monospace(format!(
                        "fps {:7.2}",
                        1.0 / fps_counter.lock().mean_duration().as_secs_f32()
                    ));

                    ui.add(egui::Separator::default().vertical());
                    ui.monospace(format!(
                        "tps {:7.2} ({:+5.2})",
                        1.0 / emu_tps_counter.lock().mean_duration().as_secs_f32(),
                        tps_adjustment
                    ));
                }

                if let Some((local_qlen, remote_qlen, local_delay, current_tick, _)) = round_info {
                    if show_debug {
                        ui.add(egui::Separator::default().vertical());
                        ui.monospace(format!(
                            "qlen {:2} vs {:2} (delay = {:2})",
                            local_qlen, remote_qlen, local_delay
                        ));

                        ui.add(egui::Separator::default().vertical());
                        ui.monospace(format!("tick {:5}", current_tick));
                    } else {
                        ui.add(egui::Separator::default().vertical());
                        ui.monospace(format!("rollback ticks {:2}", local_qlen.saturating_sub(remote_qlen)));
                    }
                }

                if let Some(latency) = latency {
                    ui.add(egui::Separator::default().vertical());
                    ui.monospace(format!("ping {:4}ms", latency.as_millis()));
                }

                if let Some((_, _, _, _, local_player_index)) = round_info {
                    ui.add(egui::Separator::default().vertical());
                    ui.monospace(format!("P{}", local_player_index + 1));
                }

                ui.add(egui::Separator::default().vertical());
            });
        });
    });
}
