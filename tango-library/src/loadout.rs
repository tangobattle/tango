//! Pick and resolve launch inputs independently of UI, devices, and
//! session scheduling: the selection policy both hosts share, and the
//! preparation that turns a selection into a bootable ROM and save.
use crate::{config::Config, game, patch, rom, storage::Storage, Catalog};
use std::path::{Path, PathBuf};
use tango_net_protocol::control as protocol;

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

/// Exact ROM, parsed save, patch overrides, and structured validation findings.
pub struct ResolvedLoadout {
    pub sram: Vec<u8>,
    pub rom: std::sync::Arc<Vec<u8>>,
    pub prepared: tango_gamesupport::PreparedSave,
    pub validation: Box<dyn tango_gamesupport::Validation>,
}

impl ResolvedLoadout {
    /// The SRAM image a linked match boots this seat from: the parsed
    /// save dumped back out in the game's own layout. It can differ from
    /// [`sram`](Self::sram), the bytes as supplied — a dump is sized to
    /// the cart's SRAM, re-masked, and for Battle Chip Challenge carries
    /// only the live file — and it is what matches have always booted.
    pub fn match_sram(&self) -> Vec<u8> {
        self.prepared.save.to_sram_dump()
    }
}

/// Both seats in local/remote order. Player assignment remains the session's job.
pub struct PreparedMatch {
    pub local: ResolvedLoadout,
    pub remote: ResolvedLoadout,
}

/// The catalog resources a preparation operation is allowed to read.
pub struct Resolver<'a> {
    pub storage: &'a dyn Storage,
    pub roms: &'a rom::Scanner,
    pub patches: &'a patch::Scanner,
    pub patches_path: PathBuf,
}
impl Resolver<'_> {
    pub fn resolve(&self, selection: &Selection, snapshot: Option<&[u8]>) -> Result<ResolvedLoadout, Error> {
        let game = selection.game.ok_or(Error::MissingGame)?;
        let bytes = match snapshot {
            Some(bytes) => bytes.to_vec(),
            None => self
                .storage
                .read(selection.save.as_deref().ok_or(Error::MissingSave)?)?,
        };
        let save = game.parse_save(&bytes)?;
        let rom = rom::load(self.storage, self.roms, &self.patches_path, game, selection.patch())?;
        let applied_patch = selection.patch().map(|patch| self.applied_patch(game, patch));
        let prepared = tango_gamesupport_common_dataview::model::prepare(
            game,
            &rom,
            selection.save.clone().unwrap_or_default(),
            save,
            applied_patch,
        );
        let validation = prepared.validate();
        Ok(ResolvedLoadout {
            sram: bytes,
            rom: std::sync::Arc::new(rom),
            prepared,
            validation,
        })
    }
    /// Prepare `save` for the save editor's preview, best-effort: a patch
    /// that fails to apply (not installed, or unreadable) falls back to
    /// the clean ROM, logged, so the save still renders. Simulation input
    /// preparation goes through [`resolve`](Self::resolve), which never
    /// substitutes a clean ROM.
    pub fn preview(
        &self,
        game: rom::GameRef,
        save_path: PathBuf,
        save: tango_gamesupport::BoxedSave,
        patch: Option<(&str, &semver::Version)>,
    ) -> Result<tango_gamesupport::PreparedSave, rom::LoadError> {
        let (rom, applied_patch) = match rom::load(self.storage, self.roms, &self.patches_path, game, patch) {
            Ok(rom) => (rom, patch.map(|patch| self.applied_patch(game, patch))),
            Err(e @ rom::LoadError::Patch { .. }) => {
                log::warn!("{e} for {:?}; previewing the clean ROM", game.family_and_variant());
                (
                    rom::load(self.storage, self.roms, &self.patches_path, game, None)?,
                    None,
                )
            }
            Err(e) => return Err(e),
        };
        Ok(tango_gamesupport_common_dataview::model::prepare(
            game,
            &rom,
            save_path,
            save,
            applied_patch,
        ))
    }

    /// [`preview`](Self::preview) one absolute player seat of a
    /// recording: that seat's game (checked against this build's
    /// simulation version) and recorded patch, and its embedded SRAM.
    pub fn preview_replay_seat(
        &self,
        replay: &tango_replay::Replay,
        player: u8,
    ) -> Result<tango_gamesupport::PreparedSave, Error> {
        let info = replay
            .metadata
            .side(player)
            .ok_or(Error::MissingReplaySeat(player))?
            .game_info
            .as_ref()
            .ok_or(Error::MissingGameInfo)?;
        let game = game::find_for_replay_side(info).map_err(|source| Error::ReplaySeat { player, source })?;
        let sram = replay
            .srams
            .get(player as usize)
            .ok_or(Error::MissingReplaySeat(player))?;
        let save = game.parse_save(sram)?;
        // A recorded version that doesn't parse previews unpatched.
        let patch = info
            .patch
            .as_ref()
            .and_then(|p| Some((p.name.as_str(), semver::Version::parse(&p.version).ok()?)));
        Ok(self.preview(
            game,
            PathBuf::new(),
            save,
            patch.as_ref().map(|(name, version)| (*name, version)),
        )?)
    }

    /// The patch record a prepared save carries, with the catalog's ROM
    /// overrides for `game`.
    fn applied_patch(
        &self,
        game: rom::GameRef,
        (name, version): (&str, &semver::Version),
    ) -> tango_gamesupport::AppliedPatch {
        tango_gamesupport::AppliedPatch {
            name: name.to_owned(),
            version: version.clone(),
            rom_overrides: self
                .patches
                .read()
                .version(name, version)
                .map(|meta| meta.rom_overrides_for(game))
                .unwrap_or_default(),
        }
    }

    pub fn prepare_match(
        &self,
        local: &protocol::Settings,
        remote: &protocol::Settings,
        saves: [&[u8]; 2],
    ) -> Result<PreparedMatch, Error> {
        Ok(PreparedMatch {
            local: self.resolve(
                &Selection::from_game_info(local.game_info.as_ref().ok_or(Error::MissingGameInfo)?)?,
                Some(saves[0]),
            )?,
            remote: self.resolve(
                &Selection::from_game_info(remote.game_info.as_ref().ok_or(Error::MissingGameInfo)?)?,
                Some(saves[1]),
            )?,
        })
    }
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

