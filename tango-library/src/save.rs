use crate::storage::{Listing, Storage};
use crate::{rom::GameRef, scanner};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

/// Exact files are available to packages, independently of native save parsing.
#[derive(Clone, Default)]
pub struct Catalog {
    files: BTreeMap<PathBuf, Arc<Vec<u8>>>,
    native: HashMap<GameRef, Vec<ScannedSave>>,
}

impl Catalog {
    pub fn insert(&mut self, path: PathBuf, bytes: Vec<u8>) {
        self.files.insert(path, Arc::new(bytes));
    }

    pub fn files(&self) -> &BTreeMap<PathBuf, Arc<Vec<u8>>> {
        &self.files
    }

    pub fn file(&self, path: &Path) -> Option<&Arc<Vec<u8>>> {
        self.files.get(path)
    }

    pub fn get(&self, game: &GameRef) -> Option<&Vec<ScannedSave>> {
        self.native.get(game)
    }
}

pub type Scanner = scanner::Scanner<Catalog>;

pub fn scan_saves(storage: &dyn Storage, listing: &Listing) -> Catalog {
    let mut catalog = Catalog::default();

    for entry in listing.entries() {
        let buf = match storage.read(&entry.path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("{}: {e}", entry.path.display());
                continue;
            }
        };

        for game in crate::game::GAMES.iter().copied() {
            if let Ok(save) = game.parse_save(&buf) {
                catalog.native.entry(game).or_default().push(ScannedSave {
                    path: entry.path.clone(),
                    save,
                });
            }
        }

        catalog.insert(entry.path.clone(), buf);
    }

    for (_, saves) in catalog.native.iter_mut() {
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

    catalog
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discovery_keeps_unknown_saves_and_refreshes_exact_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("custom-save");
        std::fs::write(&path, [1, 2, 3]).unwrap();
        let storage = crate::storage::StdStorage;
        let listing = storage.list(&[root.path().to_owned()]).await;
        let catalog = scan_saves(&storage, &listing);
        assert_eq!(catalog.file(&path).unwrap().as_slice(), [1, 2, 3]);
        assert!(catalog.native.is_empty());
        let cloned = catalog.clone();
        assert!(Arc::ptr_eq(catalog.file(&path).unwrap(), cloned.file(&path).unwrap()));
        std::fs::write(&path, [4, 5]).unwrap();
        let listing = storage.list(&[root.path().to_owned()]).await;
        assert_eq!(scan_saves(&storage, &listing).file(&path).unwrap().as_slice(), [4, 5]);
    }
}
