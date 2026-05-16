use crate::scanner;

pub type GameRef = &'static (dyn tango_gamedb::Game + Send + Sync);
pub type Scanner = scanner::Scanner<std::collections::HashMap<GameRef, Vec<u8>>>;

pub fn scan_roms(path: &std::path::Path) -> std::collections::HashMap<GameRef, Vec<u8>> {
    let mut roms = std::collections::HashMap::new();
    if std::fs::metadata(path).is_err() {
        return roms;
    }
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
