//! BN3's save-editor UI: the style-bearing navicust (read-only) and the chip folder.

use tango_gamesupport_common_ui::editor::loaded::OpenSave;
use tango_gamesupport_common_ui::editor::view as sv;
use tango_gamesupport_common_ui::editor::view::{Action, State, Tab};
use tango_gamesupport_common_ui::editor::{GameSaveEditor, SaveEditorShell};
use unic_langid::LanguageIdentifier;

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

impl GameSaveEditor for Ui {
    /// The navicust is read-only here, so it never gets the shared
    /// navicust editor.
    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        state: &'a State,
    ) -> iced::Element<'a, Action> {
        match tab {
            Tab::Navicust => sv::placeholder(tango_gamesupport_common_ui::t!(lang, "save-empty")),
            tab => sv::render_standard_edit(lang, tab, loaded, state),
        }
    }
}
