use crate::{rom::GameRef, scanner};

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

pub type Scanner = scanner::Scanner<std::collections::HashMap<GameRef, Vec<ScannedSave>>>;

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
                by_game.entry(game).or_default().push(ScannedSave {
                    path: p.to_path_buf(),
                    save,
                });
                matched = true;
            }
        }

        if !matched {
            log::warn!("save scan: {}: no matching game", p.display());
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
