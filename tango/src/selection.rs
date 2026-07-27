//! App-side constructors for [`OpenSave`].
//!
//! The type itself — the loaded save's model plus the baked art the
//! save view draws from — lives in `tango_gamesupport::loaded`;
//! everything that needs app collaborators stays here: applying the
//! selected patch (the scanner types and storage root are the app's)
//! and resolving the Cover tab's logo order from the game registry.
//! The save-editor UI comes straight off `Game::save_ui`.

use std::sync::Arc;

pub use tango_gamesupport::loaded::OpenSave;
pub use tango_gamesupport::model::AppliedPatch;

/// The Cover tab's logo order: the loaded variant first, then its
/// family siblings (the other color version, where one exists) so
/// twin-version families fan both logos out.
fn logo_games(game: crate::library::rom::GameRef) -> Vec<crate::library::rom::GameRef> {
    let (family, variant) = game.family_and_variant();
    let mut order = vec![game];
    for g in crate::library::game::games_in_family(family) {
        if g.family_and_variant().1 != variant {
            order.push(g);
        }
    }
    order
}

/// Build from a *raw* (unpatched) ROM, applying the selected patch
/// first, then bake the frontend art for it. On apply failure we fall
/// back to the unpatched ROM (and log) so the save view still renders.
pub fn build(
    game: crate::library::rom::GameRef,
    rom: Vec<u8>,
    save_path: std::path::PathBuf,
    save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    patches_path: &std::path::Path,
    patch: Option<(String, semver::Version, Arc<crate::library::patch::Version>)>,
) -> OpenSave {
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
                        rom_overrides: meta.rom_overrides.clone(),
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
    save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    applied_patch: Option<AppliedPatch>,
) -> OpenSave {
    let model = tango_gamesupport::model::SaveModel::from_patched_rom(game, rom, save_path, save, applied_patch);
    OpenSave::from_model(model, game.save_ui, &logo_games(game))
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
