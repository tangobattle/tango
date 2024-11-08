use fluent_templates::Loader;

use crate::{audio, config, discord, game, i18n, input, patch, rom, save, session, stats, updater};
use std::str::FromStr;

mod debug_window;
mod escape_window;
mod language_select;
mod main_view;
mod memoize;
mod patches_pane;
mod play_pane;
mod replay_dump_window;
mod replays_pane;
mod save_select_view;
mod save_view;
mod session_view;
mod settings_window;
mod steal_input_window;
mod ui_windows;
mod updater_window;
mod warning;
mod welcome;

pub struct Selection {
    pub game: &'static (dyn game::Game + Send + Sync),
    pub assets: Option<Box<dyn tango_dataview::rom::Assets + Send + Sync>>,
    pub save: save::ScannedSave,
    pub rom: Vec<u8>,
    pub patch: Option<(String, semver::Version, std::sync::Arc<patch::Version>)>,
    pub save_view_state: save_view::State,
}

impl Selection {
    pub fn new(
        game: &'static (dyn game::Game + Send + Sync),
        save: save::ScannedSave,
        patch: Option<(String, semver::Version, std::sync::Arc<patch::Version>)>,
        rom: Vec<u8>,
    ) -> Self {
        let assets = game
            .load_rom_assets(
                &rom,
                &save.save.as_raw_wram(),
                &patch
                    .as_ref()
                    .map(|(_, _, metadata)| metadata.rom_overrides.clone())
                    .unwrap_or_default(),
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

    pub fn reload_save(&mut self, saves_scanner: &mut save::Scanner) -> anyhow::Result<()> {
        let raw = std::fs::read(&self.save.path)?;
        self.save.save = self.game.parse_save(&raw)?;
        self.save_view_state = save_view::State::new();

        saves_scanner.modify(|saves_map| {
            let Some(saves) = saves_map.get_mut(&self.game) else {
                return;
            };
            let Some(save) = saves.iter_mut().find(|save| save.path == self.save.path) else {
                return;
            };
            *save = self.save.clone();
        });

        Ok(())
    }
}

pub struct SharedRootState {
    pub config: std::sync::Arc<parking_lot::RwLock<config::Config>>,
    pub session: std::sync::Arc<parking_lot::Mutex<Option<session::Session>>>,
    pub clipboard: arboard::Clipboard,
    pub roms_scanner: rom::Scanner,
    pub saves_scanner: save::Scanner,
    pub patches_scanner: patch::Scanner,
    pub audio_binder: audio::LateBinder,
    pub fps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    pub emu_tps_counter: std::sync::Arc<parking_lot::Mutex<stats::Counter>>,
    pub discord_client: discord::Client,
    pub font_families: FontFamilies,
    pub ui_windows: ui_windows::UiWindows,
    pub selection: Option<Selection>,
}

pub struct State {
    pub shared: SharedRootState,
    pub steal_input: Option<steal_input_window::State>,
    pub last_mouse_motion_time: Option<std::time::Instant>,
    main_view: main_view::State,
    show_escape_window: Option<escape_window::State>,
    show_settings: Option<settings_window::State>,
    font_data: std::collections::BTreeMap<String, egui::FontData>,
    themes: Themes,
    current_language: Option<unic_langid::LanguageIdentifier>,
    session_view: Option<session_view::State>,
    welcome: Option<welcome::State>,
    init_link_code: Option<String>,
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
        init_link_code: Option<String>,
    ) -> Result<Self, anyhow::Error> {
        let font_families = FontFamilies {
            latn: FontFamily::new("Latn", include_bytes!("fonts/NotoSans-Regular.ttf")),
            jpan: FontFamily::new("Jpan", include_bytes!("fonts/NotoSansJP-Regular.otf")),
            hans: FontFamily::new("Hans", include_bytes!("fonts/NotoSansSC-Regular.otf")),
            hant: FontFamily::new("Hant", include_bytes!("fonts/NotoSansTC-Regular.otf")),
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

        ctx.style_mut(|style| {
            style.spacing.scroll = egui::style::ScrollStyle::solid();
            // animation_time > 0 causes panics as egui requires us to keep data around for closing animations
            // to see what i mean, open the settings window and close it with this set to anything other than 0
            // disabling the fade_out animation on specific windows does not appear to stop egui from attempting to rerender old data
            style.animation_time = 0.0;
        });

        // load previous selection
        let working_selection = crate::gui::save_select_view::Selection::resolve_from_config(
            roms_scanner.clone(),
            saves_scanner.clone(),
            patches_scanner.clone(),
            &config.read(),
        );

        let committed_selection = working_selection
            .as_ref()
            .and_then(|selection| selection.commit(roms_scanner.clone(), saves_scanner.clone(), &config.read()));

        let main_view = main_view::State::new(working_selection, show_updater);

        let font_data = std::collections::BTreeMap::from([
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
        ]);

        Ok(Self {
            shared: SharedRootState {
                config,
                session: std::sync::Arc::new(parking_lot::Mutex::new(None)),
                clipboard: arboard::Clipboard::new().unwrap(),
                roms_scanner,
                saves_scanner,
                patches_scanner,
                audio_binder,
                fps_counter,
                emu_tps_counter,
                font_families,
                ui_windows: Default::default(),
                discord_client,
                selection: committed_selection,
            },
            last_mouse_motion_time: None,
            main_view,
            steal_input: None,
            show_settings: None,
            show_escape_window: None,
            session_view: None,
            welcome: None,
            font_data,
            themes: Themes {
                light: {
                    let mut visuals = egui::style::Visuals::light();
                    visuals.selection.bg_fill = egui::Color32::from_rgb(0x4c, 0xaf, 0x50);
                    visuals.selection.stroke.color = egui::Color32::BLACK;
                    visuals.faint_bg_color = egui::Color32::LIGHT_GRAY;
                    visuals
                },
                dark: {
                    let mut visuals = egui::style::Visuals::dark();
                    visuals.selection.bg_fill = egui::Color32::from_rgb(0x4c, 0xaf, 0x50);
                    visuals.selection.stroke.color = egui::Color32::WHITE;
                    visuals.faint_bg_color = egui::Color32::from_gray(14);
                    visuals.extreme_bg_color = egui::Color32::BLACK;
                    visuals
                },
            },
            current_language: None,
            init_link_code,
        })
    }
}

struct Themes {
    light: egui::style::Visuals,
    dark: egui::style::Visuals,
}

pub struct FontFamily {
    egui: egui::FontFamily,
    raw: &'static [u8],
    fontdue: fontdue::Font,
}

impl FontFamily {
    fn new(name: &str, raw: &'static [u8]) -> Self {
        Self {
            egui: egui::FontFamily::Name(name.into()),
            raw,
            fontdue: fontdue::Font::from_bytes(raw, fontdue::FontSettings::default()).unwrap(),
        }
    }
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

    pub fn fontdue_for_language(&self, lang: &unic_langid::LanguageIdentifier) -> &fontdue::Font {
        let mut lang = lang.clone();
        lang.maximize();
        match lang.script {
            Some(s) if s == unic_langid::subtags::Script::from_str("Jpan").unwrap() => &self.jpan.fontdue,
            Some(s) if s == unic_langid::subtags::Script::from_str("Hans").unwrap() => &self.hans.fontdue,
            Some(s) if s == unic_langid::subtags::Script::from_str("Hant").unwrap() => &self.hant.fontdue,
            _ => &self.latn.fontdue,
        }
    }

    pub fn all_fontdue(&self) -> impl Iterator<Item = &fontdue::Font> {
        [
            &self.latn.fontdue,
            &self.jpan.fontdue,
            &self.hans.fontdue,
            &self.hant.fontdue,
        ]
        .into_iter()
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
        let mut session = state.shared.session.lock();
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

        let font_families = &state.shared.font_families;

        ctx.set_fonts(egui::FontDefinitions {
            font_data: state.font_data.clone(),
            families: std::collections::BTreeMap::from([
                (egui::FontFamily::Proportional, proportional),
                (egui::FontFamily::Monospace, monospace),
                (font_families.jpan.egui.clone(), vec!["NotoSansJP-Regular".to_string()]),
                (font_families.hans.egui.clone(), vec!["NotoSansSC-Regular".to_string()]),
                (font_families.hant.egui.clone(), vec!["NotoSansTC-Regular".to_string()]),
                (font_families.latn.egui.clone(), vec!["NotoSans-Regular".to_string()]),
            ]),
        });
        state.current_language = Some(config.language.clone());
        log::info!("language was changed to {}", state.current_language.as_ref().unwrap());
    }

    let is_dark = match config.theme {
        config::Theme::System => match dark_light::detect() {
            dark_light::Mode::Light => false,
            dark_light::Mode::Default | dark_light::Mode::Dark => true,
        },
        config::Theme::Light => false,
        config::Theme::Dark => true,
    };

    ctx.set_visuals(if is_dark {
        state.themes.dark.clone()
    } else {
        state.themes.light.clone()
    });

    if config.nickname.is_none() {
        welcome::show(
            ctx,
            &state.shared,
            config,
            state.welcome.get_or_insert_with(welcome::State::new),
        );
        return;
    } else {
        state.welcome = None;
    }

    settings_window::show(
        ctx,
        &mut state.show_settings,
        &state.shared,
        config,
        window,
        &mut state.steal_input,
    );
    steal_input_window::show(ctx, &config.language, &mut state.steal_input);
    escape_window::show(
        ctx,
        &mut state.shared,
        &mut state.show_escape_window,
        &config.language,
        &mut state.show_settings,
    );

    // take ui windows to allow state to be passed to each window
    let mut ui_windows = std::mem::take(&mut state.shared.ui_windows);
    ui_windows.show(ctx, config, &mut state.shared);
    // store original ui windows, append any new ui windows
    std::mem::swap(&mut state.shared.ui_windows, &mut ui_windows);
    state.shared.ui_windows.merge(ui_windows);

    let session = state.shared.session.clone();
    let session_guard = session.lock();

    if let Some(session) = session_guard.as_ref() {
        window.set_title(&i18n::LOCALES.lookup(&config.language, "window-title.running").unwrap());
        session_view::show(
            ctx,
            config,
            &mut state.shared,
            input_state,
            session,
            &state.last_mouse_motion_time,
            &mut state.show_escape_window,
            state.session_view.get_or_insert_with(session_view::State::new),
        );
    } else {
        state.session_view = None;
        window.set_title(&i18n::LOCALES.lookup(&config.language, "window-title").unwrap());
        main_view::show(
            ctx,
            config,
            &mut state.shared,
            window,
            &mut state.show_settings,
            &mut state.main_view,
            &mut state.init_link_code,
            updater,
        );
    }
}
