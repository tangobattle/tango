//! Attach the save editor to a save the library prepared: a loaded save
//! bundled with the UI that renders it, produced through the registry's
//! opaque embedding API. Preparation itself — patching the ROM, parsing
//! the save, deriving assets — is `tango_library::loadout::Resolver`'s.

pub use tango_gamesupport::{LoadedSave, PreparedSave};

/// All selections come from the library registry, which pairs every enabled
/// game with its editor through the library's `ui` feature.
pub fn editor(game: crate::library::rom::GameRef) -> &'static dyn tango_gamesupport::SaveEditor {
    crate::library::game::save_editor(game).expect("selected game must have a registered editor")
}

/// Hand a prepared save to its game's editor.
pub fn load(prepared: PreparedSave) -> LoadedSave {
    editor(prepared.game).load(prepared)
}
