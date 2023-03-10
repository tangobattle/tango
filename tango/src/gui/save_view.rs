mod dark_ai_view;
mod folder_view;
mod navi_view;
mod navicust_view;
mod patch_cards_view;

use fluent_templates::Loader;

use crate::{gui, i18n, rom, save};

#[derive(PartialEq, Clone)]
enum Tab {
    Navi,
    Navicust,
    Folder,
    PatchCards,
    DarkAI,
}

pub struct State {
    tab: Option<Tab>,
    navi_view: navi_view::State,
    navicust_view: navicust_view::State,
    folder_view: folder_view::State,
    patch_cards_view: patch_cards_view::State,
    dark_ai_view: dark_ai_view::State,
}

impl State {
    pub fn new() -> Self {
        Self {
            tab: None,
            navi_view: navi_view::State::new(),
            navicust_view: navicust_view::State::new(),
            folder_view: folder_view::State::new(),
            patch_cards_view: patch_cards_view::State::new(),
            dark_ai_view: dark_ai_view::State::new(),
        }
    }
}
pub fn show(
    ui: &mut egui::Ui,
    streamer_mode: bool,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    save: &Box<dyn save::Save + Send + Sync>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
    prefer_vertical: bool,
) {
    ui.vertical(|ui| {
        let navi_view = save.view_navi();
        let navicust_view = save.view_navicust();
        let chips_view = save.view_chips();
        let patch_cards_view = save.view_patch_cards();
        let dark_ai_view = save.view_dark_ai();

        let mut available_tabs = vec![];
        if navi_view.is_some() {
            available_tabs.push(Tab::Navi);
        }
        if navicust_view.is_some() {
            available_tabs.push(Tab::Navicust);
        }
        if chips_view.is_some() {
            available_tabs.push(Tab::Folder);
        }
        if patch_cards_view.is_some() {
            available_tabs.push(Tab::PatchCards);
        }
        if dark_ai_view.is_some() {
            available_tabs.push(Tab::DarkAI);
        }

        ui.horizontal(|ui| {
            if streamer_mode
                && ui
                    .selectable_label(state.tab == None, i18n::LOCALES.lookup(lang, "save-tab-cover").unwrap())
                    .clicked()
            {
                state.tab = None;
            }

            for tab in available_tabs.iter() {
                if ui
                    .selectable_label(
                        state.tab.as_ref() == Some(tab),
                        i18n::LOCALES
                            .lookup(
                                lang,
                                match tab {
                                    Tab::Navi => "save-tab-navi",
                                    Tab::Navicust => "save-tab-navicust",
                                    Tab::Folder => "save-tab-folder",
                                    Tab::PatchCards => "save-tab-patch-cards",
                                    Tab::DarkAI => "save-tab-dark-ai",
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

        if state.tab.is_none() && !streamer_mode {
            state.tab = available_tabs.first().cloned();
        }

        match state.tab {
            Some(Tab::Navi) => {
                if let Some(navi_view) = navi_view {
                    navi_view::show(
                        ui,
                        clipboard,
                        font_families,
                        lang,
                        game_lang,
                        &navi_view,
                        assets,
                        &mut state.navi_view,
                    );
                }
            }
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
                        prefer_vertical,
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
            Some(Tab::PatchCards) => {
                if let Some(patch_cards_view) = patch_cards_view {
                    patch_cards_view::show(
                        ui,
                        clipboard,
                        font_families,
                        lang,
                        game_lang,
                        &patch_cards_view,
                        assets,
                        &mut state.patch_cards_view,
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
            None => {
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight)
                        .with_main_align(egui::Align::Center),
                    |ui| {
                        ui.label(i18n::LOCALES.lookup(lang, "save-cover-description").unwrap());
                    },
                );
            }
        }
    });
}
