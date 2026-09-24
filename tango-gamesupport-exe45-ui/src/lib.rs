//! EXE4.5's save-editor UI: the chip folder. The navi roster is the shared navi strip/picker's job, not a tab.

use tango_gamesupport_common_ui::editor::{GameSaveEditor, SaveEditorShell};

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

// Every tab is a standard one, so the trait's default routing is the
// whole editor.
impl GameSaveEditor for Ui {}
