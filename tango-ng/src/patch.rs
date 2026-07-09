//! BPS patch application, copied from `tango/src/patch.rs` (the disk
//! scanner/autoupdater parts come over with the Patches tab).

use crate::rom::GameRef;

/// Read and decode the .bps for `game` from `patches_path/<patch_name>/v<version>/`,
/// then apply it on top of the supplied ROM. Returns the patched ROM bytes.
pub fn apply_patch_from_disk(
    rom: &[u8],
    game: GameRef,
    patches_path: &std::path::Path,
    patch_name: &str,
    patch_version: &semver::Version,
) -> anyhow::Result<Vec<u8>> {
    let patch_name_path = std::path::Path::new(patch_name);
    if patch_name_path.components().count() > 1 {
        anyhow::bail!("attempted path traversal in patch name");
    }

    let (rom_code, revision) = game.rom_code_and_revision();
    let bps_path = patches_path
        .join(patch_name_path)
        .join(format!("v{patch_version}"))
        .join(format!(
            "{}_{:02}.bps",
            std::str::from_utf8(rom_code).unwrap(),
            revision
        ));
    let raw = std::fs::read(&bps_path)?;
    Ok(bps::Patch::decode(&raw)?.apply(rom)?)
}
