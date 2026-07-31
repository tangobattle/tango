//! What you're about to play with: a game, a save, and optionally a
//! patch. The single piece of state both screens read — the play button
//! boots it, and the lobby advertises it — so it is what the settings
//! packet and the compatibility check are built from.

use std::path::PathBuf;

use tango_library::rom::GameRef;
use tango_net_protocol::control as protocol;

#[derive(Clone, PartialEq, Default)]
pub struct Loadout {
    pub game: Option<GameRef>,
    /// Which of the game's saves, by the path the scanner found it at.
    /// A path rather than the bytes so a save edited elsewhere in the
    /// session is picked up on the next read.
    pub save_path: Option<PathBuf>,
    pub patch: Option<(String, semver::Version)>,
}

impl Loadout {
    /// Ready to play: a game we have a ROM for and a save to run it
    /// with. A patch pick is optional, but a *broken* one isn't — a
    /// missing package surfaces when the ROM is built, not here.
    pub fn is_playable(&self) -> bool {
        self.game.is_some() && self.save_path.is_some()
    }

    /// The savedata as it stands on "disk".
    pub fn save_bytes(&self) -> Option<Vec<u8>> {
        let path = self.save_path.as_ref()?;
        crate::library::with(|library| {
            use tango_library::storage::Storage as _;
            library.files.read(path).ok()
        })?
    }

    /// The ROM image to run: the stored dump with the patch applied.
    pub fn rom(&self) -> Result<Vec<u8>, String> {
        let game = self.game.ok_or_else(|| "no game selected".to_string())?;
        crate::library::patched_rom(game, self.patch.as_ref())
    }

    /// What we tell the peer we're bringing. The peer resolves this
    /// against their own catalog — which is why the patch travels as
    /// `(name, version)` rather than anything derived: both sides must
    /// arrive at the same [`tango_patch::Tag`] independently.
    pub fn game_info(&self) -> Option<protocol::GameInfo> {
        let game = self.game?;
        let (family, variant) = game.family_and_variant();
        Some(protocol::GameInfo {
            family_and_variant: (family.to_string(), variant),
            patch: self.patch.as_ref().map(|(name, version)| protocol::PatchInfo {
                name: name.clone(),
                version: version.clone(),
            }),
            sim_version: game.family.sim_version,
        })
    }

    /// Drop a pick that the library no longer backs — the game's ROM
    /// was deleted, the save it named is gone, the patch was
    /// uninstalled. Called after every rescan so the UI never offers a
    /// play button that would fail on press.
    pub fn reconcile(&mut self) {
        let Some(game) = self.game else {
            *self = Self::default();
            return;
        };
        let known = crate::library::with(|library| {
            let has_rom = library.roms.read().contains_key(&game);
            let saves: Vec<PathBuf> = library
                .saves
                .read()
                .get(&game)
                .map(|saves| saves.iter().map(|s| s.path.clone()).collect())
                .unwrap_or_default();
            let patch_ok = self
                .patch
                .as_ref()
                .map(|(name, version)| library.patches.read().is_installed(name, version))
                .unwrap_or(true);
            (has_rom, saves, patch_ok)
        });
        let Some((has_rom, saves, patch_ok)) = known else {
            return;
        };
        if !has_rom {
            *self = Self::default();
            return;
        }
        if !patch_ok {
            self.patch = None;
        }
        // Default to the first save rather than leaving the pick empty:
        // one save per game is the common case on a phone, and making
        // the user pick it every time is friction for nothing.
        if !self.save_path.as_ref().is_some_and(|p| saves.contains(p)) {
            self.save_path = saves.into_iter().next();
        }
    }
}
