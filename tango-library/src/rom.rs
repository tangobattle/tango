use crate::scanner;
use crate::storage::{Listing, Storage};
use sha3::{Digest, Sha3_256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type GameRef = tango_gamesupport::GameRef;
pub type Scanner = scanner::Scanner<Catalog>;

/// Identity of the exact stored bytes, independent of filenames and game support.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct Id([u8; 32]);

impl Id {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha3_256::digest(bytes).into())
    }
}

#[derive(Clone)]
pub struct Image {
    pub bytes: Arc<Vec<u8>>,
    pub paths: BTreeSet<PathBuf>,
    /// Optional legacy presentation; package detection never depends on it.
    pub native_game: Option<GameRef>,
}

/// All discovered cartridge images, with a separate index for legacy callers.
/// Packages inspect `images`; a native registry miss never discards a file.
#[derive(Clone, Default)]
pub struct Catalog {
    images: BTreeMap<Id, Image>,
    native: HashMap<GameRef, Id>,
}

impl Catalog {
    pub fn insert(&mut self, bytes: Vec<u8>, path: Option<PathBuf>) -> Id {
        let id = Id::of(&bytes);
        let image = self.images.entry(id).or_insert_with(|| Image {
            bytes: Arc::new(bytes),
            paths: BTreeSet::new(),
            native_game: None,
        });
        if let Some(path) = path {
            image.paths.insert(path);
        }
        id
    }

    fn insert_detected(&mut self, bytes: Vec<u8>, path: PathBuf) {
        let id = self.insert(bytes.clone(), Some(path));
        // The native detector may restore trimmed padding. Packages still
        // receive the exact original bytes, including archived cartridges.
        let mut native_bytes = bytes;
        if let Some(game) = crate::game::detect(&mut native_bytes) {
            self.images.get_mut(&id).unwrap().native_game = Some(game);
            self.insert_native(game, native_bytes);
        }
    }

    fn insert_native(&mut self, game: GameRef, bytes: Vec<u8>) -> Id {
        let id = self.insert(bytes, None);
        self.images.get_mut(&id).unwrap().native_game = Some(game);
        self.native.insert(game, id);
        id
    }

    pub fn images(&self) -> impl Iterator<Item = (Id, &Image)> {
        self.images.iter().map(|(&id, image)| (id, image))
    }

    pub fn image(&self, id: Id) -> Option<&Image> {
        self.images.get(&id)
    }
    pub fn native_id(&self, game: GameRef) -> Option<Id> {
        self.native.get(&game).copied()
    }
    pub fn get(&self, game: &GameRef) -> Option<&Vec<u8>> {
        self.image(self.native_id(*game)?).map(|image| image.bytes.as_ref())
    }
    pub fn contains_key(&self, game: &GameRef) -> bool {
        self.native.contains_key(game)
    }
    pub fn len(&self) -> usize {
        self.images.len()
    }
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }
}

impl FromIterator<(GameRef, Vec<u8>)> for Catalog {
    fn from_iter<T: IntoIterator<Item = (GameRef, Vec<u8>)>>(iter: T) -> Self {
        let mut catalog = Self::default();
        for (game, bytes) in iter {
            catalog.insert_native(game, bytes);
        }
        catalog
    }
}

impl<const N: usize> From<[(GameRef, Vec<u8>); N]> for Catalog {
    fn from(images: [(GameRef, Vec<u8>); N]) -> Self {
        images.into_iter().collect()
    }
}

/// Scan fingerprints include native archive sources as well as the user's ROMs.
pub fn scan_roots(roms_path: &Path) -> Vec<PathBuf> {
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

/// Discover exact files under the ROM directory and extracted legacy cartridges.
/// Archive paths in the fingerprint are not themselves cartridge images.
pub fn scan_roms(storage: &dyn Storage, listing: &Listing, roms_path: &Path) -> Catalog {
    let mut catalog = Catalog::default();
    #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
    for (path, bytes) in crate::bnlc::scan_steam_roms() {
        catalog.insert_detected(bytes, path);
    }
    scan_stored_roms(&mut catalog, storage, listing, roms_path);
    catalog
}

fn scan_stored_roms(catalog: &mut Catalog, storage: &dyn Storage, listing: &Listing, roms_path: &Path) {
    for entry in listing
        .entries()
        .iter()
        .filter(|entry| entry.path.starts_with(roms_path))
    {
        let bytes = match storage.read(&entry.path) {
            Ok(bytes) => bytes,
            Err(error) => {
                log::warn!("{}: {error}", entry.path.display());
                continue;
            }
        };
        catalog.insert_detected(bytes, entry.path.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_have_content_identity_and_share_storage_across_paths() {
        let mut catalog = Catalog::default();
        let id = catalog.insert(vec![1, 2, 3], Some("roms/a.custom".into()));
        let bytes = catalog.image(id).unwrap().bytes.clone();
        assert_eq!(catalog.insert(vec![1, 2, 3], Some("roms/b.custom".into())), id);
        assert!(Arc::ptr_eq(&bytes, &catalog.image(id).unwrap().bytes));
        assert_eq!(catalog.image(id).unwrap().paths.len(), 2);
        assert!(catalog.image(id).unwrap().native_game.is_none());
        assert_ne!(catalog.insert(vec![1, 2, 4], None), id);
        assert_eq!(catalog.len(), 2);
    }

    #[cfg(feature = "native")]
    #[tokio::test]
    async fn discovery_keeps_unknown_files_and_excludes_archive_fingerprints() {
        let root = tempfile::tempdir().unwrap();
        let roms = root.path().join("roms");
        let archive = root.path().join("archive");
        std::fs::create_dir(&roms).unwrap();
        std::fs::create_dir(&archive).unwrap();
        std::fs::write(roms.join("custom-image"), [1, 2, 3]).unwrap();
        std::fs::write(roms.join("custom-copy"), [1, 2, 3]).unwrap();
        std::fs::write(archive.join("source.dat"), [4, 5, 6]).unwrap();
        let storage = crate::storage::StdStorage;
        let listing = storage.list(&[roms.clone(), archive]).await;
        let mut catalog = Catalog::default();
        scan_stored_roms(&mut catalog, &storage, &listing, &roms);
        let id = Id::of(&[1, 2, 3]);
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.image(id).unwrap().paths.len(), 2);
        assert!(catalog.native.is_empty());
        std::fs::rename(roms.join("custom-image"), roms.join("renamed")).unwrap();
        let listing = storage.list(&[roms.clone()]).await;
        let mut renamed = Catalog::default();
        scan_stored_roms(&mut renamed, &storage, &listing, &roms);
        assert!(renamed.image(id).unwrap().paths.contains(&roms.join("renamed")));
    }
}
