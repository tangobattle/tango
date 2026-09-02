//! App-side constructors for [`LoadedSave`] — a loaded save bundled
//! with the UI that renders it, produced through `Game::save_editor`'s
//! opaque embedding API. Everything that needs app collaborators stays
//! here: applying the selected patch, whose scanner types and storage
//! root are the app's.

use std::sync::Arc;

pub use tango_gamesupport::{AppliedPatch, LoadedSave, PreparedSave};

/// Build from a *raw* (unpatched) ROM, applying the selected patch
/// first, then bake the frontend art for it. On apply failure we fall
/// back to the unpatched ROM (and log) so the save view still renders.
pub fn build(
    game: crate::library::rom::GameRef,
    rom: Vec<u8>,
    save_path: std::path::PathBuf,
    save: tango_gamesupport::BoxedSave,
    patches_path: &std::path::Path,
    patch: Option<(String, semver::Version, Arc<crate::library::patch::Version>)>,
) -> LoadedSave {
    let (rom, applied_patch) = match patch {
        Some((name, version, meta)) => {
            match crate::library::patch::apply_patch(
                crate::library::storage(),
                &rom,
                game,
                patches_path,
                &name,
                &version,
            ) {
                Ok(patched) => (
                    patched,
                    Some(AppliedPatch {
                        name,
                        version,
                        rom_overrides: meta.rom_overrides_for(game),
                    }),
                ),
                Err(e) => {
                    log::error!(
                        "failed to apply patch {name} v{version} to {:?}: {e}",
                        game.family_and_variant()
                    );
                    (rom, None)
                }
            }
        }
        None => (rom, None),
    };
    from_patched_rom(game, rom, save_path, save, applied_patch)
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
    save: tango_gamesupport::BoxedSave,
    applied_patch: Option<AppliedPatch>,
) -> LoadedSave {
    prepare_from_patched_rom(game, rom, save_path, save, applied_patch).load()
}

/// Prepare the parsed save and effective patched-ROM assets for callers that
/// need to inspect the model before moving it into the editor.
pub fn prepare_from_patched_rom(
    game: crate::library::rom::GameRef,
    rom: Vec<u8>,
    save_path: std::path::PathBuf,
    save: tango_gamesupport::BoxedSave,
    applied_patch: Option<AppliedPatch>,
) -> PreparedSave {
    tango_gamesupport_common_ui::model::prepare(game, rom, save_path, save, applied_patch)
}

/// Build a [`LoadedSave`] for one absolute player seat in a replay. Pulls
/// that side's ROM + patch from the scanners cache and parses the matching
/// embedded SRAM; returns Err if anything is missing.
pub fn for_replay_player(
    scanners: &crate::library::Scanners,
    config: &crate::config::Config,
    replay: &tango_replay::Replay,
    player_index: u8,
) -> anyhow::Result<LoadedSave> {
    let side = replay
        .metadata
        .side(player_index)
        .ok_or_else(|| anyhow::anyhow!("replay missing player {} side metadata", player_index + 1))?;
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

    let sram = replay
        .srams
        .get(player_index as usize)
        .ok_or_else(|| anyhow::anyhow!("replay player index {player_index} out of range"))?;
    let save = game.parse_save(sram)?;

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

/// Build the recorder's save for the replay detail viewer.
pub fn for_replay_local(
    scanners: &crate::library::Scanners,
    config: &crate::config::Config,
    replay: &tango_replay::Replay,
) -> anyhow::Result<LoadedSave> {
    for_replay_player(scanners, config, replay, replay.local_player_index)
}

/// Build the recorder's opponent save for the replay detail viewer.
pub fn for_replay_remote(
    scanners: &crate::library::Scanners,
    config: &crate::config::Config,
    replay: &tango_replay::Replay,
) -> anyhow::Result<LoadedSave> {
    for_replay_player(scanners, config, replay, 1 - replay.local_player_index)
}
