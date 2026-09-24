//! OSS's save-editor UI: the chip folder, and nothing else yet — the
//! only part of the cart's save this dataview maps. It is the shared
//! folder editor, unadorned: the save hands out a writable chips view
//! and a pack behind it, so the shell's own Edit button, chip library
//! and 30-slot rule are the whole of it.
//!
//! The cart's folder rules come with it: five copies of any one chip
//! and five navi chips, which the save reports as its navi's
//! [`FolderLimits`] and the shared editor enforces the way it does
//! every other game's — graying out and disabling any library choice
//! that would exceed a cap, and blocking Save while the folder has an
//! existing error.
//!
//! [`FolderLimits`]: tango_gamesupport_common_ui::dataview::save::FolderLimits

use tango_gamesupport_common_ui::editor::{GameSaveEditor, SaveEditorShell};

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

// Every tab is a standard one, so the trait's default routing is the
// whole editor.
impl GameSaveEditor for Ui {}
