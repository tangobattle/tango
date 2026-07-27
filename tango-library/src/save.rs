use crate::storage::{Listing, Storage};
use crate::{rom::GameRef, scanner};

pub struct ScannedSave {
    pub path: std::path::PathBuf,
    pub save: tango_gamesupport::BoxedSave,
}

impl Clone for ScannedSave {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            save: self.save.clone_box(),
        }
    }
}

pub type Scanner = scanner::Scanner<std::collections::HashMap<GameRef, Vec<ScannedSave>>>;

pub fn scan_saves(storage: &dyn Storage, listing: &Listing) -> std::collections::HashMap<GameRef, Vec<ScannedSave>> {
    let mut by_game: std::collections::HashMap<GameRef, Vec<ScannedSave>> = std::collections::HashMap::new();

    for entry in listing.entries() {
        let buf = match storage.read(&entry.path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("{}: {e}", entry.path.display());
                continue;
            }
        };

        let mut matched = false;
        for game in crate::game::GAMES.iter().copied() {
            if let Ok(save) = game.parse_save(&buf) {
                by_game.entry(game).or_default().push(ScannedSave {
                    path: entry.path.clone(),
                    save,
                });
                matched = true;
            }
        }

        if !matched {
            log::warn!("save scan: {}: no matching game", entry.path.display());
        }
    }

    for (_, saves) in by_game.iter_mut() {
        // Order by extensionless name (full path as the tiebreak), the
        // same way the save picker displays rows — consumers take the
        // first entry as a default pick, and that should agree with
        // what the picker shows first.
        saves.sort_by(|a, b| {
            a.path
                .file_stem()
                .cmp(&b.path.file_stem())
                .then_with(|| a.path.cmp(&b.path))
        });
    }

    by_game
}
