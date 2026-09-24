//! Pick and resolve launch inputs independently of UI, devices, and
//! session scheduling: the selection policy both hosts share, and the
//! preparation that turns a selection into a bootable ROM and save.
use crate::{config::Config, game, patch, rom, Catalog};
use std::path::{Path, PathBuf};
use tango_net_protocol::control as protocol;

mod resolve;
pub use resolve::{PreparedMatch, ResolvedLoadout, Resolver};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no game selected")]
    MissingGame,
    #[error("no save selected")]
    MissingSave,
    #[error("unknown game {0} v{1}")]
    UnknownGame(String, u8),
    #[error("match settings missing game information")]
    MissingGameInfo,
    #[error("simulation version does not match for {0}")]
    SimulationVersion(String),
    #[error("replay has no player {0}")]
    MissingReplaySeat(u8),
    #[error("replay player {player}: {source}")]
    ReplaySeat {
        player: u8,
        #[source]
        source: game::ReplaySideError,
    },
    #[error(transparent)]
    Rom(#[from] rom::LoadError),
    #[error(transparent)]
    Save(#[from] tango_gamesupport::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// What the user is bringing to a session: a game family, the concrete
/// game and save picked within it, and an optional patch overlay.
///
/// The *identity* of a selection is `(family, game, save)`. The patch is
/// deliberately not part of it: it's an overlay, selectable per
/// selection and remembered per save ([`Config::last_patch_per_save`]).
/// Picking a save restores the patch it was last used with; picking a
/// patch sticks to the current save. Saves whose patch association is
/// intrinsic (created from a patch's save template) keep it
/// automatically; vanilla-compatible saves just remember whatever they
/// last ran under.
///
/// Fields change only through the methods below, which keep game, save,
/// and patch consistent with each other and with the catalog.
#[derive(Clone, PartialEq, Default)]
pub struct Selection {
    /// The family picker's value. The concrete `game` is resolved from
    /// whichever save is chosen.
    family: Option<&'static str>,
    game: Option<rom::GameRef>,
    save: Option<PathBuf>,
    patch: Option<String>,
    /// `None` with a patch named when no version of it resolves; such a
    /// selection is never [`patch_ready`](Self::patch_ready).
    patch_version: Option<semver::Version>,
}

impl Selection {
    pub fn family(&self) -> Option<&'static str> {
        self.family
    }

    pub fn game(&self) -> Option<rom::GameRef> {
        self.game
    }

    pub fn save(&self) -> Option<&Path> {
        self.save.as_deref()
    }

    pub fn patch_name(&self) -> Option<&str> {
        self.patch.as_deref()
    }

    pub fn patch_version(&self) -> Option<&semver::Version> {
        self.patch_version.as_ref()
    }

    /// The patch overlay, when both its name and version are chosen.
    pub fn patch(&self) -> Option<(&str, &semver::Version)> {
        self.patch.as_deref().zip(self.patch_version.as_ref())
    }

    pub fn is_playable(&self) -> bool {
        self.game.is_some() && self.save.is_some()
    }

    pub fn game_info(&self) -> Option<protocol::GameInfo> {
        let game = self.game?;
        Some(protocol::GameInfo {
            family_and_variant: (game.family.id.to_owned(), game.variant),
            patch: self.patch().map(|(name, version)| protocol::PatchInfo {
                name: name.to_owned(),
                version: version.clone(),
            }),
            sim_version: game.pvp.sim_version(),
        })
    }

    /// The selection a peer committed to, with no save of ours attached.
    pub fn from_game_info(info: &protocol::GameInfo) -> Result<Self, Error> {
        let (family, variant) = &info.family_and_variant;
        let game = game::find_by_family_and_variant(family, *variant)
            .ok_or_else(|| Error::UnknownGame(family.clone(), *variant))?;
        if game.pvp.sim_version() != info.sim_version {
            return Err(Error::SimulationVersion(family.clone()));
        }
        Ok(Self {
            family: Some(game.family_and_variant().0),
            game: Some(game),
            save: None,
            patch: info.patch.as_ref().map(|p| p.name.clone()),
            patch_version: info.patch.as_ref().map(|p| p.version.clone()),
        })
    }

    /// Bring back the selection [`persist`](Self::persist) recorded. The
    /// family needs no scan and is restored whenever none is picked yet;
    /// the game, its save, and that save's patch overlay each have to
    /// still be in `catalog` to come back, so a host calls this again
    /// once its first scan lands.
    pub fn restore(&mut self, config: &Config, catalog: &Catalog) {
        if self.family.is_none() {
            // Falls back to the family of `last_game` for configs
            // written before `last_family` existed.
            self.family = config
                .last_family
                .as_deref()
                .and_then(game::family_static)
                .or_else(|| config.last_game.as_ref().and_then(|(f, _)| game::family_static(f)));
        }
        let Some((family, variant)) = config.last_game.as_ref() else {
            return;
        };
        let Some(game) = game::find_by_family_and_variant(family, *variant) else {
            return;
        };
        if !catalog.roms.read().contains_key(&game) {
            return;
        }
        self.game = Some(game);
        self.family = Some(game.family_and_variant().0);
        let Some(rel) = config.last_save_per_family.get(game.family_and_variant().0) else {
            return;
        };
        let abs = config.data_relative_to_absolute(rel);
        if !has_save(catalog, game, &abs) {
            return;
        }
        self.save = Some(abs);
        // The patch overlay hangs off the save — restore whatever this
        // save was last used with, if the patch still exists and
        // supports the variant.
        if let Some(Some((name, version))) = config.last_patch_per_save.get(rel) {
            if catalog.patches.read().supported_games(name, version).contains(&game) {
                self.patch = Some(name.clone());
                self.patch_version = Some(version.clone());
            }
        }
    }

    /// Record the selection in `config` so the next launch restores it.
    /// The save is remembered per family, and the patch overlay per save
    /// — so every save carries the patch it was last used with,
    /// including the patch a template-created save was born under.
    pub fn persist(&self, config: &mut Config) {
        config.last_family = self.family.map(str::to_owned);
        config.last_game = self
            .game
            .map(|g| (g.family_and_variant().0.to_owned(), g.family_and_variant().1));
        if let (Some(game), Some(path)) = (self.game, self.save.as_ref()) {
            if let Some(rel) = config.data_relative_string(path) {
                config
                    .last_save_per_family
                    .insert(game.family_and_variant().0.to_owned(), rel.clone());
                let overlay = self.patch().map(|(name, version)| (name.to_owned(), version.clone()));
                config.last_patch_per_save.insert(rel, overlay);
            }
        }
    }

    /// Switch families, landing on the family's remembered (or first
    /// available) save, which also fixes the concrete game.
    pub fn pick_family(&mut self, family: &'static str, catalog: &Catalog, config: &Config) {
        self.family = Some(family);
        match resolve_family_save(config, catalog, family) {
            Some((game, path)) => {
                self.game = Some(game);
                self.save = Some(path);
            }
            None => {
                self.game = None;
                self.save = None;
            }
        }
        // A family switch resets the overlay baseline; the landed save's
        // own patch memory then has the last word.
        self.patch = None;
        self.patch_version = None;
        self.restore_patch_memory(config, catalog);
    }

    /// Switch to one concrete game, landing on its remembered (or first)
    /// save and that save's patch memory — for a host whose picker lists
    /// games rather than families.
    pub fn pick_game(&mut self, game: rom::GameRef, catalog: &Catalog, config: &Config) {
        self.family = Some(game.family_and_variant().0);
        self.game = Some(game);
        self.save = remembered_save_for_game(config, catalog, game);
        self.restore_patch_memory(config, catalog);
    }

    /// Pick a save, which carries the concrete game it resolves to. The
    /// patch follows the save: its remembered overlay applies, and only
    /// saves with no memory inherit the current patch (kept only if it
    /// supports the new variant). Also the programmatic pick after a
    /// delete, with the same semantics.
    pub fn pick_save(&mut self, game: rom::GameRef, path: PathBuf, catalog: &Catalog, config: &Config) {
        self.game = Some(game);
        self.family = Some(game.family_and_variant().0);
        self.save = Some(path);
        self.restore_patch_memory(config, catalog);
    }

    /// Pick a patch by name (`None` for no patch), at its newest version
    /// supporting the current game.
    ///
    /// A patch the current save can't run can still be picked. The
    /// actively-chosen patch wins: the save (and the game it resolved
    /// to) is deselected rather than the patch, and the version is then
    /// picked unconstrained by the dropped game.
    pub fn pick_patch(&mut self, name: Option<String>, catalog: &Catalog, config: &Config) {
        match name {
            None => {
                self.patch = None;
                self.patch_version = None;
            }
            Some(name) => {
                let patches = catalog.patches.read();
                let mut version = patches.newest_version(&name, self.game);
                if self.game.is_some() && version.is_none() {
                    self.save = None;
                    self.game = None;
                    version = patches.newest_version(&name, None);
                }
                self.patch_version = version;
                self.patch = Some(name);
            }
        }
        // With no save selected yet, land on the game's remembered/first
        // save (without applying that save's patch memory — the user just
        // picked this patch explicitly; it'll be recorded for the save
        // instead).
        if self.save.is_none() {
            if let Some(game) = self.game {
                self.save = remembered_save_for_game(config, catalog, game);
            }
        }
    }

    /// Pick a version of the current patch. Hosts offer only versions
    /// supporting the current game, so nothing else needs fixing up.
    pub fn pick_patch_version(&mut self, version: semver::Version) {
        self.patch_version = Some(version);
    }

    /// Adopt a save just created for `game` from a template. Templates
    /// are only offered for patch-supported variants, so the patch
    /// normally still applies; it's dropped only if it somehow doesn't
    /// support the created variant. The save's own patch memory is not
    /// consulted: it's new, and [`persist`](Self::persist) records the
    /// patch it was born under.
    pub fn adopt_created_save(&mut self, game: rom::GameRef, path: PathBuf, catalog: &Catalog) {
        if !self.patch_supports(catalog, game) {
            self.patch = None;
            self.patch_version = None;
        }
        self.game = Some(game);
        self.family = Some(game.family_and_variant().0);
        self.save = Some(path);
    }

    /// The selected save's file was renamed, or duplicated and the copy
    /// is taking over: point at `path`, keeping the game and patch.
    pub fn follow_save_file(&mut self, path: PathBuf) {
        self.save = Some(path);
    }

    /// Drop the save — deleted, or gone from the latest scan — keeping
    /// the family, game, and patch.
    pub fn clear_save(&mut self) {
        self.save = None;
    }

    /// With no save selected, land on the first available save anywhere
    /// in the family (a sibling variant is fine), fixing the game to
    /// whatever that save resolves to.
    pub fn pick_first_family_save(&mut self, catalog: &Catalog, config: &Config) {
        if self.save.is_some() {
            return;
        }
        if let Some((game, path)) = self
            .family
            .and_then(|family| first_available_family_save(catalog, family))
        {
            self.pick_save(game, path, catalog, config);
        }
    }

    /// Re-validate after a rescan, for a host with no download-on-pick
    /// flow: a game whose ROM is gone clears everything, a save that's
    /// gone falls back to the game's remembered (or first) save, and a
    /// patch that isn't installed is dropped.
    pub fn reconcile(&mut self, catalog: &Catalog, config: &Config) {
        let Some(game) = self.game.filter(|game| catalog.roms.read().contains_key(game)) else {
            *self = Self::default();
            return;
        };
        self.family = Some(game.family_and_variant().0);
        if !self.save.as_ref().is_some_and(|path| has_save(catalog, game, path)) {
            self.save = remembered_save_for_game(config, catalog, game);
            self.restore_patch_memory(config, catalog);
        }
        let installed = match self.patch() {
            Some((name, version)) => catalog.patches.read().is_installed(name, version),
            None => self.patch.is_none(),
        };
        if !installed {
            self.patch = None;
            self.patch_version = None;
        }
    }

    /// The games the selected patch version supports, or `None` when no
    /// patch (or no version) is selected — meaning "don't filter".
    pub fn patch_supported_games(&self, catalog: &Catalog) -> Option<std::collections::HashSet<rom::GameRef>> {
        let (name, version) = self.patch()?;
        let games = catalog.patches.read().supported_games(name, version);
        (!games.is_empty()).then_some(games)
    }

    /// Whether the selected patch version supports `game`. True when no
    /// patch (or no version) is selected — there's nothing for the save
    /// to be incompatible with.
    pub fn patch_supports(&self, catalog: &Catalog, game: rom::GameRef) -> bool {
        self.patch_supported_games(catalog)
            .map(|games| games.contains(&game))
            .unwrap_or(true)
    }

    /// Whether the selected patch is actually installed.
    ///
    /// False while its package is downloading, after a failed download,
    /// and for a patch with no resolvable version — in every one of those
    /// the session would run unpatched, so the entry points stay shut
    /// until it lands. `true` when no patch is selected, which is a valid
    /// setup.
    pub fn patch_ready(&self, catalog: &Catalog) -> bool {
        match (&self.patch, &self.patch_version) {
            (None, _) => true,
            (Some(name), Some(version)) => catalog.patches.read().is_installed(name, version),
            (Some(_), None) => false,
        }
    }

    /// Apply the selected save's remembered patch overlay:
    /// * recorded patch → restore it (if it still exists and supports
    ///   the save's variant);
    /// * recorded "explicitly unpatched" → clear the patch;
    /// * no record (brand-new save) → keep the current patch if it
    ///   supports the new variant, else clear it.
    fn restore_patch_memory(&mut self, config: &Config, catalog: &Catalog) {
        let rel = self.save.as_ref().and_then(|p| config.data_relative_string(p));
        match rel.and_then(|r| config.last_patch_per_save.get(&r).cloned()) {
            Some(Some((name, version))) => {
                let supported = self
                    .game
                    .is_some_and(|g| catalog.patches.read().supported_games(&name, &version).contains(&g));
                if supported {
                    self.patch = Some(name);
                    self.patch_version = Some(version);
                } else {
                    self.retain_patch_for_game(catalog);
                }
            }
            Some(None) => {
                self.patch = None;
                self.patch_version = None;
            }
            None => self.retain_patch_for_game(catalog),
        }
    }

    /// Keep the active patch only if it can run the current game: prefer
    /// the already-selected version, fall back to the newest version that
    /// supports the variant, clear the patch entirely when none does.
    fn retain_patch_for_game(&mut self, catalog: &Catalog) {
        let (Some(name), Some(game)) = (self.patch.clone(), self.game) else {
            return;
        };
        let patches = catalog.patches.read();
        let current_ok = self
            .patch_version
            .as_ref()
            .is_some_and(|v| patches.supported_games(&name, v).contains(&game));
        if current_ok {
            return;
        }
        match patches.newest_version(&name, Some(game)) {
            Some(version) => self.patch_version = Some(version),
            None => {
                self.patch = None;
                self.patch_version = None;
            }
        }
    }
}

fn has_save(catalog: &Catalog, game: rom::GameRef, path: &Path) -> bool {
    catalog
        .saves
        .read()
        .get(&game)
        .is_some_and(|saves| saves.iter().any(|s| s.path == path))
}

/// The remembered save for `game` if it's still in the scan, otherwise
/// the first save listed for it.
fn remembered_save_for_game(config: &Config, catalog: &Catalog, game: rom::GameRef) -> Option<PathBuf> {
    let saves = catalog.saves.read();
    let saves_for_game = saves.get(&game);
    let remembered = config
        .last_save_per_family
        .get(game.family_and_variant().0)
        .map(|rel| config.data_relative_to_absolute(rel))
        .filter(|p| saves_for_game.is_some_and(|v| v.iter().any(|s| s.path == *p)));
    remembered.or_else(|| saves_for_game.and_then(|v| v.first().map(|s| s.path.clone())))
}

/// Pick the (game, save) to land on after a *family* selection. Prefers
/// the remembered save of any owned-ROM game in the family; otherwise
/// the first available save. Saves whose ROM isn't owned are never
/// auto-selected.
fn resolve_family_save(config: &Config, catalog: &Catalog, family: &str) -> Option<(rom::GameRef, PathBuf)> {
    // One remembered save for the family, and it names the version:
    // whichever of the family's owned games actually has that save is
    // the game to land on.
    if let Some(rel) = config.last_save_per_family.get(family) {
        let roms = catalog.roms.read();
        let abs = config.data_relative_to_absolute(rel);
        for game in game::games_in_family(family) {
            if roms.contains_key(&game) && has_save(catalog, game, &abs) {
                return Some((game, abs));
            }
        }
    }
    first_available_family_save(catalog, family)
}

/// First owned-ROM save across every game in `family`, ordered by
/// extensionless name like the picker, with the concrete game alongside
/// so callers needn't re-sniff the save.
fn first_available_family_save(catalog: &Catalog, family: &str) -> Option<(rom::GameRef, PathBuf)> {
    let roms = catalog.roms.read();
    let saves = catalog.saves.read();
    let mut candidates: Vec<(rom::GameRef, PathBuf)> = Vec::new();
    for game in game::games_in_family(family) {
        if !roms.contains_key(&game) {
            continue;
        }
        if let Some(v) = saves.get(&game) {
            candidates.extend(v.iter().map(|s| (game, s.path.clone())));
        }
    }
    candidates.sort_by(|a, b| a.1.file_stem().cmp(&b.1.file_stem()).then_with(|| a.1.cmp(&b.1)));
    candidates.into_iter().next()
}

/// Patch names the picker offers for `games`: every patch, installed or
/// merely indexed, with a version supporting any of them. Favorites sort
/// first, alphabetical within each group.
pub fn patch_names_for(
    patches: &patch::Catalog,
    games: &[rom::GameRef],
    favorites: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut names: Vec<String> = patches
        .names()
        .into_iter()
        .filter(|name| {
            patches
                .versions(name)
                .keys()
                .any(|v| games.iter().any(|g| patches.supported_games(name, v).contains(g)))
        })
        .map(str::to_owned)
        .collect();
    names.sort_by(|a, b| favorites.contains(b).cmp(&favorites.contains(a)).then_with(|| a.cmp(b)));
    names
}

#[cfg(test)]
mod tests;
