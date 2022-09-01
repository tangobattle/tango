mod folder_view;
mod modcards56_view;

use fluent_templates::Loader;

use crate::{game, gui, i18n, rom, save};

#[derive(PartialEq)]
enum Tab {
    Folder,
    Modcards,
}

pub struct State {
    tab: Tab,
    folder_view: folder_view::State,
    modcards56_view: modcards56_view::State,
    texture_cache:
        std::collections::HashMap<(gui::save_view::CachedAssetType, usize), egui::TextureHandle>,
}

impl State {
    pub fn new() -> Self {
        Self {
            tab: Tab::Folder,
            folder_view: folder_view::State::new(),
            modcards56_view: modcards56_view::State::new(),
            texture_cache: std::collections::HashMap::new(),
        }
    }
}

pub struct SaveView {
    folder_view: folder_view::FolderView,
    modcards56_view: modcards56_view::Modcards56View,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CachedAssetType {
    ChipIcon,
    ElementIcon,
}

impl SaveView {
    pub fn new() -> Self {
        Self {
            folder_view: folder_view::FolderView::new(),
            modcards56_view: modcards56_view::Modcards56View::new(),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        clipboard: &mut arboard::Clipboard,
        font_families: &gui::FontFamilies,
        lang: &unic_langid::LanguageIdentifier,
        game: &'static (dyn game::Game + Send + Sync),
        save: &Box<dyn save::Save + Send + Sync>,
        assets: &Box<dyn rom::Assets + Send + Sync>,
        state: &mut State,
    ) {
        let chips_view = save.view_chips();
        let modcards56_view = save.view_modcards56();

        ui.horizontal(|ui| {
            if chips_view.is_some() {
                if ui
                    .selectable_label(
                        state.tab == Tab::Folder,
                        i18n::LOCALES.lookup(lang, "save.folder").unwrap(),
                    )
                    .clicked()
                {
                    state.tab = Tab::Folder;
                }
            }
            if modcards56_view.is_some() {
                if ui
                    .selectable_label(
                        state.tab == Tab::Modcards,
                        i18n::LOCALES.lookup(lang, "save.modcards").unwrap(),
                    )
                    .clicked()
                {
                    state.tab = Tab::Modcards;
                }
            }
        });

        match state.tab {
            Tab::Folder => {
                if let Some(chips_view) = chips_view {
                    self.folder_view.show(
                        ui,
                        clipboard,
                        font_families,
                        lang,
                        game,
                        &chips_view,
                        assets,
                        &mut state.texture_cache,
                        &mut state.folder_view,
                    );
                }
            }
            Tab::Modcards => {
                if let Some(modcards56_view) = modcards56_view {
                    self.modcards56_view.show(
                        ui,
                        clipboard,
                        font_families,
                        lang,
                        game,
                        &modcards56_view,
                        assets,
                        &mut state.modcards56_view,
                    );
                }
            }
        }
    }
}
