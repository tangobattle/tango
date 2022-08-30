use crate::{game, gui, rom, save};

mod folder_view;

pub struct SaveView {
    folder_view: folder_view::FolderView,
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
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        font_families: &gui::FontFamilies,
        lang: &unic_langid::LanguageIdentifier,
        game: &'static (dyn game::Game + Send + Sync),
        save: &Box<dyn save::Save + Send + Sync>,
        assets: &Box<dyn rom::Assets + Send + Sync>,
        texture_cache: &mut std::collections::HashMap<
            (CachedAssetType, usize),
            egui::TextureHandle,
        >,
    ) {
        ui.horizontal(|ui| {
            ui.selectable_label(true, "TODO");
            ui.selectable_label(false, "TODO 2");
        });

        if let Some(chips_view) = save.view_chips() {
            self.folder_view.show(
                ui,
                font_families,
                lang,
                game,
                &chips_view,
                assets,
                texture_cache,
            );
        }
    }
}
