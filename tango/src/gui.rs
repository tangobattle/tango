use std::str::FromStr;

use fluent_templates::Loader;

use crate::{audio, config, games, i18n, input, session, stats, video};

const DISCORD_APP_ID: u64 = 974089681333534750;

mod about;
mod menubar;
mod play;
mod settings;

pub struct State {
    pub config: config::Config,
    pub session: Option<session::Session>,
    pub steal_input: Option<StealInputState>,
    roms: std::collections::HashMap<&'static (dyn games::Game + Send + Sync), Vec<u8>>,
    saves: std::collections::HashMap<
        &'static (dyn games::Game + Send + Sync),
        Vec<std::path::PathBuf>,
    >,
    audio_binder: audio::LateBinder,
    fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    show_menubar: bool,
    show_play: Option<play::State>,
    show_settings: Option<settings::State>,
    show_about: bool,
    drpc: discord_rpc_client::Client,
}

impl State {
    pub fn new(
        config: config::Config,
        audio_binder: audio::LateBinder,
        fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
        emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    ) -> Self {
        let mut drpc = discord_rpc_client::Client::new(DISCORD_APP_ID);
        drpc.start();

        let roms = games::scan_roms(&config.roms_path);
        let saves = games::scan_saves(&config.saves_path);
        Self {
            config,
            roms,
            saves,
            audio_binder,
            fps_counter,
            emu_tps_counter,
            session: None,
            steal_input: None,
            show_menubar: false,
            show_play: None,
            show_settings: None,
            show_about: false,
            drpc,
        }
    }
}

pub struct StealInputState {
    callback: Box<dyn Fn(input::PhysicalInput, &mut input::Mapping)>,
    userdata: Box<dyn std::any::Any>,
}

impl StealInputState {
    pub fn run_callback(&self, phy: input::PhysicalInput, mapping: &mut input::Mapping) {
        (self.callback)(phy, mapping)
    }
}

struct VBuf {
    buf: Vec<u8>,
    texture: egui::TextureHandle,
}

struct Themes {
    light: egui::style::Visuals,
    dark: egui::style::Visuals,
}

#[derive(Clone)]
pub struct FontFamilies {
    latn: egui::FontFamily,
    jpan: egui::FontFamily,
    hans: egui::FontFamily,
    hant: egui::FontFamily,
}

pub struct Gui {
    vbuf: Option<VBuf>,
    menubar: menubar::Menubar,
    play: play::Play,
    about: about::About,
    settings: settings::Settings,
    font_data: std::collections::BTreeMap<String, egui::FontData>,
    font_families: FontFamilies,
    themes: Themes,
    current_language: Option<unic_langid::LanguageIdentifier>,
}

