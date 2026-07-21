//! The Dioxus component tree: a Play/Settings tab shell while idle and
//! a fullscreen session view while a game runs (or its end is still on
//! screen). Modeled on gbaroll's shell; the screens themselves are
//! Tango's.

mod diag;
pub(crate) mod icons;
mod lobby_band;
mod patches_tab;
mod play;
mod replays;
mod session_view;
mod settings;
mod shell;
mod touch;
mod welcome;

pub use shell::App;

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;

use crate::config::Config;
use crate::library::Library;
use crate::runtime::Runtime;
use crate::storage::Storage;

/// The desktop's four tabs, same order: Play and Replays as labeled
/// pills, Patches and Settings demoted to icon-only.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Play,
    Replays,
    Patches,
    Settings,
}

/// Handles shared by every screen, provided once by [`shell::App`].
/// `pub(crate)` so out-of-tree embedders (the save view) can reach the
/// storage handle too.
#[derive(Clone)]
pub(crate) struct Ctx {
    pub(crate) runtime: Rc<RefCell<Runtime>>,
    pub(crate) config: Signal<Config>,
    /// Bumped to rescan the library after imports and deletes.
    pub(crate) library_rev: Signal<u64>,
    /// `Some(None)` when the browser has no OPFS.
    pub(crate) storage: Resource<Option<Storage>>,
    /// The ROM library scan; `None` until OPFS is up.
    pub(crate) library: Resource<Option<Library>>,
    /// The synced patch list, rescanned on PATCHES_REV bumps.
    pub(crate) patches: Resource<Vec<crate::patches::Patch>>,
    /// The picked game *family* (region-specific family string) —
    /// whose saves the save pane shows. Per family, not per game,
    /// like the desktop loadout.
    pub(crate) selected_family: Signal<Option<String>>,
    /// The save picker's choice for the next boot: a file name inside
    /// the flat `saves/` directory, or a `//fresh/<variant>` sentinel.
    /// `None` = the default fresh row (first owned variant).
    pub(crate) selected_save: Signal<Option<String>>,
}

pub(crate) fn use_ctx() -> Ctx {
    use_context::<Ctx>()
}
