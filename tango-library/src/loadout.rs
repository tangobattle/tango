//! Resolve launch inputs independently of UI, devices, and session scheduling.
use crate::{game, patch, rom, save, storage::Storage};
use std::path::{Path, PathBuf};
use tango_net_protocol::control as protocol;

#[derive(Clone, PartialEq, Default)]
pub struct LoadoutSelection {
    pub game: Option<rom::GameRef>,
    pub save_path: Option<PathBuf>,
    pub patch: Option<(String, semver::Version)>,
}

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
    pub patches_path: &'a Path,
}
impl Resolver<'_> {
    pub fn resolve(&self, selection: &LoadoutSelection, snapshot: Option<&[u8]>) -> Result<ResolvedLoadout, Error> {
        let game = selection.game.ok_or(Error::MissingGame)?;
        let bytes = match snapshot {
            Some(bytes) => bytes.to_vec(),
            None => self
                .storage
                .read(selection.save_path.as_deref().ok_or(Error::MissingSave)?)?,
        };
        let save = game.parse_save(&bytes)?;
        let rom = rom::load(
            self.storage,
            self.roms,
            self.patches_path,
            game,
            selection.patch.as_ref().map(|(name, version)| (name.as_str(), version)),
        )?;
        let applied_patch = selection
            .patch
            .as_ref()
            .map(|(name, version)| tango_gamesupport::AppliedPatch {
                name: name.clone(),
                version: version.clone(),
                rom_overrides: self
                    .patches
                    .read()
                    .version(name, version)
                    .map(|meta| meta.rom_overrides_for(game))
                    .unwrap_or_default(),
            });
        let prepared = tango_gamesupport_common_dataview::model::prepare(
            game,
            &rom,
            selection.save_path.clone().unwrap_or_default(),
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
    pub fn prepare_match(
        &self,
        local: &protocol::Settings,
        remote: &protocol::Settings,
        saves: [&[u8]; 2],
    ) -> Result<PreparedMatch, Error> {
        Ok(PreparedMatch {
            local: self.resolve(
                &LoadoutSelection::from_game_info(local.game_info.as_ref().ok_or(Error::MissingGameInfo)?)?,
                Some(saves[0]),
            )?,
            remote: self.resolve(
                &LoadoutSelection::from_game_info(remote.game_info.as_ref().ok_or(Error::MissingGameInfo)?)?,
                Some(saves[1]),
            )?,
        })
    }
}
impl LoadoutSelection {
    pub fn is_playable(&self) -> bool {
        self.game.is_some() && self.save_path.is_some()
    }
    pub fn game_info(&self) -> Option<protocol::GameInfo> {
        let game = self.game?;
        Some(protocol::GameInfo {
            family_and_variant: (game.family.id.to_owned(), game.variant),
            patch: self.patch.as_ref().map(|(name, version)| protocol::PatchInfo {
                name: name.clone(),
                version: version.clone(),
            }),
            sim_version: game.pvp.sim_version(),
        })
    }
    pub fn from_game_info(info: &protocol::GameInfo) -> Result<Self, Error> {
        let (family, variant) = &info.family_and_variant;
        let game = game::find_by_family_and_variant(family, *variant)
            .ok_or_else(|| Error::UnknownGame(family.clone(), *variant))?;
        if game.pvp.sim_version() != info.sim_version {
            return Err(Error::SimulationVersion(family.clone()));
        }
        Ok(Self {
            game: Some(game),
            save_path: None,
            patch: info.patch.as_ref().map(|p| (p.name.clone(), p.version.clone())),
        })
    }
    /// Browser selection policy: retain valid choices, otherwise choose the first save.
    pub fn reconcile(
        &mut self,
        roms: &std::collections::HashMap<rom::GameRef, Vec<u8>>,
        saves: &std::collections::HashMap<rom::GameRef, Vec<save::ScannedSave>>,
        patches: &patch::Catalog,
    ) {
        let Some(game) = self.game.filter(|game| roms.contains_key(game)) else {
            *self = Self::default();
            return;
        };
        if self
            .patch
            .as_ref()
            .is_some_and(|(name, version)| !patches.is_installed(name, version))
        {
            self.patch = None;
        }
        let saves = saves.get(&game).map(Vec::as_slice).unwrap_or_default();
        if !self
            .save_path
            .as_ref()
            .is_some_and(|path| saves.iter().any(|save| &save.path == path))
        {
            self.save_path = saves.first().map(|save| save.path.clone());
        }
    }
}

/// Library facts needed for compatibility; contains neither locks nor ROM bytes.
pub struct CompatibilityFacts {
    pub remote_rom_available: bool,
    pub matching_tags: bool,
    pub missing_patch: Option<(String, semver::Version)>,
}
pub fn compatibility_facts(
    local: &protocol::Settings,
    remote: &protocol::Settings,
    roms: &std::collections::HashMap<rom::GameRef, Vec<u8>>,
    catalog: &patch::Catalog,
) -> CompatibilityFacts {
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
    CompatibilityFacts {
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
        games: &[&GAME],
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
            patches_path: Path::new("/patches"),
        };
        let selection = LoadoutSelection {
            game: Some(&GAME),
            save_path: Some(path.clone()),
            patch: None,
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
            patches_path: Path::new("/patches"),
        };
        assert!(matches!(
            resolver.resolve(&LoadoutSelection::default(), Some(&[1, 1])),
            Err(Error::MissingGame)
        ));
        let mut selection = LoadoutSelection {
            game: Some(&GAME),
            ..Default::default()
        };
        assert!(matches!(resolver.resolve(&selection, None), Err(Error::MissingSave)));
        assert!(matches!(
            resolver.resolve(&selection, Some(&[1, 1])),
            Err(Error::Rom(rom::LoadError::MissingRom { .. }))
        ));
        roms.rescan(|| Some([(&GAME, b"rom".to_vec())].into()));
        selection.patch = Some(("missing".into(), semver::Version::new(1, 0, 0)));
        assert!(matches!(
            resolver.resolve(&selection, Some(&[1, 1])),
            Err(Error::Rom(rom::LoadError::Patch { .. }))
        ));
        assert_eq!(roms.read()[&GAME], b"rom");
    }
}
