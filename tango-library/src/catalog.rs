//! Everything the library folders hold, scanned: the one bundle both
//! hosts keep and hand to their screens and session launchers.
//!
//! The individual ROM, save, patch, and replay scanners live beside the
//! formats they index. A [`Catalog`] groups them, runs the rescan
//! pipeline over a [`Storage`], and builds the preparation helpers that
//! read from them, so neither host assembles those by hand.

use crate::config::Config;
use crate::storage::{Listing, Storage};
use crate::{loadout, patch, replays, rom, save};

/// The scanned library. Cloning shares the same scanners.
#[derive(Clone)]
pub struct Catalog {
    pub roms: rom::Scanner,
    pub saves: save::Scanner,
    pub patches: patch::Scanner,
    pub replays: replays::Scanner,
}

impl Default for Catalog {
    fn default() -> Self {
        Self::new()
    }
}

/// One [`Listing`] per scanner, gathered before any file is parsed.
pub struct Listings {
    pub roms: Listing,
    pub saves: Listing,
    pub patches: Listing,
    pub replays: Listing,
}

impl Catalog {
    pub fn new() -> Self {
        Self {
            roms: rom::Scanner::new(),
            saves: save::Scanner::new(),
            patches: patch::Scanner::new(),
            replays: replays::Scanner::new(),
        }
    }

    /// What the four scans read, enumerated but not yet read: the cheap
    /// half of a rescan, and the only asynchronous one. See
    /// [`Catalog::rescan`].
    pub async fn list(storage: &dyn Storage, config: &Config) -> Listings {
        Listings {
            roms: storage.list(&rom::scan_roots(&config.roms_path())).await,
            saves: storage.list(&[config.saves_path()]).await,
            patches: storage.list(&patch::scan_roots(&config.patches_path())).await,
            replays: storage.list(&[config.replays_path()]).await,
        }
    }

    /// Rescan all four collections from an already-gathered [`Listings`].
    /// Each scanner is gated on its listing, so automatic triggers skip the
    /// full read-and-parse unless files actually changed.
    pub fn rescan(&self, storage: &dyn Storage, config: &Config, listings: &Listings) {
        self.rescan_library(storage, config, listings);
        self.rescan_replays(storage, &listings.replays);
    }

    /// Everything a loadout is picked from. This is separate from the
    /// replay index so startup can show the main screen without waiting on a
    /// replay collection that can grow without bound.
    pub fn rescan_library(&self, storage: &dyn Storage, config: &Config, listings: &Listings) {
        let patches_path = config.patches_path();
        let start = web_time::Instant::now();
        self.roms
            .rescan_if_changed(&listings.roms, || Some(rom::scan_roms(storage, &listings.roms)));
        let roms = start.elapsed();
        self.saves
            .rescan_if_changed(&listings.saves, || Some(save::scan_saves(storage, &listings.saves)));
        let saves = start.elapsed() - roms;
        self.patches.rescan_if_changed(&listings.patches, || {
            match patch::scan(storage, &patches_path, &listings.patches) {
                Ok(catalog) => Some(catalog),
                Err(e) => {
                    log::warn!("patch scan failed: {e}");
                    None
                }
            }
        });
        let patches = start.elapsed() - roms - saves;
        log::debug!("rescan: roms {roms:.1?}, saves {saves:.1?}, patches {patches:.1?}");
    }

    /// Refresh the replay index from its own listing.
    pub fn rescan_replays(&self, storage: &dyn Storage, listing: &Listing) {
        let start = web_time::Instant::now();
        self.replays
            .rescan_if_changed(listing, || Some(replays::scan_replays(storage, listing)));
        log::debug!("rescan: replays {:.1?}", start.elapsed());
    }

    /// Launch preparation over this catalog's ROMs and patches.
    pub fn resolver<'a>(&'a self, storage: &'a dyn Storage, config: &Config) -> loadout::Resolver<'a> {
        loadout::Resolver {
            storage,
            roms: &self.roms,
            patches: &self.patches,
            patches_path: config.patches_path(),
        }
    }

    /// The exact games and patched ROMs a recording ran; see
    /// [`replays::resolve_roms`].
    pub fn resolve_replay_roms(
        &self,
        storage: &dyn Storage,
        config: &Config,
        metadata: &tango_replay::Metadata,
    ) -> Result<replays::ResolvedRoms, replays::ResolveError> {
        replays::resolve_roms(storage, &self.roms, &config.patches_path(), metadata)
    }
}
