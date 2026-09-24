//! The shared selection policy, with explicit browser library access.
//!
//! Every pick goes through [`tango_library::loadout::Selection`], the
//! same policy the desktop's pickers use — including the memory of each
//! family's last save and each save's last patch, which the app shell
//! records through [`crate::library::remember_selection`].
pub use tango_library::loadout::Selection as Loadout;

use tango_library::rom::GameRef;

/// Run `f` over the selection with the library's catalog and config.
/// A no-op before the library has opened.
fn apply(
    loadout: &mut Loadout,
    library: &crate::library::Handle,
    f: impl FnOnce(&mut Loadout, &tango_library::Catalog, &tango_library::config::Config),
) {
    crate::library::with(library, |catalog| {
        f(loadout, &catalog.catalog, &catalog.config.borrow())
    });
}

/// Bring back what was picked last visit.
pub fn restore(loadout: &mut Loadout, library: &crate::library::Handle) {
    apply(loadout, library, |loadout, catalog, config| {
        loadout.restore(config, catalog)
    });
}

/// Re-validate after the library changed underneath the pick.
pub fn reconcile(loadout: &mut Loadout, library: &crate::library::Handle) {
    apply(loadout, library, |loadout, catalog, config| {
        loadout.reconcile(catalog, config)
    });
}

pub fn pick_game(loadout: &mut Loadout, game: GameRef, library: &crate::library::Handle) {
    apply(loadout, library, |loadout, catalog, config| {
        loadout.pick_game(game, catalog, config)
    });
}

pub fn pick_save(loadout: &mut Loadout, game: GameRef, path: std::path::PathBuf, library: &crate::library::Handle) {
    apply(loadout, library, |loadout, catalog, config| {
        loadout.pick_save(game, path, catalog, config)
    });
}

/// Pick a patch at an exact version, or `None` for the game as it shipped.
pub fn pick_patch(loadout: &mut Loadout, patch: Option<(String, semver::Version)>, library: &crate::library::Handle) {
    apply(loadout, library, |loadout, catalog, config| match patch {
        Some((name, version)) => {
            loadout.pick_patch(Some(name), catalog, config);
            loadout.pick_patch_version(version);
        }
        None => loadout.pick_patch(None, catalog, config),
    });
}

/// Adopt a save just created from a template.
pub fn adopt_created_save(
    loadout: &mut Loadout,
    game: GameRef,
    path: std::path::PathBuf,
    library: &crate::library::Handle,
) {
    apply(loadout, library, |loadout, catalog, _| {
        loadout.adopt_created_save(game, path, catalog)
    });
}

pub fn save_bytes(loadout: &Loadout, library: &crate::library::Handle) -> Option<Vec<u8>> {
    use tango_library::Storage;
    crate::library::with(library, |catalog| catalog.files.read(loadout.save()?).ok())?
}

pub fn resolve(
    loadout: &Loadout,
    library: &crate::library::Handle,
) -> Result<tango_library::loadout::ResolvedLoadout, String> {
    crate::library::with(library, |library| {
        library
            .catalog
            .resolver(&library.files, &library.config.borrow())
            .resolve(loadout, None)
    })
    .ok_or_else(|| "library not open".to_owned())?
    .map_err(|e| e.to_string())
}
