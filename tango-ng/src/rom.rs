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
        log::info!(
            "rom scan: {}: {:?}",
            path.display(),
            game.family_and_variant()
        );
        roms.insert(game, buf);
    }
    roms
}

/// Locate Steam-installed Battle Network Legacy Collection (Vol 1 + Vol 2)
/// and extract every recognized .srl ROM from their packaged .dat zips.
fn scan_bnlc_steam_roms() -> std::collections::HashMap<GameRef, Vec<u8>> {
    let mut roms = std::collections::HashMap::new();
    let Ok(steamdir) = steamlocate::SteamDir::locate().inspect_err(|err| {
        log::debug!("steam not located: {err:?}");
    }) else {
        return roms;
    };

    // App IDs are the published Steam IDs for BNLC Vol 1 / Vol 2.
    for app_id in [1798010_u32, 1798020] {
        if let Ok(Some((app, lib))) = steamdir.find_app(app_id) {
            roms.extend(scan_bnlc_rom_archives(&lib.resolve_app_dir(&app)));
        }
    }
    roms
}

fn scan_bnlc_rom_archives(lc_path: &std::path::Path) -> std::collections::HashMap<GameRef, Vec<u8>> {
    let mut roms = std::collections::HashMap::new();
    let data_path = lc_path.join("exe").join("data");
    let read_dir = match std::fs::read_dir(&data_path) {
        Ok(rd) => rd,
        Err(e) => {
            log::warn!("bnlc: {}: {e}", lc_path.display());
            return roms;
        }
    };
    for entry in read_dir {
        let Ok(entry) = entry else { continue };
        let p = entry.path();
        let Some(file_name) = p.file_name() else { continue };
        // Only the `exe*.dat` archives contain ROMs; exe.dat itself is
        // the shared assets file and shouldn't be opened.
        if file_name == std::ffi::OsStr::new("exe.dat")
            || !file_name.to_string_lossy().starts_with("exe")
            || p.extension() != Some(std::ffi::OsStr::new("dat"))
        {
            continue;
        }
        roms.extend(scan_bnlc_rom_archive(&p));
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
