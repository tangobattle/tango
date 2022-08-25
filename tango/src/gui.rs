use crate::{audio, config, games, i18n, input, net, session, stats, video};
use fluent_templates::Loader;
use std::str::FromStr;

const DISCORD_APP_ID: u64 = 974089681333534750;

mod save_select_window;
mod settings_window;

enum MainScreenState {
    Session(session::Session),
    Start(Start),
}

enum ConnectionState {
    Starting,
    Signaling,
    Waiting,
    Ready(
        (
            datachannel_wrapper::DataChannel,
            datachannel_wrapper::PeerConnection,
        ),
    ),
    Failed(anyhow::Error), // TODO: Not this
}

struct Start {
    link_code: String,
    connection_state: std::sync::Arc<tokio::sync::Mutex<Option<ConnectionState>>>,
    show_save_select: Option<save_select_window::State>,
}

impl Start {
    fn new() -> Self {
        Self {
            link_code: String::new(),
            connection_state: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            show_save_select: None,
        }
    }
}

pub struct State {
    pub config: config::Config,
    pub steal_input: Option<StealInputState>,
    saves_list: SavesListState,
    audio_binder: audio::LateBinder,
    fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    main_screen: MainScreenState,
    show_settings: Option<settings_window::State>,
    drpc: discord_rpc_client::Client,
}

#[derive(Clone)]
pub struct SavesListState {
    inner: std::sync::Arc<parking_lot::RwLock<SavesListStateInner>>,
}

pub struct SavesListStateInner {
    roms: std::collections::HashMap<&'static (dyn games::Game + Send + Sync), Vec<u8>>,
    saves: std::collections::HashMap<
        &'static (dyn games::Game + Send + Sync),
        Vec<std::path::PathBuf>,
    >,
    last_rescan_time: std::time::Instant,
}

