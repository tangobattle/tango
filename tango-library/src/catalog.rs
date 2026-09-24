//! Everything the library folders hold, scanned: the one bundle both
//! hosts keep and hand to their screens and session launchers.
//!
//! The individual ROM, save, patch, and replay scanners live beside the
//! formats they index. A [`Catalog`] groups them, runs the rescan
//! pipeline over a [`Storage`], and builds the preparation helpers that
//! read from them, so neither host assembles those by hand.

use crate::config::Config;
use crate::storage::{Listing, Storage};
use crate::{game, loadout, patch, replays, rom, save};
use tango_net_protocol::control as protocol;

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

    /// The library facts the lobby's compatibility verdict needs for
    /// `local` and `remote`'s settings: whether the opponent's game is
    /// one we own a ROM for, whether both sides' patches carry the same
    /// compatibility tag, and the first patch either side named that
    /// isn't installed here.
    pub fn compatibility_facts(
        &self,
        local: &protocol::Settings,
        remote: &protocol::Settings,
    ) -> tango_net_protocol::compat::Facts {
        let patches = self.patches.read();
        let resolve = |info: &protocol::GameInfo| {
            game::find_by_family_and_variant(&info.family_and_variant.0, info.family_and_variant.1)
        };
        let tag = |info: &protocol::GameInfo| {
            patches.tag(
                resolve(info)?,
                info.patch.as_ref().map(|p| (p.name.as_str(), &p.version)),
            )
        };
        let local_tag = local.game_info.as_ref().and_then(tag);
        let remote_tag = remote.game_info.as_ref().and_then(tag);
        tango_net_protocol::compat::Facts {
            remote_rom_available: remote
                .game_info
                .as_ref()
                .and_then(resolve)
                .is_some_and(|game| self.roms.read().contains_key(&game)),
            matching_tags: local_tag.is_some() && local_tag == remote_tag,
            missing_patch: [local, remote]
                .into_iter()
                .filter_map(|s| s.game_info.as_ref()?.patch.as_ref())
                .find(|p| !patches.is_installed(&p.name, &p.version))
                .map(|p| (p.name.clone(), p.version.clone())),
        }
    }

    /// Open the recording at `path` with the exact games and patched
    /// ROMs it ran; see [`replays::open`].
    pub fn open_replay(
        &self,
        storage: &dyn Storage,
        config: &Config,
        path: &std::path::Path,
    ) -> Result<(tango_replay::Replay, replays::ResolvedRoms), replays::OpenError> {
        replays::open(storage, &self.roms, &config.patches_path(), path)
    }
}
