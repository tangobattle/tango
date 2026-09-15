use crate::scanner;
use crate::storage::{Listing, Storage};

pub type GameRef = tango_gamesupport::GameRef;
pub type Scanner = scanner::Scanner<std::collections::HashMap<GameRef, Vec<u8>>>;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("ROM for {family} v{variant} is not in the library")]
    MissingRom { family: &'static str, variant: u8 },
    #[error("apply {name} {version}: {source}")]
    Patch {
        name: String,
        version: semver::Version,
        #[source]
        source: crate::patch::Error,
    },
}

/// Load the scanned ROM with the exact requested patch. The scanner keeps
/// the clean image; every session and replay consumer gets its own bytes.
/// Release the scanner lock before reading or applying a patch.
pub fn load(
    storage: &dyn Storage,
    roms: &Scanner,
    patches_path: &std::path::Path,
    game: GameRef,
    patch: Option<(&str, &semver::Version)>,
) -> Result<Vec<u8>, LoadError> {
    let raw = roms.read().get(&game).cloned().ok_or(LoadError::MissingRom {
        family: game.family.id,
        variant: game.variant,
    })?;
    match patch {
        None => Ok(raw),
        Some((name, version)) => {
            crate::patch::apply_patch(storage, &raw, game, patches_path, name, version).map_err(|source| {
                LoadError::Patch {
                    name: name.to_owned(),
                    version: version.clone(),
                    source,
                }
            })
        }
    }
}

/// Everything [`scan_roms`] reads: the configured roms dir plus any
/// BNLC Steam per-game archives. Feeds the scanner's change-detection
/// fingerprint so an unchanged-on-disk rescan can be skipped.
pub fn scan_roots(roms_path: &std::path::Path) -> Vec<std::path::PathBuf> {
    // Only the BNLC arm below mutates this, and that arm is compiled out
    // of a wasm build.
    #[allow(unused_mut)]
    let mut roots = vec![roms_path.to_path_buf()];
    #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
    for volume in [crate::bnlc::Volume::Vol1, crate::bnlc::Volume::Vol2] {
        if let Some(b) = crate::bnlc::get(volume) {
            roots.extend(b.rom_archives());
        }
    }
    roots
}

/// Discover ROMs from the library's roms directory, plus — natively —
/// any Steam-installed BN Legacy Collection volumes.
///
/// `listing` is the snapshot of [`scan_roots`] the caller already
/// gathered; see [`Listing`] for why the enumeration happens there and
/// not here.
pub fn scan_roms(storage: &dyn Storage, listing: &Listing) -> std::collections::HashMap<GameRef, Vec<u8>> {
    let mut roms = std::collections::HashMap::new();
    #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
    roms.extend(crate::bnlc::scan_steam_roms());
    roms.extend(scan_stored_roms(storage, listing));
    roms
}

fn scan_stored_roms(storage: &dyn Storage, listing: &Listing) -> std::collections::HashMap<GameRef, Vec<u8>> {
    let mut roms = std::collections::HashMap::new();
    for entry in listing.entries() {
        let mut buf = match storage.read(&entry.path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("{}: {e}", entry.path.display());
                continue;
            }
        };
        let Some(game) = crate::game::detect(&mut buf) else {
            log::debug!("rom scan: {}: not a recognized rom", entry.path.display());
            continue;
        };
        log::info!("rom scan: {}: {:?}", entry.path.display(), game.family_and_variant());
        roms.insert(game, buf);
    }
    roms
}