impl SavesListState {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(parking_lot::RwLock::new(SavesListStateInner {
                roms: std::collections::HashMap::new(),
                saves: std::collections::HashMap::new(),
                last_rescan_time: std::time::Instant::now(),
            })),
        }
    }

    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, SavesListStateInner> {
        self.inner.read()
    }

    pub fn rescan(&self, roms_path: &std::path::Path, saves_path: &std::path::Path) {
        if self.inner.is_locked_exclusive() {
            return;
        }

        let roms = games::scan_roms(&roms_path);
        let saves = games::scan_saves(&saves_path);
        let last_rescan_time = std::time::Instant::now();

        let mut inner = self.inner.write();
        if inner.last_rescan_time > last_rescan_time {
            return;
        }

        inner.roms = roms;
        inner.saves = saves;
        inner.last_rescan_time = last_rescan_time;
    }
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

        let mut saves_list = SavesListState::new();
        saves_list.rescan(&config.roms_path, &config.saves_path);

        Self {
            config,
            saves_list,
            main_screen: MainScreenState::Start(Start::new()),
            audio_binder,
            fps_counter,
            emu_tps_counter,
            steal_input: None,
            show_settings: None,
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
    save_select_window: save_select_window::SaveSelectWindow,
    settings_window: settings_window::SettingsWindow,
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
            save_select_window: save_select_window::SaveSelectWindow::new(),
            settings_window: settings_window::SettingsWindow::new(font_families.clone()),
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

                        if let MainScreenState::Session(session) = &state.main_screen {
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
        window: &glutin::window::Window,
        input_state: &input::State,
        state: &mut State,
    ) {
        if let MainScreenState::Session(session) = &state.main_screen {
            if session.completed() {
                state.main_screen = MainScreenState::Start(Start::new());
            }
        }

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

        self.draw_debug_overlay(ctx, handle.clone(), state);
        self.settings_window.show(
            ctx,
            &mut state.show_settings,
            &mut state.config,
            &mut state.steal_input,
        );
        self.show_steal_input_dialog(ctx, &state.config.language, &mut state.steal_input);

        match &mut state.main_screen {
            MainScreenState::Session(session) => {
                self.show_session(
                    ctx,
                    input_state,
                    &state.config.input_mapping,
                    session,
                    &state.config.video_filter,
                    state.config.max_scale,
                );
            }
            MainScreenState::Start(start) => {
                self.save_select_window.show(
                    ctx,
                    &mut start.show_save_select,
                    &state.config.language,
                    &state.config.saves_path,
                    state.saves_list.clone(),
                    state.audio_binder.clone(),
                    state.emu_tps_counter.clone(),
                );

                egui::TopBottomPanel::top("start-top-panel")
                    .frame(egui::Frame {
                        inner_margin: egui::style::Margin::symmetric(8.0, 2.0),
                        rounding: egui::Rounding::none(),
                        fill: ctx.style().visuals.window_fill(),
                        ..Default::default()
                    })
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .selectable_label(state.show_settings.is_some(), "⚙️")
                                        .on_hover_text_at_pointer(
                                            i18n::LOCALES
                                                .lookup(&state.config.language, "settings")
                                                .unwrap(),
                                        )
                                        .clicked()
                                    {
                                        state.show_settings = if state.show_settings.is_none() {
                                            Some(settings_window::State::new())
                                        } else {
                                            None
                                        };
                                    }
                                },
                            );
                        });
                    });
                egui::TopBottomPanel::bottom("start-bottom-panel")
                    .frame(egui::Frame {
                        inner_margin: egui::style::Margin::symmetric(8.0, 2.0),
                        rounding: egui::Rounding::none(),
                        fill: ctx.style().visuals.window_fill(),
                        ..Default::default()
                    })
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let submit = |start: &Start| {
                                        if !start.link_code.is_empty() {
                                            log::info!("{}", start.link_code);
                                            handle.block_on(async {
                                                *start.connection_state.lock().await =
                                                    Some(ConnectionState::Starting);
                                            });

                                            let connection_state = start.connection_state.clone();
                                            let matchmaking_addr =
                                                state.config.matchmaking_endpoint.clone();
                                            let link_code = start.link_code.clone();

                                            handle.spawn(async move {
                                                if let Err(e) = {
                                                    let connection_state = connection_state.clone();
                                                    move || async move {
                                                        log::info!("signaling...");
                                                        *connection_state.lock().await =
                                                            Some(ConnectionState::Signaling);
                                                        // TODO: Add a timeout.
                                                        let pending_conn = net::signaling::open(
                                                            &matchmaking_addr,
                                                            &link_code,
                                                        )
                                                        .await?;

                                                        log::info!("waiting...");
                                                        *connection_state.lock().await =
                                                            Some(ConnectionState::Waiting);

                                                        let (mut dc, peer_conn) =
                                                            pending_conn.connect().await?;
                                                        net::negotiate(&mut dc).await?;

                                                        log::info!("hello...");
                                                        *connection_state.lock().await = Some(
                                                            ConnectionState::Ready((dc, peer_conn)),
                                                        );

                                                        Ok(())
                                                    }
                                                }(
                                                )
                                                .await
                                                {
                                                    *connection_state.lock().await =
                                                        Some(ConnectionState::Failed(e))
                                                }
                                            });
                                        }
                                    };

                                    if ui
                                        .button(if start.link_code.is_empty() {
                                            format!(
                                                "▶️ {}",
                                                i18n::LOCALES
                                                    .lookup(&state.config.language, "start.play")
                                                    .unwrap()
                                            )
                                        } else {
                                            format!(
                                                "🥊 {}",
                                                i18n::LOCALES
                                                    .lookup(&state.config.language, "start.fight")
                                                    .unwrap()
                                            )
                                        })
                                        .clicked()
                                    {
                                        submit(start);
                                    }

                                    let input_resp = ui.add(
                                        egui::TextEdit::singleline(&mut start.link_code)
                                            .hint_text(
                                                i18n::LOCALES
                                                    .lookup(
                                                        &state.config.language,
                                                        "start.link-code",
                                                    )
                                                    .unwrap(),
                                            )
                                            .desired_width(f32::INFINITY),
                                    );
                                    start.link_code = start
                                        .link_code
                                        .to_lowercase()
                                        .chars()
                                        .filter(|c| {
                                            "abcdefghijklmnopqrstuvwxyz0123456789-"
                                                .chars()
                                                .any(|c2| c2 == *c)
                                        })
                                        .take(40)
                                        .collect::<String>()
                                        .trim_start_matches("-")
                                        .to_string();

                                    if let Some(last) = start.link_code.chars().last() {
                                        if last == '-' {
                                            start.link_code = start
                                                .link_code
                                                .chars()
                                                .rev()
                                                .skip_while(|c| *c == '-')
                                                .collect::<Vec<_>>()
                                                .into_iter()
                                                .rev()
                                                .collect::<String>()
                                                + "-";
                                        }
                                    }

                                    if input_resp.lost_focus()
                                        && ctx.input().key_pressed(egui::Key::Enter)
                                    {
                                        submit(start);
                                    }
                                },
                            );
                        });
                    });
                egui::CentralPanel::default().show(ctx, |ui| {});
            }
        }
    }
}