/// Resolve the library facts the lobby's compatibility verdict needs.
pub fn compatibility_facts(
    local: &protocol::Settings,
    remote: &protocol::Settings,
    roms: &std::collections::HashMap<rom::GameRef, Vec<u8>>,
    catalog: &patch::Catalog,
) -> tango_net_protocol::compat::Facts {
    let resolve = |info: &protocol::GameInfo| {
        game::find_by_family_and_variant(&info.family_and_variant.0, info.family_and_variant.1)
    };
    let tag = |info: &protocol::GameInfo| {
        catalog.tag(
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
            .is_some_and(|game| roms.contains_key(&game)),
        matching_tags: local_tag.is_some() && local_tag == remote_tag,
        missing_patch: [local, remote]
            .into_iter()
            .filter_map(|s| s.game_info.as_ref()?.patch.as_ref())
            .find(|p| !catalog.is_installed(&p.name, &p.version))
            .map(|p| (p.name.clone(), p.version.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Memory;
    use tango_gamesupport::{Family, Game, Region};
    struct NoEmulator;
    impl tango_match::Backend for NoEmulator {
        fn sim_version(&self) -> u32 {
            7
        }
        fn screen_layout(&self, _: tango_match::SessionMode) -> tango_match::ScreenLayout {
            panic!("preparation must not create a session")
        }
        fn keys_mask(&self) -> u32 {
            0
        }
        fn tps(&self) -> num_rational::Ratio<u32> {
            num_rational::Ratio::new(60, 1)
        }
        fn start(&self, _: tango_match::StartConfig) -> Result<tango_match::Match, tango_match::Error> {
            panic!("preparation must not boot an emulator")
        }
    }
    #[derive(Clone)]
    struct Save(Vec<u8>);
    impl tango_gamesupport_common_dataview::save::Save for Save {
        fn to_sram_dump(&self) -> Vec<u8> {
            self.0.clone()
        }
        fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
            self.0.as_slice().into()
        }
        fn rebuild_checksum(&mut self) {
            self.0[1] = self.0[0];
        }
        fn game_violations(
            &self,
            _: &dyn tango_gamesupport_common_dataview::rom::Assets,
        ) -> Option<Box<dyn std::any::Any + Send + Sync>> {
            (self.0[0] == 9).then(|| Box::new(vec!["invalid setup"]) as Box<dyn std::any::Any + Send + Sync>)
        }
    }
    static FAMILY: Family = Family {
        id: "test",
        games: &[&GAME, &OTHER_VARIANT],
        match_types: &[1],
        players_colored_by_seat: false,
        translations: &[],
    };
    static GAME: Game = Game {
        family: &FAMILY,
        variant: 0,
        rom_code: b"TEST",
        revision: 0,
        crc32: 0,
        rom_size: 3,
        region: Region::US,
        parse_save_fn: |bytes| {
            if bytes.len() != 2 {
                return Err(tango_gamesupport::Error::IncompatibleSave);
            }
            Ok(tango_gamesupport_common_dataview::wrap_save(Box::new(Save(
                bytes.into(),
            ))))
        },
        load_rom_assets_fn: None,
        pvp: &NoEmulator,
        save_templates: None,
        logo_image: None,
        background: None,
    };
    static OTHER_VARIANT: Game = Game { variant: 1, ..GAME };

    fn scanned_save(path: &str) -> crate::save::ScannedSave {
        crate::save::ScannedSave {
            path: path.into(),
            save: GAME.parse_save(&[1, 1]).unwrap(),
        }
    }

    /// A catalog owning both test variants, with `saves` scanned and one
    /// installed patch "p" 1.0.0 that supports `patch_games`.
    fn catalog(saves: &[(rom::GameRef, &str)], patch_games: &[rom::GameRef]) -> Catalog {
        let catalog = Catalog::new();
        catalog
            .roms
            .rescan(|| Some([(&GAME, b"rom".to_vec()), (&OTHER_VARIANT, b"rom".to_vec())].into()));
        rescan_saves(&catalog, saves);
        let version = patch::Version {
            path: "/patches/p.tangopatch".into(),
            netplay: tango_patch::Compatibility::Isolated,
            rom_overrides: Default::default(),
            supported_games: patch_games.iter().copied().collect(),
            save_templates: Default::default(),
            readme: None,
        };
        let installed = patch::Patch {
            title: "P".into(),
            authors: vec![],
            license: None,
            source: None,
            versions: [(semver::Version::new(1, 0, 0), std::sync::Arc::new(version))].into(),
        };
        catalog.patches.rescan(|| {
            Some(patch::Catalog {
                installed: [("p".to_owned(), std::sync::Arc::new(installed))].into(),
                index: Default::default(),
            })
        });
        catalog
    }

    fn rescan_saves(catalog: &Catalog, saves: &[(rom::GameRef, &str)]) {
        let mut by_game: std::collections::HashMap<rom::GameRef, Vec<crate::save::ScannedSave>> = Default::default();
        for (game, path) in saves {
            by_game.entry(*game).or_default().push(scanned_save(path));
        }
        catalog.saves.rescan(|| Some(by_game));
    }

    fn p() -> Option<(&'static str, semver::Version)> {
        Some(("p", semver::Version::new(1, 0, 0)))
    }

    fn overlay(selection: &Selection) -> Option<(&str, semver::Version)> {
        selection.patch().map(|(name, version)| (name, version.clone()))
    }

    #[test]
    fn the_patch_overlay_follows_the_save() {
        let catalog = catalog(
            &[
                (&GAME, "/saves/a.sav"),
                (&GAME, "/saves/b.sav"),
                (&OTHER_VARIANT, "/saves/c.sav"),
            ],
            &[&GAME],
        );
        let mut config = Config::with_data_path("/".into());
        let mut selection = Selection::default();

        selection.pick_save(&GAME, "/saves/a.sav".into(), &catalog, &config);
        assert_eq!(selection.family(), Some("test"));
        assert_eq!(overlay(&selection), None);
        selection.pick_patch(Some("p".into()), &catalog, &config);
        assert_eq!(overlay(&selection), p());
        assert_eq!(selection.save(), Some(Path::new("/saves/a.sav")));
        selection.persist(&mut config);
        assert_eq!(config.last_save_per_family["test"], "saves/a.sav");
        assert_eq!(
            config.last_patch_per_save["saves/a.sav"],
            Some(("p".to_owned(), semver::Version::new(1, 0, 0)))
        );

        // A save with no memory inherits a patch that can run it...
        selection.pick_save(&GAME, "/saves/b.sav".into(), &catalog, &config);
        assert_eq!(overlay(&selection), p());
        // ...and remembers being explicitly unpatched.
        selection.pick_patch(None, &catalog, &config);
        selection.persist(&mut config);
        selection.pick_save(&GAME, "/saves/a.sav".into(), &catalog, &config);
        assert_eq!(overlay(&selection), p());
        selection.pick_save(&GAME, "/saves/b.sav".into(), &catalog, &config);
        assert_eq!(overlay(&selection), None);

        // A variant the patch can't run drops it.
        selection.pick_save(&GAME, "/saves/a.sav".into(), &catalog, &config);
        selection.pick_save(&OTHER_VARIANT, "/saves/c.sav".into(), &catalog, &config);
        assert_eq!(overlay(&selection), None);
        assert_eq!(selection.game(), Some(&OTHER_VARIANT as rom::GameRef));
    }

    #[test]
    fn an_explicitly_picked_patch_wins_over_the_save() {
        let catalog = catalog(&[(&GAME, "/saves/a.sav"), (&OTHER_VARIANT, "/saves/c.sav")], &[&GAME]);
        let config = Config::with_data_path("/".into());
        let mut selection = Selection::default();
        selection.pick_save(&OTHER_VARIANT, "/saves/c.sav".into(), &catalog, &config);

        selection.pick_patch(Some("p".into()), &catalog, &config);
        assert_eq!(overlay(&selection), p());
        assert_eq!(selection.game(), None);
        assert_eq!(selection.save(), None);
        assert_eq!(selection.family(), Some("test"));
        assert!(selection.patch_ready(&catalog));
        assert!(selection.patch_supports(&catalog, &GAME));
        assert!(!selection.patch_supports(&catalog, &OTHER_VARIANT));

        // A created save adopts the patch it was created under, and one the
        // patch can't run drops it.
        selection.adopt_created_save(&GAME, "/saves/new.sav".into(), &catalog);
        assert_eq!(overlay(&selection), p());
        selection.adopt_created_save(&OTHER_VARIANT, "/saves/new2.sav".into(), &catalog);
        assert_eq!(overlay(&selection), None);
        assert_eq!(selection.save(), Some(Path::new("/saves/new2.sav")));
    }

    #[test]
    fn reconcile_drops_what_a_rescan_retired() {
        let catalog = catalog(&[(&GAME, "/saves/a.sav"), (&GAME, "/saves/b.sav")], &[&GAME]);
        let mut config = Config::with_data_path("/".into());
        let mut selection = Selection::default();
        selection.pick_save(&GAME, "/saves/b.sav".into(), &catalog, &config);
        selection.pick_patch(Some("p".into()), &catalog, &config);
        selection.persist(&mut config);
        selection.pick_save(&GAME, "/saves/a.sav".into(), &catalog, &config);
        selection.pick_patch(None, &catalog, &config);

        // The picked save is gone: fall back to the remembered one, with
        // its patch memory.
        rescan_saves(&catalog, &[(&GAME, "/saves/b.sav")]);
        selection.reconcile(&catalog, &config);
        assert_eq!(selection.save(), Some(Path::new("/saves/b.sav")));
        assert_eq!(overlay(&selection), p());

        // An uninstalled patch is dropped.
        catalog.patches.rescan(|| Some(patch::Catalog::default()));
        selection.reconcile(&catalog, &config);
        assert_eq!(overlay(&selection), None);
        assert_eq!(selection.save(), Some(Path::new("/saves/b.sav")));

        // A game whose ROM is gone clears everything.
        catalog.roms.rescan(|| Some(Default::default()));
        selection.reconcile(&catalog, &config);
        assert!(selection == Selection::default());
    }

    /// Restoring and family picks resolve through the game registry.
    #[cfg(feature = "gamesupport-bn6")]
    #[test]
    fn a_persisted_selection_restores_once_scanned() {
        let games: Vec<rom::GameRef> = game::games_in_family("bn6").collect();
        let (gregar, falzar) = (games[0], games[1]);
        let catalog = Catalog::new();
        catalog
            .roms
            .rescan(|| Some([(gregar, b"rom".to_vec()), (falzar, b"rom".to_vec())].into()));
        rescan_saves(
            &catalog,
            &[
                (gregar, "/saves/e.sav"),
                (falzar, "/saves/f.sav"),
                (gregar, "/saves/g.sav"),
            ],
        );
        let mut config = Config::with_data_path("/".into());
        let mut selection = Selection::default();
        selection.pick_save(falzar, "/saves/f.sav".into(), &catalog, &config);
        selection.persist(&mut config);

        // Before the first scan only the family comes back.
        let mut restored = Selection::default();
        restored.restore(&config, &Catalog::new());
        assert_eq!(restored.family(), Some("bn6"));
        assert_eq!(restored.game(), None);
        restored.restore(&config, &catalog);
        assert!(restored == selection);

        // A family pick lands on the remembered save, else the first one.
        let mut picked = Selection::default();
        picked.pick_family("bn6", &catalog, &config);
        assert_eq!(picked.save(), Some(Path::new("/saves/f.sav")));
        assert_eq!(picked.game(), Some(falzar));
        config.last_save_per_family.clear();
        picked.pick_family("bn6", &catalog, &config);
        assert_eq!(picked.save(), Some(Path::new("/saves/e.sav")));
        assert_eq!(picked.game(), Some(gregar));

        // Deleting the save lands on the family's next one.
        rescan_saves(&catalog, &[(falzar, "/saves/f.sav"), (gregar, "/saves/g.sav")]);
        picked.clear_save();
        picked.pick_first_family_save(&catalog, &config);
        assert_eq!(picked.save(), Some(Path::new("/saves/f.sav")));
        assert_eq!(picked.game(), Some(falzar));
    }

    #[test]
    fn committed_snapshot_takes_precedence_and_validation_is_headless() {
        let files = Memory::default();
        let path = PathBuf::from("/save.sav");
        files.write(&path, &[1, 1]).unwrap();
        let roms = rom::Scanner::new();
        roms.rescan(|| Some([(&GAME, b"rom".to_vec())].into()));
        let catalog = patch::Scanner::new();
        let resolver = Resolver {
            storage: &files,
            roms: &roms,
            patches: &catalog,
            patches_path: PathBuf::from("/patches"),
        };
        let selection = Selection {
            game: Some(&GAME),
            save: Some(path.clone()),
            ..Default::default()
        };
        let resolved = resolver.resolve(&selection, Some(&[9, 0])).unwrap();
        assert_eq!(resolved.sram, [9, 0]);
        assert!(!resolved.validation.is_empty());
        assert_eq!(resolved.prepared.snapshot_sram(), [9, 9]);
        assert_eq!(resolved.prepared.save.to_sram_dump(), [9, 0]);
        assert_eq!(
            files.read(&path).unwrap(),
            [1, 1],
            "preparing and snapshotting must not commit edits"
        );
        assert_eq!(roms.read()[&GAME], b"rom");
        let disk = resolver.resolve(&selection, None).unwrap();
        assert!(disk.validation.is_empty());
        assert_eq!(disk.sram, [1, 1]);
    }
    #[test]
    fn broken_selection_fails_before_launch_and_missing_patch_never_falls_back() {
        let files = Memory::default();
        let roms = rom::Scanner::new();
        let catalog = patch::Scanner::new();
        let resolver = Resolver {
            storage: &files,
            roms: &roms,
            patches: &catalog,
            patches_path: PathBuf::from("/patches"),
        };
        assert!(matches!(
            resolver.resolve(&Selection::default(), Some(&[1, 1])),
            Err(Error::MissingGame)
        ));
        let mut selection = Selection {
            game: Some(&GAME),
            ..Default::default()
        };
        assert!(matches!(resolver.resolve(&selection, None), Err(Error::MissingSave)));
        assert!(matches!(
            resolver.resolve(&selection, Some(&[1, 1])),
            Err(Error::Rom(rom::LoadError::MissingRom { .. }))
        ));
        roms.rescan(|| Some([(&GAME, b"rom".to_vec())].into()));
        selection.patch = Some("missing".into());
        selection.patch_version = Some(semver::Version::new(1, 0, 0));
        assert!(matches!(
            resolver.resolve(&selection, Some(&[1, 1])),
            Err(Error::Rom(rom::LoadError::Patch { .. }))
        ));
        assert_eq!(roms.read()[&GAME], b"rom");

        // The save editor's preview recovers from the same missing patch.
        let preview = resolver
            .preview(
                &GAME,
                PathBuf::new(),
                GAME.parse_save(&[1, 1]).unwrap(),
                selection.patch(),
            )
            .unwrap();
        assert!(preview.patch.is_none());
        assert_eq!(preview.save.to_sram_dump(), [1, 1]);
    }
}
