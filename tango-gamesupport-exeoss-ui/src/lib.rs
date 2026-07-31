//! OSS's save-editor UI: the chip folder, and nothing else yet — the
//! only part of the cart's save this dataview maps. Editing is off for
//! this game (the save hands out no writable chips view, since writing
//! a bank back would have to restamp integrity words nothing
//! reproduces), so the shell offers no Edit button and the folder
//! editor below never runs. The tab probes the view the same way BN5DS
//! does, which makes everything under the top bar the ordinary save
//! viewer.

use tango_gamesupport_common::editor::loaded::OpenSave;
use tango_gamesupport_common::editor::view as sv;
use tango_gamesupport_common::editor::view::{Action, RenderOpts, State, Tab};
use tango_gamesupport_common::editor::{GameSaveEditor, SaveEditorShell};
use unic_langid::LanguageIdentifier;

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

impl GameSaveEditor for Ui {
    fn tabs(&self, loaded: &OpenSave) -> Vec<Tab> {
        let save = loaded.save.as_ref();
        let mut tabs = vec![];
        if save.view_chips().is_some() {
            tabs.push(Tab::Folder);
        }
        tabs
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
            Tab::Folder => sv::folder::render_folder(lang, loaded, opts.folder_grouped),
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
            Tab::Folder => sv::folder::render_folder_edit(lang, loaded, state),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
        }
    }

    fn tab_as_text(&self, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
        match tab {
            Tab::Folder => sv::folder::as_text(loaded, opts),
            _ => None,
        }
    }
}
