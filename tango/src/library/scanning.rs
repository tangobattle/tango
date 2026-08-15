//! Native library scan coordination.
//!
//! The individual ROM, save, patch, and replay scanners live beside the
//! formats they index. `Scanners` groups them into the catalog snapshot the
//! native frontend shares across its tabs and sessions. Keeping that catalog
//! in the library layer prevents those consumers from depending on the app
//! shell merely to read indexed content.

use crate::config;
use tango_library::storage::Listing;

use super::{patch, replays, rom, save};

#[derive(Clone)]
pub(crate) struct Scanners {
    pub(crate) roms: rom::Scanner,
    pub(crate) saves: save::Scanner,
    pub(crate) patches: patch::Scanner,
    pub(crate) replays: replays::Scanner,
}

impl Scanners {
    pub(crate) fn new() -> Self {
        Self {
            roms: rom::Scanner::new(),
            saves: save::Scanner::new(),
            patches: patch::Scanner::new(),
            replays: replays::Scanner::new(),
        }
    }

    /// What the four scans read, enumerated but not yet read: the cheap
    /// half of a rescan, and the only asynchronous one. See
    /// [`Scanners::rescan`].
    pub(crate) async fn list(config: &config::Config) -> Listings {
        let storage = super::storage();
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
    pub(crate) fn rescan(&self, config: &config::Config, listings: &Listings) {
        self.rescan_library(config, listings);
        self.rescan_replays(&listings.replays);
    }

    /// Everything the play tab is built from. This is separate from the
    /// replay index so startup can show the main screen without waiting on a
    /// replay collection that can grow without bound.
    pub(crate) fn rescan_library(&self, config: &config::Config, listings: &Listings) {
        let storage = super::storage();
        let patches_path = config.patches_path();
        let start = std::time::Instant::now();
        self.roms
            .rescan_if_changed(&listings.roms, || Some(rom::scan_roms(storage, &listings.roms)));
        let roms = start.elapsed();
        self.saves
            .rescan_if_changed(&listings.saves, || Some(save::scan_saves(storage, &listings.saves)));
        let saves = start.elapsed() - roms;
        self.patches.rescan_if_changed(&listings.patches, || {
            patch::scan(storage, &patches_path, &listings.patches).ok()
        });
        let patches = start.elapsed() - roms - saves;
        log::debug!("rescan: roms {roms:.1?}, saves {saves:.1?}, patches {patches:.1?}");
    }

    /// Refresh the replay index from its own listing.
    pub(crate) fn rescan_replays(&self, listing: &Listing) {
        let storage = super::storage();
        let start = std::time::Instant::now();
        self.replays
            .rescan_if_changed(listing, || Some(replays::scan_replays(storage, listing)));
        log::debug!("rescan: replays {:.1?}", start.elapsed());
    }
}

/// One [`Listing`] per scanner, gathered before any file is parsed.
pub(crate) struct Listings {
    pub(crate) roms: Listing,
    pub(crate) saves: Listing,
    pub(crate) patches: Listing,
    pub(crate) replays: Listing,
}
