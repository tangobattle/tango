//! BN5's save-editor UI: navicust, folder (with Dark chips), patch cards (56-style) and auto battle data. Link navis drop the navicust/patch-card tabs at runtime.

use tango_gamesupport_common_ui::editor::{GameSaveEditor, SaveEditorShell};

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

// Every tab is a standard one, so the trait's default routing is the
// whole editor.
impl GameSaveEditor for Ui {}
