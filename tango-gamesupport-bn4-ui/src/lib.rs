//! BN4's save-editor UI: navicust, folder, the six-slot Mod Card (patch
//! card 4) form — BN4's own, not the BN5/BN6 list — and auto battle data.

use tango_gamesupport_common::editor::loaded::OpenSave;
use tango_gamesupport_common::editor::view as sv;
use tango_gamesupport_common::editor::view::{Action, RenderOpts, State, Tab};
use tango_gamesupport_common::editor::{GameSaveEditor, SaveEditorShell};
use unic_langid::LanguageIdentifier;

mod patch_cards4;

pub use patch_cards4::PatchCard4Edit;

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

impl GameSaveEditor for Ui {
    fn tabs(&self, loaded: &OpenSave) -> Vec<Tab> {
        let save = loaded.save.as_ref();
        let mut tabs = vec![];
        if save.view_navicust().is_some() {
            tabs.push(Tab::Navicust);
        }
        tabs.push(Tab::Folder);
        // Every BN4 save has the six Mod Card slots (there's no link
        // navi to take them away).
        tabs.push(Tab::PatchCards);
        if save.view_auto_battle_data().is_some() {
            tabs.push(Tab::AutoBattleData);
        }
        tabs
    }

    /// Mod Cards are BN4's own model, invisible to the shared
    /// `Editability` probe — they're always editable here.
    fn tab_editable(&self, tab: Tab, loaded: &OpenSave) -> bool {
        match tab {
            Tab::PatchCards => true,
            _ => tab.editable_on(&loaded.editability),
        }
    }

    fn render<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        opts: RenderOpts,
    ) -> iced::Element<'a, Action> {
        match tab {
            Tab::Cover => sv::cover::render_cover(lang, loaded),
            Tab::Navicust => sv::navicust::render_navicust_tab(lang, loaded),
            Tab::Folder => sv::folder::render_folder(lang, loaded, opts.folder_grouped),
            Tab::PatchCards => patch_cards4::render(lang, loaded),
            Tab::AutoBattleData => sv::abd::render_auto_battle_data(lang, loaded),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
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
            Tab::PatchCards => patch_cards4::render_edit(lang, loaded, state),
            Tab::AutoBattleData => sv::abd::render_auto_battle_data_edit(lang, loaded, state),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
        }
    }

    fn tab_as_text(&self, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
        match tab {
            Tab::Navicust => sv::navicust::navicust_as_text(loaded),
            Tab::Folder => sv::folder::as_text(loaded, opts),
            Tab::PatchCards => patch_cards4::as_text(loaded),
            Tab::AutoBattleData => sv::abd::as_text(loaded),
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
