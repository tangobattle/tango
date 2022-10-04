use fluent_templates::Loader;

use crate::{audio, config, discord, game, i18n, input, patch, rom, save, session, stats, updater};
use std::str::FromStr;

mod debug_window;
mod escape_window;
mod language_select;
mod main_view;
mod patches_pane;
mod play_pane;
mod replay_dump_windows;
mod replays_pane;
mod save_select_view;
mod save_view;
mod session_view;
mod settings_window;
mod steal_input_window;
mod updater_window;
mod warning;
mod welcome;

pub struct Selection {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub assets: Option<Box<dyn rom::Assets + Send + Sync>>,
    pub save: save::ScannedSave,
    pub rom: Vec<u8>,
    pub patch: Option<(String, semver::Version, patch::Version)>,
    pub save_view_state: save_view::State,
}

impl Selection {
    pub fn new(
        game: &'static (dyn game::Game + Send + Sync),
        save: save::ScannedSave,
        patch: Option<(String, semver::Version, patch::Version)>,
        rom: Vec<u8>,
    ) -> Self {
        let assets = game
            .load_rom_assets(
                &rom,
                save.save.as_raw_wram(),
                &patch
                    .as_ref()
                    .map(|(_, _, metadata)| metadata.rom_overrides.clone())
                    .unwrap_or_else(|| Default::default()),
            )
            .ok();
        Self {
            game,
            assets,
            save,
            patch,
            rom,
            save_view_state: save_view::State::new(),
        }
    }

    pub fn reload_save(&mut self) -> anyhow::Result<()> {
        let raw = std::fs::read(&self.save.path)?;
        self.save.save = self.game.parse_save(&raw)?;
        self.save_view_state = save_view::State::new();
        Ok(())
    }
}

pub struct State {
    config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    pub session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    selection: Option<Selection>,
    pub steal_input: Option<steal_input_window::State>,
    roms_scanner: rom::Scanner,
    saves_scanner: save::Scanner,
    patches_scanner: patch::Scanner,
    pub last_mouse_motion_time: Option<std::time::Instant>,
    audio_binder: audio::LateBinder,
    fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    main_view: main_view::State,
    show_escape_window: Option<escape_window::State>,
    show_settings: Option<settings_window::State>,
    replay_dump_windows: replay_dump_windows::State,
    clipboard: arboard::Clipboard,
    font_data: std::collections::BTreeMap<String, egui::FontData>,
    font_families: FontFamilies,
    themes: Themes,
    current_language: Option<unic_langid::LanguageIdentifier>,
    session_view: Option<session_view::State>,
    welcome: Option<welcome::State>,
    discord_client: discord::Client,
}

