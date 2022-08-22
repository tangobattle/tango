use std::str::FromStr;

use fluent_templates::Loader;

use crate::{config, game, i18n, input, session, video};

struct VBuf {
    buf: Vec<u8>,
    texture: egui::TextureHandle,
}

pub struct FontFamilies {
    latn: egui::FontFamily,
    jpan: egui::FontFamily,
    hans: egui::FontFamily,
    hant: egui::FontFamily,
}

pub struct Icons {
    pub sports_esports: egui_extras::RetainedImage,
    pub keyboard: egui_extras::RetainedImage,
}

pub struct Gui {
    vbuf: Option<VBuf>,
    icons: Icons,
    font_data: std::collections::BTreeMap<String, egui::FontData>,
    font_families: FontFamilies,
    current_language: Option<unic_langid::LanguageIdentifier>,
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
            font_families: FontFamilies {
                latn: egui::FontFamily::Name("Latn".into()),
                jpan: egui::FontFamily::Name("Jpan".into()),
                hans: egui::FontFamily::Name("Hans".into()),
                hant: egui::FontFamily::Name("Hant".into()),
            },
            current_language: None,
        }
    }

    fn draw_settings_window(
        &mut self,
        ctx: &egui::Context,
        selected_tab: &mut game::SettingsTab,
        config: &mut config::Config,
        steal_input: &mut game::StealInputState,
    ) {
        egui::Window::new(i18n::LOCALES.lookup(&config.language, "settings").unwrap())
            .id(egui::Id::new("settings-window"))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            selected_tab,
                            game::SettingsTab::General,
                            i18n::LOCALES
                                .lookup(&config.language, "settings.general")
                                .unwrap(),
                        );
                        ui.selectable_value(
                            selected_tab,
                            game::SettingsTab::InputMapping,
                            i18n::LOCALES
                                .lookup(&config.language, "settings.input-mapping")
                                .unwrap(),
                        );
                    });

                    match selected_tab {
                        game::SettingsTab::General => self.draw_settings_general_tab(ui, config),
                        game::SettingsTab::InputMapping => self.draw_settings_input_mapping_tab(
                            ui,
                            &config.language,
                            &mut config.input_mapping,
                            steal_input,
                        ),
                    }
                });
            });
    }

    fn draw_settings_general_tab(&mut self, ui: &mut egui::Ui, config: &mut config::Config) {
        egui::Grid::new("settings-window-general-grid").show(ui, |ui| {
            {
                ui.label(
                    egui::RichText::new(
                        i18n::LOCALES
                            .lookup(&config.language, "settings-theme")
                            .unwrap(),
                    )
                    .strong(),
                );

                let light_label = i18n::LOCALES
                    .lookup(&config.language, "settings-theme.light")
                    .unwrap();
                let dark_label = i18n::LOCALES
                    .lookup(&config.language, "settings-theme.dark")
                    .unwrap();

                egui::ComboBox::from_id_source("settings-window-general-theme")
                    .selected_text(match config.theme {
                        config::Theme::Light => &light_label,
                        config::Theme::Dark => &dark_label,
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.theme, config::Theme::Light, &light_label);
                        ui.selectable_value(&mut config.theme, config::Theme::Dark, &dark_label);
                    });
                ui.end_row();
            }

            {
                ui.label(
                    egui::RichText::new(
                        i18n::LOCALES
                            .lookup(&config.language, "settings-language")
                            .unwrap(),
                    )
                    .strong(),
                );

                let en_label =
                    egui::RichText::new("English").family(self.font_families.latn.clone());
                let ja_label =
                    egui::RichText::new("日本語").family(self.font_families.jpan.clone());
                let zh_hans_label =
                    egui::RichText::new("简体中文").family(self.font_families.hans.clone());
                let zh_hant_label =
                    egui::RichText::new("繁體中文").family(self.font_families.hant.clone());

                egui::ComboBox::from_id_source("settings-window-general-language")
                    .selected_text(match &config.language {
                        lang if lang.matches(&unic_langid::langid!("en"), false, true) => {
                            en_label.clone()
                        }
                        lang if lang.matches(&unic_langid::langid!("ja"), false, true) => {
                            ja_label.clone()
                        }
                        lang if lang.matches(&unic_langid::langid!("zh-Hans"), false, true) => {
                            zh_hans_label.clone()
                        }
                        lang if lang.matches(&unic_langid::langid!("zh-Hant"), false, true) => {
                            zh_hant_label.clone()
                        }
                        _ => egui::RichText::new(""),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut config.language,
                            unic_langid::langid!("en"),
                            en_label.clone(),
                        );
                        ui.selectable_value(
                            &mut config.language,
                            unic_langid::langid!("ja"),
                            ja_label.clone(),
                        );
                        ui.selectable_value(
                            &mut config.language,
                            unic_langid::langid!("zh-Hans"),
                            zh_hans_label.clone(),
                        );
                        ui.selectable_value(
                            &mut config.language,
                            unic_langid::langid!("zh-Hant"),
                            zh_hant_label.clone(),
                        );
                    });
                ui.end_row();
            }
        });
    }

    fn draw_settings_input_mapping_tab(
        &mut self,
        ui: &mut egui::Ui,
        lang: &unic_langid::LanguageIdentifier,
        input_mapping: &mut input::Mapping,
        steal_input: &mut game::StealInputState,
    ) {
        egui::Grid::new("settings-window-input-mapping-grid").show(ui, |ui| {
            let mut add_row = |label_text_id,
                               get_mapping: fn(
                &mut input::Mapping,
            ) -> &mut Vec<input::PhysicalInput>| {
                ui.label(
                    egui::RichText::new(i18n::LOCALES.lookup(lang, label_text_id).unwrap())
                        .strong(),
                );
                ui.horizontal(|ui| {
                    let mapping = get_mapping(input_mapping);
                    for (i, c) in mapping.clone().iter().enumerate() {
                        ui.group(|ui| {
                            ui.add(
                                egui::Image::new(
                                    match c {
                                        input::PhysicalInput::Key(_) => &self.icons.keyboard,
                                        input::PhysicalInput::Button(_)
                                        | input::PhysicalInput::Axis { .. } => {
                                            &self.icons.sports_esports
                                        }
                                    }
                                    .texture_id(ui.ctx()),
                                    egui::Vec2::new(
                                        ui.text_style_height(&egui::TextStyle::Body),
                                        ui.text_style_height(&egui::TextStyle::Body),
                                    ),
                                )
                                .tint(ui.style().visuals.widgets.noninteractive.fg_stroke.color),
                            );
                            ui.label(match c {
                                input::PhysicalInput::Key(key) => {
                                    let raw = serde_plain::to_string(key).unwrap();
                                    i18n::LOCALES
                                        .lookup(lang, &format!("physical-input-keys.{}", raw))
                                        .unwrap_or(raw)
                                }
                                input::PhysicalInput::Button(button) => {
                                    let raw = button.string();
                                    i18n::LOCALES
                                        .lookup(lang, &format!("physical-input-buttons.{}", raw))
                                        .unwrap_or(raw)
                                }
                                input::PhysicalInput::Axis { axis, direction } => {
                                    let raw = format!(
                                        "{}{}",
                                        axis.string(),
                                        match direction {
                                            input::AxisDirection::Positive => "plus",
                                            input::AxisDirection::Negative => "minus",
                                        }
                                    );
                                    i18n::LOCALES
                                        .lookup(lang, &format!("physical-input-axes.{}", raw))
                                        .unwrap_or(raw)
                                }
                            });
                            if ui.add(egui::Button::new("×").small()).clicked() {
                                mapping.remove(i);
                            }
                        });
                    }
                    if ui.add(egui::Button::new("+")).clicked() {
                        *steal_input = game::StealInputState::Stealing {
                            callback: {
                                let get_mapping = get_mapping.clone();
                                Box::new(move |phy, input_mapping| {
                                    let mapping = get_mapping(input_mapping);
                                    mapping.push(phy);
                                    mapping.sort_by_key(|c| match c {
                                        input::PhysicalInput::Key(key) => (0, *key as usize, 0),
                                        input::PhysicalInput::Button(button) => {
                                            (1, *button as usize, 0)
                                        }
                                        input::PhysicalInput::Axis { axis, direction } => {
                                            (2, *axis as usize, *direction as usize)
                                        }
                                    });
                                    mapping.dedup();
                                })
                            },
                            userdata: Box::new(label_text_id),
                        };
                    }
                });
                ui.end_row();
            };

            add_row("input-button.left", |input_mapping| &mut input_mapping.left);
            add_row("input-button.right", |input_mapping| {
                &mut input_mapping.right
            });
            add_row("input-button.up", |input_mapping| &mut input_mapping.up);
            add_row("input-button.down", |input_mapping| &mut input_mapping.down);
            add_row("input-button.a", |input_mapping| &mut input_mapping.a);
            add_row("input-button.b", |input_mapping| &mut input_mapping.b);
            add_row("input-button.l", |input_mapping| &mut input_mapping.l);
            add_row("input-button.r", |input_mapping| &mut input_mapping.r);
            add_row("input-button.start", |input_mapping| {
                &mut input_mapping.start
            });
            add_row("input-button.select", |input_mapping| {
                &mut input_mapping.select
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
                        let tps_adjustment = if let session::Mode::PvP(match_) = session.mode() {
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

    fn draw_emulator(&mut self, ui: &mut egui::Ui, session: &session::Session, video_filter: &str) {
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
        video_filter: &str,
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
        if let session::Mode::PvP(match_) = session.mode() {
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
        if self.current_language.as_ref() != Some(&state.config.language) {
            let mut language = state.config.language.clone();
            language.maximize();
            log::info!("language is changing to {}", language);

            ctx.set_fonts(egui::FontDefinitions {
                font_data: self.font_data.clone(),
                families: std::collections::BTreeMap::from([
                    (
                        egui::FontFamily::Proportional,
                        vec![
                            match language.script {
                                Some(s)
                                    if s == unic_langid::subtags::Script::from_str("Jpan")
                                        .unwrap() =>
                                {
                                    "NotoSansJP-Regular"
                                }
                                Some(s)
                                    if s == unic_langid::subtags::Script::from_str("Hans")
                                        .unwrap() =>
                                {
                                    "NotoSansSC-Regular"
                                }
                                Some(s)
                                    if s == unic_langid::subtags::Script::from_str("Hant")
                                        .unwrap() =>
                                {
                                    "NotoSansTC-Regular"
                                }
                                _ => "NotoSans-Regular",
                            }
                            .to_string(),
                            "NotoSans-Regular".to_string(),
                            "NotoSansJP-Regular".to_string(),
                            "NotoSansSC-Regular".to_string(),
                            "NotoSansTC-Regular".to_string(),
                            "NotoEmoji-Regular".to_string(),
                        ],
                    ),
                    (
                        egui::FontFamily::Monospace,
                        vec![
                            "NotoSansMono-Regular".to_string(),
                            "NotoEmoji-Regular".to_string(),
                        ],
                    ),
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
        }

        ctx.set_visuals(match state.config.theme {
            config::Theme::Light => egui::style::Visuals::light(),
            config::Theme::Dark => egui::style::Visuals::dark(),
        });

        if let Some(session) = &state.session {
            self.draw_session(
                ctx,
                handle.clone(),
                window,
                input_state,
                &state.config.input_mapping,
                session,
                &state.title_prefix,
                &state.config.video_filter,
            );
        }

        if input_state.is_key_pressed(glutin::event::VirtualKeyCode::Grave) {
            state.show_debug = !state.show_debug;
        }
        self.draw_debug_window(ctx, handle.clone(), state);
        self.draw_settings_window(
            ctx,
            &mut state.selected_settings_tab,
            &mut state.config,
            &mut state.steal_input,
        );

        let mut steal_input_open = match state.steal_input {
            game::StealInputState::Idle => false,
            game::StealInputState::Stealing { .. } => true,
        };
        if let Some(inner_response) = egui::Window::new("")
            .id(egui::Id::new("input-capture-window"))
            .open(&mut steal_input_open)
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .inner_margin(egui::style::Margin::symmetric(32.0, 16.0))
                    .show(ui, |ui| {
                        let userdata = if let game::StealInputState::Stealing { userdata, .. } =
                            &state.steal_input
                        {
                            userdata
                        } else {
                            unreachable!();
                        };

                        ui.label(
                            egui::RichText::new(
                                i18n::LOCALES
                                    .lookup_with_args(
                                        &state.config.language,
                                        "input-mapping.prompt",
                                        &std::collections::HashMap::from([(
                                            "key",
                                            i18n::LOCALES
                                                .lookup(
                                                    &state.config.language,
                                                    userdata.downcast_ref::<&str>().unwrap(),
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
            })
        {
            ctx.move_to_top(inner_response.response.layer_id);
        }
        if !steal_input_open {
            state.steal_input = game::StealInputState::Idle;
        }
    }
}
