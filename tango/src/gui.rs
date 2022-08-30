use crate::{audio, config, game, input, patch, save, scanner, stats};
use std::str::FromStr;

const DISCORD_APP_ID: u64 = 974089681333534750;

mod debug_window;
mod escape_window;
mod main_view;
mod patches_window;
mod save_select_window;
mod session_view;
mod settings_window;
mod steal_input_window;

type ROMsScanner =
    scanner::Scanner<std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<u8>>>;
type SavesScanner = scanner::Scanner<
    std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<save::ScannedSave>>,
>;
type PatchesScanner = scanner::Scanner<std::collections::BTreeMap<String, patch::Patch>>;

pub struct State {
    pub config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    pub steal_input: Option<steal_input_window::State>,
    roms_scanner: ROMsScanner,
    saves_scanner: SavesScanner,
    patches_scanner: PatchesScanner,
    audio_binder: audio::LateBinder,
    fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    main_view: std::sync::Arc<parking_lot::Mutex<main_view::State>>,
    show_escape_window: Option<escape_window::State>,
    show_settings: Option<settings_window::State>,
    drpc: discord_rpc_client::Client,
}

impl State {
    pub fn new(
        config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
        audio_binder: audio::LateBinder,
        fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
        emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    ) -> Self {
        let mut drpc = discord_rpc_client::Client::new(DISCORD_APP_ID);
        drpc.start();

        let roms_scanner = scanner::Scanner::new();
        let saves_scanner = scanner::Scanner::new();
        let patches_scanner = scanner::Scanner::new();
        {
            let config = config.read().clone();
            let roms_path = config.roms_path();
            let saves_path = config.saves_path();
            let patches_path = config.patches_path();
            roms_scanner.rescan(move || game::scan_roms(&roms_path));
            saves_scanner.rescan(move || save::scan_saves(&saves_path));
            patches_scanner.rescan(move || patch::scan(&patches_path).unwrap_or_default());
        }

        Self {
            config,
            roms_scanner,
            saves_scanner,
            patches_scanner,
            main_view: std::sync::Arc::new(parking_lot::Mutex::new(main_view::State::new())),
            audio_binder,
            fps_counter,
            emu_tps_counter,
            steal_input: None,
            show_settings: None,
            show_escape_window: None,
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

impl FontFamilies {
    pub fn for_language(&self, lang: &unic_langid::LanguageIdentifier) -> egui::FontFamily {
        let mut lang = lang.clone();
        lang.maximize();
        match lang.script {
            Some(s) if s == unic_langid::subtags::Script::from_str("Jpan").unwrap() => {
                self.jpan.clone()
            }
            Some(s) if s == unic_langid::subtags::Script::from_str("Hans").unwrap() => {
                self.hans.clone()
            }
            Some(s) if s == unic_langid::subtags::Script::from_str("Hant").unwrap() => {
                self.hant.clone()
            }
            _ => self.latn.clone(),
        }
    }
}

pub struct Gui {
    main_view: main_view::MainView,
    settings_window: settings_window::SettingsWindow,
    debug_window: debug_window::DebugWindow,
    steal_input_window: steal_input_window::StealInputWindow,
    escape_window: escape_window::EscapeWindow,
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

        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (
                egui::TextStyle::Heading,
                egui::FontId::new(22.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Body,
                egui::FontId::new(18.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Monospace,
                egui::FontId::new(18.0, egui::FontFamily::Monospace),
            ),
            (
                egui::TextStyle::Button,
                egui::FontId::new(18.0, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Small,
                egui::FontId::new(16.0, egui::FontFamily::Proportional),
            ),
        ]
        .into();
        ctx.set_style(style);

        Self {
            main_view: main_view::MainView::new(),
            steal_input_window: steal_input_window::StealInputWindow::new(),
            debug_window: debug_window::DebugWindow::new(),
            settings_window: settings_window::SettingsWindow::new(font_families.clone()),
            escape_window: escape_window::EscapeWindow::new(),
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
        config: &mut config::Config,
        handle: tokio::runtime::Handle,
        window: &glutin::window::Window,
        input_state: &input::State,
        state: &mut State,
    ) {
        {
            let mut main_view = state.main_view.lock();
            if let Some(session) = main_view.session.as_ref() {
                if session.completed() {
                    main_view.session = None;
                }
            }
        }

        if self.current_language.as_ref() != Some(&config.language) {
            let mut language = config.language.clone();
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
            self.current_language = Some(config.language.clone());
            log::info!(
                "language was changed to {}",
                self.current_language.as_ref().unwrap()
            );
        }

        ctx.set_visuals(match config.theme {
            config::Theme::System => match dark_light::detect() {
                dark_light::Mode::Light => self.themes.light.clone(),
                dark_light::Mode::Dark => self.themes.dark.clone(),
            },
            config::Theme::Light => self.themes.light.clone(),
            config::Theme::Dark => self.themes.dark.clone(),
        });

        self.debug_window.show(ctx, config, handle.clone(), state);
        self.settings_window.show(
            ctx,
            &mut state.show_settings,
            config,
            state.roms_scanner.clone(),
            state.saves_scanner.clone(),
            state.patches_scanner.clone(),
            window,
            &mut state.steal_input,
        );
        self.steal_input_window
            .show(ctx, &config.language, &mut state.steal_input);
        self.escape_window.show(
            ctx,
            state.main_view.clone(),
            &mut state.show_escape_window,
            &config.language,
            &mut state.show_settings,
        );
        self.main_view.show(
            ctx,
            &self.font_families,
            config,
            handle.clone(),
            window,
            input_state,
            state,
        );
    }
}
