//! Save discovery, copied from `tango/src/save.rs` (minus the `Scanner`
//! alias — see `rom.rs`).

use crate::rom::GameRef;

pub struct ScannedSave {
    pub path: std::path::PathBuf,
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
}

impl Clone for ScannedSave {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            save: self.save.clone_box(),
        }
    }
}

pub fn scan_saves(path: &std::path::Path) -> std::collections::HashMap<GameRef, Vec<ScannedSave>> {
    let mut by_game: std::collections::HashMap<GameRef, Vec<ScannedSave>> = std::collections::HashMap::new();

    if std::fs::metadata(path).is_err() {
        return by_game;
    }

    for entry in walkdir::WalkDir::new(path) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!("save scan: {e:?}");
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let buf = match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("{}: {e}", p.display());
                continue;
            }
        };

        let mut matched = false;
        for game in crate::game::GAMES.iter().copied() {
            if let Ok(save) = game.parse_save(&buf) {
                log::info!("save scan: {}: {:?}", p.display(), game.family_and_variant());
                by_game.entry(game).or_default().push(ScannedSave {
                    path: p.to_path_buf(),
                    save,
                });
                matched = true;
            }
        }

        if !matched {
            log::debug!("save scan: {}: no matching game", p.display());
        }
    }

    for (_, saves) in by_game.iter_mut() {
        saves.sort_by_key(|s| s.path.clone());
    }

    by_game
}