impl State {
    pub fn new(
        ctx: &egui::Context,
        show_updater: bool,
        config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
        discord_client: discord::Client,
        audio_binder: audio::LateBinder,
        fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
        emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
        roms_scanner: rom::Scanner,
        saves_scanner: save::Scanner,
        patches_scanner: patch::Scanner,
    ) -> Self {
        let font_families = FontFamilies {
            latn: FontFamily {
                egui: egui::FontFamily::Name("Latn".into()),
                raw: include_bytes!("fonts/NotoSans-Regular.ttf"),
            },
            jpan: FontFamily {
                egui: egui::FontFamily::Name("Jpan".into()),
                raw: include_bytes!("fonts/NotoSansJP-Regular.otf"),
            },
            hans: FontFamily {
                egui: egui::FontFamily::Name("Hans".into()),
                raw: include_bytes!("fonts/NotoSansSC-Regular.otf"),
            },
            hant: FontFamily {
                egui: egui::FontFamily::Name("Hant".into()),
                raw: include_bytes!("fonts/NotoSansTC-Regular.otf"),
            },
        };

        ctx.set_fonts(egui::FontDefinitions {
            font_data: std::collections::BTreeMap::default(),
            families: std::collections::BTreeMap::from([
                (egui::FontFamily::Proportional, vec![]),
                (egui::FontFamily::Monospace, vec![]),
                (font_families.latn.egui.clone(), vec![]),
                (font_families.jpan.egui.clone(), vec![]),
                (font_families.hans.egui.clone(), vec![]),
                (font_families.hant.egui.clone(), vec![]),
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
            config,
            session: std::sync::Arc::new(parking_lot::Mutex::new(None)),
            selection: None,
            last_mouse_motion_time: None,
            roms_scanner,
            saves_scanner,
            patches_scanner,
            main_view: main_view::State::new(show_updater),
            audio_binder,
            fps_counter,
            emu_tps_counter,
            steal_input: None,
            show_settings: None,
            show_escape_window: None,
            session_view: None,
            welcome: None,
            replay_dump_windows: replay_dump_windows::State::new(),
            clipboard: arboard::Clipboard::new().unwrap(),
            font_data: std::collections::BTreeMap::from([
                (
                    "NotoSans-Regular".to_string(),
                    egui::FontData::from_static(font_families.latn.raw),
                ),
                (
                    "NotoSansJP-Regular".to_string(),
                    egui::FontData::from_static(font_families.jpan.raw),
                ),
                (
                    "NotoSansSC-Regular".to_string(),
                    egui::FontData::from_static(font_families.hans.raw),
                ),
                (
                    "NotoSansTC-Regular".to_string(),
                    egui::FontData::from_static(font_families.hant.raw),
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
            discord_client,
        }
    }
}

struct Themes {
    light: egui::style::Visuals,
    dark: egui::style::Visuals,
}

pub struct FontFamily {
    pub egui: egui::FontFamily,
    pub raw: &'static [u8],
}

pub struct FontFamilies {
    latn: FontFamily,
    jpan: FontFamily,
    hans: FontFamily,
    hant: FontFamily,
}

impl FontFamilies {
    pub fn for_language(&self, lang: &unic_langid::LanguageIdentifier) -> egui::FontFamily {
        let mut lang = lang.clone();
        lang.maximize();
        match lang.script {
            Some(s) if s == unic_langid::subtags::Script::from_str("Jpan").unwrap() => self.jpan.egui.clone(),
            Some(s) if s == unic_langid::subtags::Script::from_str("Hans").unwrap() => self.hans.egui.clone(),
            Some(s) if s == unic_langid::subtags::Script::from_str("Hant").unwrap() => self.hant.egui.clone(),
            _ => self.latn.egui.clone(),
        }
    }

    pub fn raw_for_language(&self, lang: &unic_langid::LanguageIdentifier) -> &[u8] {
        let mut lang = lang.clone();
        lang.maximize();
        match lang.script {
            Some(s) if s == unic_langid::subtags::Script::from_str("Jpan").unwrap() => self.jpan.raw,
            Some(s) if s == unic_langid::subtags::Script::from_str("Hans").unwrap() => self.hans.raw,
            Some(s) if s == unic_langid::subtags::Script::from_str("Hant").unwrap() => self.hant.raw,
            _ => self.latn.raw,
        }
    }
}

pub fn show(
    ctx: &egui::Context,
    config: &mut config::Config,
    window: &winit::window::Window,
    input_state: &input::State,
    state: &mut State,
    updater: &updater::Updater,
) {
    {
        let mut session = state.session.lock();
        if let Some(s) = session.as_ref() {
            if s.completed() {
                *session = None;
            }
        }
    }

    if state.current_language.as_ref() != Some(&config.language) {
        let mut language = config.language.clone();
        language.maximize();

        let primary_font = match language.script {
            Some(s) if s == unic_langid::subtags::Script::from_str("Jpan").unwrap() => "NotoSansJP-Regular",
            Some(s) if s == unic_langid::subtags::Script::from_str("Hans").unwrap() => "NotoSansSC-Regular",
            Some(s) if s == unic_langid::subtags::Script::from_str("Hant").unwrap() => "NotoSansTC-Regular",
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
            font_data: state.font_data.clone(),
            families: std::collections::BTreeMap::from([
                (egui::FontFamily::Proportional, proportional),
                (egui::FontFamily::Monospace, monospace),
                (
                    state.font_families.jpan.egui.clone(),
                    vec!["NotoSansJP-Regular".to_string()],
                ),
                (
                    state.font_families.hans.egui.clone(),
                    vec!["NotoSansSC-Regular".to_string()],
                ),
                (
                    state.font_families.hant.egui.clone(),
                    vec!["NotoSansTC-Regular".to_string()],
                ),
                (
                    state.font_families.latn.egui.clone(),
                    vec!["NotoSans-Regular".to_string()],
                ),
            ]),
        });
        state.current_language = Some(config.language.clone());
        log::info!("language was changed to {}", state.current_language.as_ref().unwrap());
    }

    ctx.set_visuals(match config.theme {
        config::Theme::System => match dark_light::detect() {
            dark_light::Mode::Light => state.themes.light.clone(),
            dark_light::Mode::Dark => state.themes.dark.clone(),
        },
        config::Theme::Light => state.themes.light.clone(),
        config::Theme::Dark => state.themes.dark.clone(),
    });

    if config.nickname.is_none() {
        welcome::show(
            ctx,
            &state.font_families,
            config,
            state.roms_scanner.clone(),
            state.saves_scanner.clone(),
            state.welcome.get_or_insert_with(|| welcome::State::new()),
        );
        return;
    } else {
        state.welcome = None;
    }

    settings_window::show(
        ctx,
        &mut state.show_settings,
        &state.font_families,
        config,
        state.roms_scanner.clone(),
        state.saves_scanner.clone(),
        state.patches_scanner.clone(),
        window,
        &mut state.steal_input,
    );
    steal_input_window::show(ctx, &config.language, &mut state.steal_input);
    escape_window::show(
        ctx,
        state.session.clone(),
        &mut state.selection,
        &mut state.show_escape_window,
        &config.language,
        &mut state.show_settings,
    );
    replay_dump_windows::show(
        ctx,
        &mut state.replay_dump_windows,
        &config.language,
        &config.replays_path(),
    );

    if let Some(session) = state.session.lock().as_ref() {
        window.set_title(&i18n::LOCALES.lookup(&config.language, "window-title.running").unwrap());
        session_view::show(
            ctx,
            &config.language,
            &mut state.clipboard,
            &state.font_families,
            input_state,
            &config.input_mapping,
            session,
            &config.video_filter,
            config.integer_scaling,
            config.max_scale,
            config.speed_change_percent as f32 / 100.0,
            config.show_own_setup,
            &config.crashstates_path(),
            &state.last_mouse_motion_time,
            &mut state.show_escape_window,
            state.fps_counter.clone(),
            state.emu_tps_counter.clone(),
            config.show_debug,
            config.always_show_status_bar,
            state.session_view.get_or_insert_with(|| session_view::State::new()),
            &mut state.discord_client,
        );
    } else {
        state.session_view = None;
        window.set_title(&i18n::LOCALES.lookup(&config.language, "window-title").unwrap());
        main_view::show(
            ctx,
            &state.font_families,
            config,
            state.config.clone(),
            window,
            &mut state.show_settings,
            &mut state.replay_dump_windows,
            &mut state.clipboard,
            state.audio_binder.clone(),
            state.roms_scanner.clone(),
            state.saves_scanner.clone(),
            state.patches_scanner.clone(),
            state.emu_tps_counter.clone(),
            state.session.clone(),
            &mut state.selection,
            &mut state.main_view,
            &mut state.discord_client,
            updater,
        );
    }
}