impl Gui {
    pub fn new(ctx: &egui::Context) -> Self {
        let font_families = FontFamilies {
            latn: egui::FontFamily::Name("Latn".into()),
            jpan: egui::FontFamily::Name("Jpan".into()),
            hans: egui::FontFamily::Name("Hans".into()),
            hant: egui::FontFamily::Name("Hant".into()),
        };

        ctx.set_fonts(egui::FontDefinitions {
            font_data: std::collections::BTreeMap::default(),
            families: std::collections::BTreeMap::from([
                (egui::FontFamily::Proportional, vec![]),
                (egui::FontFamily::Monospace, vec![]),
                (font_families.latn.clone(), vec![]),
                (font_families.jpan.clone(), vec![]),
                (font_families.hans.clone(), vec![]),
                (font_families.hant.clone(), vec![]),
            ]),
        });

        Self {
            vbuf: None,
            menubar: menubar::Menubar::new(),
            play: play::Play::new(),
            settings: settings::Settings::new(font_families.clone()),
            about: about::About::new(),
            font_data: std::collections::BTreeMap::from([
                (
                    "NotoSans-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSans-Regular.ttf")),
                ),
                (
                    "NotoSansJP-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSansJP-Regular.otf")),
                ),
                (
                    "NotoSansSC-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSansSC-Regular.otf")),
                ),
                (
                    "NotoSansTC-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSansTC-Regular.otf")),
                ),
                (
                    "NotoSansMono-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoSansMono-Regular.ttf")),
                ),
                (
                    "NotoEmoji-Regular".to_string(),
                    egui::FontData::from_static(include_bytes!("fonts/NotoEmoji-Regular.ttf")),
                ),
            ]),
            font_families,
            themes: Themes {
                light: {
                    let mut visuals = egui::style::Visuals::light();
                    visuals.selection.bg_fill = egui::Color32::from_rgb(0x4c, 0xaf, 0x50);
                    visuals.selection.stroke.color = egui::Color32::BLACK;
                    visuals
                },
                dark: {
                    let mut visuals = egui::style::Visuals::dark();
                    visuals.selection.bg_fill = egui::Color32::from_rgb(0x4c, 0xaf, 0x50);
                    visuals.selection.stroke.color = egui::Color32::WHITE;
                    visuals
                },
            },
            current_language: None,
        }
    }

    fn draw_debug_overlay(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        state: &mut State,
    ) {
        egui::Window::new("")
            .id(egui::Id::new("debug-window"))
            .resizable(false)
            .title_bar(false)
            .open(&mut state.config.show_debug_overlay)
            .show(ctx, |ui| {
                egui::Grid::new("debug-window-grid")
                    .num_columns(2)
                    .show(ui, |ui| {
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
                            let tps_adjustment = if let session::Mode::PvP(match_) = session.mode()
                            {
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
                                    1.0 / state
                                        .emu_tps_counter
                                        .lock()
                                        .mean_duration()
                                        .as_secs_f32(),
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

    pub fn show_session(
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
                        self.draw_emulator(ui, session, video_filter, max_scale);
                    },
                );
            });
    }

    pub fn show_steal_input_dialog(
        &mut self,
        ctx: &egui::Context,
        language: &unic_langid::LanguageIdentifier,
        steal_input: &mut Option<StealInputState>,
    ) {
        let mut steal_input_open = steal_input.is_some();
        if let Some(inner_response) = egui::Window::new("")
            .id(egui::Id::new("input-capture-window"))
            .open(&mut steal_input_open)
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::top_down_justified(egui::Align::Center),
                    |ui| {
                        egui::Frame::none()
                            .inner_margin(egui::style::Margin::symmetric(32.0, 16.0))
                            .show(ui, |ui| {
                                let userdata =
                                    if let Some(StealInputState { userdata, .. }) = &steal_input {
                                        userdata
                                    } else {
                                        unreachable!();
                                    };

                                ui.label(
                                    egui::RichText::new(
                                        i18n::LOCALES
                                            .lookup_with_args(
                                                &language,
                                                "input-mapping.prompt",
                                                &std::collections::HashMap::from([(
                                                    "key",
                                                    i18n::LOCALES
                                                        .lookup(
                                                            &language,
                                                            userdata
                                                                .downcast_ref::<&str>()
                                                                .unwrap(),
                                                        )
                                                        .unwrap()
                                                        .into(),
                                                )]),
                                            )
                                            .unwrap(),
                                    )
                                    .size(32.0),
                                );
                            });
                    },
                );
            })
        {
            ctx.move_to_top(inner_response.response.layer_id);
        }
        if !steal_input_open {
            *steal_input = None;
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        _window: &glutin::window::Window,
        input_state: &input::State,
        state: &mut State,
    ) {
        if self.current_language.as_ref() != Some(&state.config.language) {
            let mut language = state.config.language.clone();
            language.maximize();

            let primary_font = match language.script {
                Some(s) if s == unic_langid::subtags::Script::from_str("Jpan").unwrap() => {
                    "NotoSansJP-Regular"
                }
                Some(s) if s == unic_langid::subtags::Script::from_str("Hans").unwrap() => {
                    "NotoSansSC-Regular"
                }
                Some(s) if s == unic_langid::subtags::Script::from_str("Hant").unwrap() => {
                    "NotoSansTC-Regular"
                }
                _ => "NotoSans-Regular",
            };

            let proportional = vec![
                primary_font.to_string(),
                "NotoSans-Regular".to_string(),
                "NotoSansJP-Regular".to_string(),
                "NotoSansSC-Regular".to_string(),
                "NotoSansTC-Regular".to_string(),
                "NotoEmoji-Regular".to_string(),
            ];

            let mut monospace = vec!["NotoSansMono-Regular".to_string()];
            monospace.extend(proportional.clone());

            ctx.set_fonts(egui::FontDefinitions {
                font_data: self.font_data.clone(),
                families: std::collections::BTreeMap::from([
                    (egui::FontFamily::Proportional, proportional),
                    (egui::FontFamily::Monospace, monospace),
                    (
                        self.font_families.jpan.clone(),
                        vec!["NotoSansJP-Regular".to_string()],
                    ),
                    (
                        self.font_families.hans.clone(),
                        vec!["NotoSansSC-Regular".to_string()],
                    ),
                    (
                        self.font_families.hant.clone(),
                        vec!["NotoSansTC-Regular".to_string()],
                    ),
                    (
                        self.font_families.latn.clone(),
                        vec!["NotoSans-Regular".to_string()],
                    ),
                ]),
            });
            self.current_language = Some(state.config.language.clone());
            log::info!(
                "language was changed to {}",
                self.current_language.as_ref().unwrap()
            );
        }

        ctx.set_visuals(match state.config.theme {
            config::Theme::System => match dark_light::detect() {
                dark_light::Mode::Light => self.themes.light.clone(),
                dark_light::Mode::Dark => self.themes.dark.clone(),
            },
            config::Theme::Light => self.themes.light.clone(),
            config::Theme::Dark => self.themes.dark.clone(),
        });

        if input_state.is_key_pressed(glutin::event::VirtualKeyCode::Escape) {
            state.show_menubar = !state.show_menubar;
        }
        if state.session.is_none() {
            state.show_menubar = true;
        }

        self.draw_debug_overlay(ctx, handle.clone(), state);
        self.play.show(
            ctx,
            &mut state.show_play,
            &mut state.show_menubar,
            &state.config.language,
            &state.config.saves_path,
            &mut state.session,
            &mut state.roms,
            &mut state.saves,
            state.audio_binder.clone(),
            state.emu_tps_counter.clone(),
        );
        self.settings.show(
            ctx,
            &mut state.show_settings,
            &mut state.config,
            &mut state.steal_input,
        );
        self.about
            .show(ctx, &state.config.language, &mut state.show_about);
        self.show_steal_input_dialog(ctx, &state.config.language, &mut state.steal_input);

        if let Some(session) = &state.session {
            self.show_session(
                ctx,
                input_state,
                &state.config.input_mapping,
                session,
                &state.config.video_filter,
                state.config.max_scale,
            );
        }

        if state.show_menubar {
            self.menubar.show(
                ctx,
                &state.config.language,
                &mut state.show_play,
                &mut state.show_settings,
                &mut state.show_about,
            );
        }
    }
}
