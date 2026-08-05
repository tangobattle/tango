//! BN5's save-editor UI: navicust, folder (with Dark chips), patch cards (56-style) and auto battle data. Link navis drop the navicust/patch-card tabs at runtime.

use tango_gamesupport_common_ui::editor::loaded::OpenSave;
use tango_gamesupport_common_ui::editor::view as sv;
use tango_gamesupport_common_ui::editor::view::{Action, RenderOpts, State, Tab};
use tango_gamesupport_common_ui::editor::{GameSaveEditor, SaveEditorShell};
use unic_langid::LanguageIdentifier;

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

impl GameSaveEditor for Ui {
    fn tabs(&self, loaded: &OpenSave) -> Vec<Tab> {
        let _ = loaded;
        {
            let save = loaded.save.as_ref();
            let mut tabs = vec![];
            if save.view_navicust().is_some() {
                tabs.push(Tab::Navicust);
            }
            tabs.push(Tab::Folder);
            if save.view_patch_card56s().is_some() {
                tabs.push(Tab::PatchCards);
            }
            if save.view_auto_battle_data().is_some() {
                tabs.push(Tab::AutoBattleData);
            }
            tabs
        }
    }

    fn render<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        opts: RenderOpts,
    ) -> iced::Element<'a, Action> {
        let _ = opts;
        match tab {
            Tab::Cover => sv::cover::render_cover(lang, loaded),
            Tab::Navicust => sv::navicust::render_navicust_tab(lang, loaded),
            Tab::Folder => sv::folder::render_folder(lang, loaded, opts.folder_grouped),
            Tab::PatchCards => sv::patch_cards::render_patch_cards56(lang, loaded),
            Tab::AutoBattleData => sv::abd::render_auto_battle_data(lang, loaded),
            _ => sv::placeholder(tango_gamesupport_common_ui::t!(lang, "save-empty")),
        }
    }

    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        state: &'a State,
    ) -> iced::Element<'a, Action> {
        match tab {
            Tab::Navicust => sv::navicust::render_navicust_edit(lang, loaded, state),
            Tab::Folder => sv::folder::render_folder_edit(lang, loaded, state),
            Tab::PatchCards => sv::patch_cards::render_patch_cards56_edit(lang, loaded, state),
            Tab::AutoBattleData => sv::abd::render_auto_battle_data_edit(lang, loaded, state),
            _ => sv::placeholder(tango_gamesupport_common_ui::t!(lang, "save-empty")),
        }
    }

    fn tab_as_text(&self, lang: &LanguageIdentifier, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
        let _ = opts;
        match tab {
            Tab::Navicust => sv::navicust::navicust_as_text(loaded),
            Tab::Folder => sv::folder::as_text(loaded, opts),
            Tab::PatchCards => sv::patch_cards::as_text56(loaded),
            Tab::AutoBattleData => sv::abd::as_text(lang, loaded),
            _ => None,
        }
    }

    fn tab_as_image(&self, tab: Tab, loaded: &OpenSave) -> Option<image::RgbaImage> {
        match tab {
            Tab::Navicust => sv::navicust::as_image(loaded),
            _ => None,
        }
    }
}
