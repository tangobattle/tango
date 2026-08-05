//! EXE4.5's save-editor UI: the chip folder. The navi roster is the shared navi strip/picker's job, not a tab.

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
        vec![Tab::Folder]
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
            Tab::Folder => sv::folder::render_folder(lang, loaded, opts.folder_grouped),
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
            Tab::Folder => sv::folder::render_folder_edit(lang, loaded, state),
            _ => sv::placeholder(tango_gamesupport_common_ui::t!(lang, "save-empty")),
        }
    }

    fn tab_as_text(&self, _lang: &LanguageIdentifier, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
        let _ = opts;
        match tab {
            Tab::Folder => sv::folder::as_text(loaded, opts),
            _ => None,
        }
    }
}
