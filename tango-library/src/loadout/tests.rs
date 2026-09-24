use super::*;
use crate::storage::Storage;
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
