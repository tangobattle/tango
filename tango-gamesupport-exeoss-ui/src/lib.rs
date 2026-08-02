//! OSS's save-editor UI: the chip folder, and nothing else yet — the
//! only part of the cart's save this dataview maps. It is the shared
//! folder editor, unadorned: the save hands out a writable chips view
//! and a pack behind it, so the shell's own Edit button, chip library
//! and 30-slot rule are the whole of it.
//!
//! The cart's folder rules come with it: five copies of any one chip
//! and five navi chips, which the save reports as its navi's
//! [`FolderLimits`] and the shared editor enforces the way it does
//! every other game's — greying a library chip out at the cap, and
//! holding Save shut on a folder that is over one.
//!
//! [`FolderLimits`]: tango_gamesupport_common::dataview::save::FolderLimits

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
