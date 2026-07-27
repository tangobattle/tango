//! App-side constructors for [`OpenSave`].
//!
//! The type itself — the loaded save's model plus the baked art the
//! save view draws from — lives in `tango_gamesupport_common::loaded`; the
//! constructors that need app collaborators (the storage root, the
//! scanner caches, the config) stay here. Every construction resolves
//! the game's save-editor UI through [`crate::save_view::save_ui_for`],
//! so the view side never has to ask again.

use std::sync::Arc;

pub use tango_gamesupport_common::loaded::OpenSave;
pub use tango_savemodel::AppliedPatch;

/// Build from a *raw* (unpatched) ROM, applying the selected patch
/// first, then bake the frontend art for it.
pub fn build(
    game: crate::library::rom::GameRef,
    rom: Vec<u8>,
    save_path: std::path::PathBuf,
    save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    patches_path: &std::path::Path,
    patch: Option<(String, semver::Version, Arc<crate::library::patch::Version>)>,
) -> OpenSave {
    OpenSave::from_model(
        tango_savemodel::SaveModel::build(
            crate::library::storage(),
            game,
            rom,
            save_path,
            save,
            patches_path,
            patch,
        ),
        crate::save_view::save_ui_for(game),
    )
}

/// Build from a ROM that's *already* had its patch applied, plus the
/// [`AppliedPatch`] that produced it (`None` for a raw ROM) — for
/// callers that already hold the patched image (e.g. a live session
/// that patched the ROM for the emulator), so the BPS patch isn't
/// applied a second time.
pub fn from_patched_rom(
    game: crate::library::rom::GameRef,
    rom: Vec<u8>,
    save_path: std::path::PathBuf,
    save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    applied_patch: Option<AppliedPatch>,
) -> OpenSave {
    OpenSave::from_patched_rom(
        game,
        rom,
        save_path,
        save,
        applied_patch,
        crate::save_view::save_ui_for(game),
    )
}

/// Build an [`OpenSave`] for the local side of a replay — used by the
/// replays tab to embed the save view in its detail panel. Pulls
/// the local rom + patch from the scanners cache; returns Err
/// if anything's missing.
pub fn for_replay_local(
    scanners: &crate::app::Scanners,
    config: &crate::config::Config,
    replay: &tango_replay::Replay,
) -> anyhow::Result<OpenSave> {
    let side = replay
        .local_side()
        .ok_or_else(|| anyhow::anyhow!("replay missing local side metadata"))?;
    let gi = side
        .game_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("replay side has no game info"))?;
    let variant =
        u8::try_from(gi.rom_variant).map_err(|_| anyhow::anyhow!("variant {} out of range", gi.rom_variant))?;
    let game = crate::library::game::find_by_family_and_variant(&gi.rom_family, variant)
        .ok_or_else(|| anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, gi.rom_variant))?;
    let rom = scanners
        .roms
        .read()
        .get(&game)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("rom for {}/{} not scanned", gi.rom_family, gi.rom_variant))?;

    let save = game.parse_save(&replay.srams[replay.local_player_index as usize])?;

    // Optional patch info — pull the Arc<Version> from the patch
    // scanner so we get the same rom_overrides (charset etc.) as
    // the play tab.
    let patch_meta = gi.patch.as_ref().and_then(|p| {
        let v = semver::Version::parse(&p.version).ok()?;
        let patches = scanners.patches.read();
        let vmeta = patches.version(&p.name, &v)?.clone();
        Some((p.name.clone(), v, vmeta))
    });

    Ok(build(
        game,
        rom,
        std::path::PathBuf::new(),
        save,
        &config.patches_path(),
        patch_meta,
    ))
}
