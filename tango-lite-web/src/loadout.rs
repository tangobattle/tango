//! Shared selection policy with explicit browser library access.
pub use tango_library::loadout::LoadoutSelection as Loadout;

pub fn reconcile(loadout: &mut Loadout, library: &crate::library::Handle) {
    crate::library::with(library, |catalog| {
        loadout.reconcile(&catalog.roms.read(), &catalog.saves.read(), &catalog.patches.read())
    });
}
pub fn save_bytes(loadout: &Loadout, library: &crate::library::Handle) -> Option<Vec<u8>> {
    use tango_library::Storage;
    crate::library::with(library, |catalog| {
        catalog.files.read(loadout.save_path.as_deref()?).ok()
    })?
}
pub fn resolve(
    loadout: &Loadout,
    library: &crate::library::Handle,
) -> Result<tango_library::loadout::ResolvedLoadout, String> {
    crate::library::with(library, |catalog| {
        tango_library::loadout::Resolver {
            storage: &catalog.files,
            roms: &catalog.roms,
            patches: &catalog.patches,
            patches_path: &catalog.config.patches_path(),
        }
        .resolve(loadout, None)
    })
    .ok_or_else(|| "library not open".to_owned())?
    .map_err(|e| e.to_string())
}
