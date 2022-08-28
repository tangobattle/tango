use crate::{games, gui, i18n, patch};

pub struct State {}

pub struct PatchesWindow {}

impl PatchesWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        show: &mut Option<State>,
        language: &unic_langid::LanguageIdentifier,
        patches_path: &std::path::Path,
        patches_scanner: gui::PatchesScanner,
    ) {
    }
}
