mod dark_ai_view;
mod folder_view;
mod modcards_view;
mod navicust_view;

use fluent_templates::Loader;

use crate::{gui, i18n, rom, save};

#[derive(PartialEq, Clone)]
enum Tab {
    Navicust,
    Folder,
    Modcards,
    DarkAI,
}

pub struct State {
    tab: Option<Tab>,
    navicust_view: navicust_view::State,
    folder_view: folder_view::State,
    modcards_view: modcards_view::State,
    dark_ai_view: dark_ai_view::State,
}

impl State {
    pub fn new() -> Self {
        Self {
            tab: None,
            navicust_view: navicust_view::State::new(),
            folder_view: folder_view::State::new(),
            modcards_view: modcards_view::State::new(),
            dark_ai_view: dark_ai_view::State::new(),
        }
    }
}
pub fn show(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    save: &Box<dyn save::Save + Send + Sync>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
) {
    ui.vertical(|ui| {
        let navicust_view = save.view_navicust();
        let chips_view = save.view_chips();
        let modcards_view = save.view_modcards();
        let dark_ai_view = save.view_dark_ai();

        let mut available_tabs = vec![];
        if navicust_view.is_some() {
            available_tabs.push(Tab::Navicust);
        }
        if chips_view.is_some() {
            available_tabs.push(Tab::Folder);
        }
        if modcards_view.is_some() {
            available_tabs.push(Tab::Modcards);
        }
        if dark_ai_view.is_some() {
            available_tabs.push(Tab::DarkAI);
        }

        ui.horizontal(|ui| {
            for tab in available_tabs.iter() {
                if ui
                    .selectable_label(
                        state.tab.as_ref() == Some(tab),
                        i18n::LOCALES
                            .lookup(
                                lang,
                                match tab {
                                    Tab::Navicust => "save.navicust",
                                    Tab::Folder => "save.folder",
                                    Tab::Modcards => "save.modcards",
                                    Tab::DarkAI => "save.dark-ai",
                                },
                            )
                            .unwrap(),
                    )
                    .clicked()
                {
                    state.tab = Some(tab.clone());
                }
            }
        });

        if state.tab.is_none() {
            state.tab = available_tabs.first().cloned();
        }

        match state.tab {
            Some(Tab::Navicust) => {
                if let Some(navicust_view) = navicust_view {
                    navicust_view::show(
                        ui,
                        clipboard,
                        font_families,
                        lang,
                        game_lang,
                        &navicust_view,
                        assets,
                        &mut state.navicust_view,
                    );
                }
            }
            Some(Tab::Folder) => {
                if let Some(chips_view) = chips_view {
                    folder_view::show(
                        ui,
                        clipboard,
                        font_families,
                        lang,
                        game_lang,
                        &chips_view,
                        assets,
                        &mut state.folder_view,
                    );
                }
            }
            Some(Tab::Modcards) => {
                if let Some(modcards_view) = modcards_view {
                    modcards_view::show(
                        ui,
                        clipboard,
                        font_families,
                        lang,
                        game_lang,
                        &modcards_view,
                        assets,
                        &mut state.modcards_view,
                    );
                }
            }
            Some(Tab::DarkAI) => {
                if let Some(dark_ai_view) = dark_ai_view {
                    dark_ai_view::show(
                        ui,
                        clipboard,
                        font_families,
                        lang,
                        game_lang,
                        &dark_ai_view,
                        assets,
                        &mut state.dark_ai_view,
                    );
                }
            }
            None => {}
        }
    });
}
