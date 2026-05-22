use crate::bnlc;
use crate::scanner;
use std::io::Read;

pub type GameRef = &'static (dyn tango_gamedb::Game + Send + Sync);
pub type Scanner = scanner::Scanner<std::collections::HashMap<GameRef, Vec<u8>>>;

/// Discover ROMs from both the local data path and any Steam-installed
/// BN Legacy Collection volumes. Mirrors the layered scan in
/// `tango/src/game.rs::scan_roms`.
pub fn scan_roms(path: &std::path::Path) -> std::collections::HashMap<GameRef, Vec<u8>> {
    let mut roms = std::collections::HashMap::new();
    roms.extend(scan_bnlc_steam_roms());
    if std::fs::metadata(path).is_ok() {
        roms.extend(scan_non_bnlc_roms(path));
    }
    roms
}

fn scan_non_bnlc_roms(path: &std::path::Path) -> std::collections::HashMap<GameRef, Vec<u8>> {
    let mut roms = std::collections::HashMap::new();
    for entry in walkdir::WalkDir::new(path) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!("rom scan: {e:?}");
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let buf = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("{}: {e}", path.display());
                continue;
            }
        };
        let Some(game) = tango_gamedb::detect(&buf) else {
            log::debug!("rom scan: {}: not a recognized rom", path.display());
            continue;
        };
        log::info!("rom scan: {}: {:?}", path.display(), game.family_and_variant());
        roms.insert(game, buf);
    }
    roms
}

/// Pull every recognized .srl ROM out of each BNLC volume's
/// per-game `exeN.dat` archives. Steam discovery + the per-volume
/// `Bnlc` handle live in `crate::bnlc`.
fn scan_bnlc_steam_roms() -> std::collections::HashMap<GameRef, Vec<u8>> {
    let mut roms = std::collections::HashMap::new();
    for volume in [bnlc::Volume::Vol1, bnlc::Volume::Vol2] {
        let Some(b) = bnlc::get(volume) else {
            continue;
        };
        for archive in b.rom_archives() {
            roms.extend(scan_bnlc_rom_archive(&archive));
        }
    }
    roms
}

fn scan_bnlc_rom_archive(path: &std::path::Path) -> std::collections::HashMap<GameRef, Vec<u8>> {
    log::info!("scanning bnlc archive: {}", path.display());
    let mut roms = std::collections::HashMap::new();
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("failed to open lc archive {}: {e}", path.display());
            return roms;
        }
    };
    let mut za = match zip::ZipArchive::new(f) {
        Ok(za) => za,
        Err(e) => {
            log::error!("failed to open lc archive {}: {e}", path.display());
            return roms;
        }
    };
    for i in 0..za.len() {
        let mut entry = match za.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                log::warn!("bnlc: {}({}): {e}", path.display(), i);
                continue;
            }
        };
        let Some(entry_path) = entry.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };
        if entry_path.extension() != Some(std::ffi::OsStr::new("srl")) {
            continue;
        }
        let mut rom = vec![];
        if let Err(e) = entry.read_to_end(&mut rom) {
            log::warn!("bnlc: {}/{}: {e}", path.display(), entry_path.display());
            continue;
        }
        let Some(game) = tango_gamedb::detect(&rom) else {
            log::warn!("bnlc: {}/{}: not recognized", path.display(), entry_path.display());
            continue;
        };
        log::info!(
            "bnlc: {}/{}: {:?}",
            path.display(),
            entry_path.display(),
            game.family_and_variant()
        );
        roms.insert(game, rom);
    }
    roms
}
