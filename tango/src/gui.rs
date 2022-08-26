use crate::{audio, config, games, input, stats};
use std::str::FromStr;

const DISCORD_APP_ID: u64 = 974089681333534750;

mod debug_window;
mod main_view;
mod save_select_window;
mod session_view;
mod settings_window;
mod steal_input_window;

pub struct State {
    pub config: config::Config,
    pub steal_input: Option<steal_input_window::State>,
    saves_list: SavesListState,
    audio_binder: audio::LateBinder,
    fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    main_view: std::sync::Arc<parking_lot::Mutex<main_view::State>>,
    show_settings: Option<settings_window::State>,
    drpc: discord_rpc_client::Client,
}

#[derive(Clone)]
pub struct SavesListState {
    inner: std::sync::Arc<parking_lot::RwLock<SavesListStateInner>>,
}

pub struct SavesListStateInner {
    pub roms: std::collections::HashMap<&'static (dyn games::Game + Send + Sync), Vec<u8>>,
    pub saves: std::collections::HashMap<
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

        let saves_list = SavesListState::new();
        saves_list.rescan(&config.roms_path, &config.saves_path);

        Self {
            config,
            saves_list,
            main_view: std::sync::Arc::new(parking_lot::Mutex::new(main_view::State::Start(
                main_view::Start::new(),
            ))),
            audio_binder,
            fps_counter,
            emu_tps_counter,
            steal_input: None,
            show_settings: None,
            drpc,
        }
    }
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
    main_view: main_view::MainView,
    settings_window: settings_window::SettingsWindow,
    debug_window: debug_window::DebugWindow,
    steal_input_window: steal_input_window::StealInputWindow,
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
            main_view: main_view::MainView::new(),
            steal_input_window: steal_input_window::StealInputWindow::new(),
            debug_window: debug_window::DebugWindow::new(),
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

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        handle: tokio::runtime::Handle,
        window: &glutin::window::Window,
        input_state: &input::State,
        state: &mut State,
    ) {
        {
            let mut main_view = state.main_view.lock();
            if let main_view::State::Session(session) = &*main_view {
                if session.completed() {
                    *main_view = main_view::State::Start(main_view::Start::new());
                }
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

        self.debug_window.show(ctx, handle.clone(), state);
        self.settings_window.show(
            ctx,
            &mut state.show_settings,
            &mut state.config,
            &mut state.steal_input,
        );
        self.steal_input_window
            .show(ctx, &state.config.language, &mut state.steal_input);
        self.main_view
            .show(ctx, handle.clone(), window, input_state, state);
    }
}
