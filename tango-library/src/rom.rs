use crate::scanner;
use crate::storage::{Listing, Storage};

pub type GameRef = tango_gamesupport::GameRef;
pub type Scanner = scanner::Scanner<std::collections::HashMap<GameRef, Vec<u8>>>;

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
        let buf = match storage.read(&entry.path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("{}: {e}", entry.path.display());
                continue;
            }
        };
        let Some(game) = crate::game::detect(&buf) else {
            log::debug!("rom scan: {}: not a recognized rom", entry.path.display());
            continue;
        };
        log::info!("rom scan: {}: {:?}", entry.path.display(), game.family_and_variant());
        roms.insert(game, buf);
    }
    roms
}
