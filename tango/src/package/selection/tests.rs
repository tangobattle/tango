use super::*;
use crate::loadout::{self, GameChoice, Loadout, Message};
use tango_script::Package;

fn package(editor: bool, gamemode: bool) -> Package {
    let mut manifest = "api=1\nname='selection-test'\nversion='1.0.0'\n".to_owned();
    if gamemode {
        manifest.push_str("default_gamemode='first'\n[[gamemode]]\nname='first'\npath='./mode'\n[[gamemode]]\nname='second'\npath='./mode'\n");
    }
    if editor {
        manifest.push_str("[[editor]]\nname='save'\npath='./editor'\n");
    }
    Package::load(
        [
            ("package.toml".into(), manifest.into_bytes()),
            ("init.luau".into(), b"--!strict\nreturn {}".to_vec()),
            (
                "editor.luau".into(),
                br#"--!strict
return {
    detect_rom = function(rom: buffer): boolean return buffer.len(rom) == 1 and buffer.readu8(rom, 0) == 42 end,
    decode = function(bytes: buffer): buffer assert(buffer.len(bytes) == 2, "invalid length"); return bytes end,
    encode = function(bytes: buffer): buffer return bytes end,
    validate = function(bytes: buffer): {string}
        if buffer.readu8(bytes, 0) ~= buffer.readu8(bytes, 1) then return {"unequal bytes"} end
        return {}
    end,
    update = function() end,
    view = function(bytes: buffer): Node
        assert(buffer.readu8(bytes, 0) ~= 99, "view failed")
        return {kind = "text", text = "Editor"}
    end,
} :: Editor
"#
                .to_vec(),
            ),
            (
                "mode.luau".into(),
                br#"--!strict
return {
    platform = "gba",
    detect_rom = function(rom: buffer): boolean return buffer.len(rom) == 1 and buffer.readu8(rom, 0) == 42 end,
    setup = function(_context: GameModeContext, game: Game) game.hook(0x08000000, game.ready) end,
} :: GameMode
"#
                .to_vec(),
            ),
            (
                "locales/en-US/package.ftl".into(),
                b"package-name = Test game\ngamemode-first = First\ngamemode-second = Second\n".to_vec(),
            ),
        ]
        .into(),
    )
    .unwrap()
}

fn setup(editor: bool, gamemode: bool) -> (Scanners, crate::config::Config, rom::Id) {
    let scanners = Scanners::new();
    let config = crate::config::Config {
        library: tango_library::config::Config::with_data_path("/virtual/tango".into()),
        ..Default::default()
    };
    let mut roms = rom::Catalog::default();
    let id = roms.insert(vec![42], Some(config.roms_path().join("unknown.custom")));
    assert!(roms.image(id).unwrap().native_game.is_none());
    scanners.roms.rescan(|| Some(roms));
    let packages = futures::executor::block_on(tango_library::package::Catalog::scan(
        crate::library::storage(),
        Path::new("unused"),
        &Default::default(),
        &[package(editor, gamemode)],
    ));
    assert!(packages.issues().is_empty(), "{:?}", packages.issues());
    scanners.packages.rescan(|| Some(packages));
    *scanners.package_roms.write().unwrap() = crate::package::Catalog::scan(&scanners);
    saves(
        &scanners,
        &config,
        &[("good.sav", &[7, 7]), ("bad.sav", &[7, 8]), ("broken.sav", &[1])],
    );
    (scanners, config, id)
}

fn saves(scanners: &Scanners, config: &crate::config::Config, files: &[(&str, &[u8])]) {
    let mut saves = crate::library::save::Catalog::default();
    for (name, bytes) in files {
        saves.insert(config.saves_path().join(name), bytes.to_vec());
    }
    scanners.saves.rescan(|| Some(saves));
}

fn choose(scanners: &Scanners, config: &crate::config::Config, id: rom::Id) -> (Loadout, Option<LoadedSave>) {
    let choice = loadout::game_options(&config.language, scanners)
        .into_iter()
        .find(|option| option.choice == GameChoice::Package(id))
        .unwrap();
    assert!(choice.available);
    assert!(choice.display.contains("Test game"), "{}", choice.display);
    assert!(choice.display.contains("unknown.custom"));
    let mut loadout = Loadout::default();
    loadout.update(Message::GameSelected(choice), scanners, config);
    let mut loaded = None;
    loadout.refresh_package(scanners, config, &mut loaded);
    if let Some(option) = loadout::save_options(&loadout, &config.language, scanners, config)
        .into_iter()
        .find(|option| option.path.ends_with("good.sav"))
    {
        loadout.update(Message::SaveSelected(option), scanners, config);
        loadout.refresh_package(scanners, config, &mut loaded);
    }
    assert!(loadout.package_save.error.is_none(), "{:?}", loadout.package_save.error);
    (loadout, loaded)
}

#[test]
fn picker_loads_an_unregistered_cartridge_and_filters_saves_through_its_editor() {
    let (scanners, mut config, id) = setup(true, true);
    let (mut loadout, loaded) = choose(&scanners, &config, id);
    assert!(loadout.game.is_none());
    assert!(loadout.family.is_none());
    assert!(loaded.as_ref().unwrap().native_game.is_none());
    assert_eq!(loadout.gamemodes.selected.as_ref().unwrap().name, "first");
    let options = loadout::save_options(&loadout, &config.language, &scanners, &config);
    assert_eq!(options.len(), 2); // A validation warning is editable, not an unknown format.
    let good = options
        .into_iter()
        .find(|option| option.path.ends_with("good.sav"))
        .unwrap();
    loadout.update(Message::SaveSelected(good), &scanners, &config);
    assert!(loadout.has_save(loaded.as_ref()));
    assert_eq!(loadout.save_sram(loaded.as_ref()).unwrap(), [7, 7]);
    let settings = loadout.make_local_settings(&config, &Default::default());
    assert!(settings.game_info.is_none());
    assert_eq!(settings.gamemode.unwrap().identity.export, "first");

    let second = loadout
        .gamemodes
        .choices
        .iter()
        .find(|choice| choice.reference.name == "second")
        .unwrap()
        .reference
        .clone();
    loadout.update(Message::GameModeSelected(second.clone()), &scanners, &config);
    loadout.remember_package(&mut config);
    let config: crate::config::Config = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
    let mut restored = Loadout::default();
    restored.select_package(config.last_package_rom.unwrap(), &scanners, &config);
    assert_eq!(restored.save, loadout.save);
    assert_eq!(restored.editors.selected, loadout.editors.selected);
    assert_eq!(restored.gamemodes.selected, Some(second.clone()));
    assert!(restored.gamemodes.error.is_none());

    // Uninstalling the selected package leaves a visible, unusable selection.
    scanners.packages.rescan(|| Some(Default::default()));
    *scanners.package_roms.write().unwrap() = crate::package::Catalog::scan(&scanners);
    restored.select_package(id, &scanners, &config);
    let mut loaded = loaded;
    restored.refresh_package(&scanners, &config, &mut loaded);
    assert_eq!(restored.gamemodes.selected, Some(second));
    assert!(restored.gamemodes.prepared.is_none());
    assert!(restored.package_save.error.is_some());
    assert!(!restored.has_save(loaded.as_ref()));
}

#[test]
fn unrelated_scans_preserve_the_editor_but_external_save_changes_reload_it() {
    let (scanners, config, id) = setup(true, true);
    let (mut loadout, mut loaded) = choose(&scanners, &config, id);
    let state = &*loaded.as_ref().unwrap().state as *const dyn tango_gamesupport::SaveEditorState;
    let packages = scanners.packages.read().clone();
    scanners.packages.rescan(|| Some(packages));
    *scanners.package_roms.write().unwrap() = crate::package::Catalog::scan(&scanners);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(std::ptr::eq(state, &*loaded.as_ref().unwrap().state));
    saves(&scanners, &config, &[("good.sav", &[7, 7]), ("other.sav", &[8, 8])]);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(std::ptr::eq(state, &*loaded.as_ref().unwrap().state));
    saves(&scanners, &config, &[("good.sav", &[9, 9])]);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert_eq!(loadout.save_sram(loaded.as_ref()).unwrap(), [9, 9]);
    saves(&scanners, &config, &[("good.sav", &[9])]);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(loaded.is_none());
    assert!(loadout
        .package_save
        .error
        .as_deref()
        .unwrap()
        .contains("invalid length"));
    assert!(!loadout.has_save(None));
    saves(&scanners, &config, &[]);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(loadout
        .package_save
        .error
        .as_deref()
        .unwrap()
        .contains("no longer available"));
}

#[test]
fn gamemode_without_an_editor_commits_the_exact_selected_file() {
    let (scanners, config, id) = setup(false, true);
    let (mut loadout, mut loaded) = choose(&scanners, &config, id);
    assert!(loaded.is_none());
    let options = loadout::save_options(&loadout, &config.language, &scanners, &config);
    let selected = options
        .into_iter()
        .find(|option| option.path.ends_with("broken.sav"))
        .unwrap();
    loadout.update(Message::SaveSelected(selected), &scanners, &config);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(loadout.has_save(None));
    assert_eq!(loadout.save_sram(None).unwrap(), [1]);
    loadout.save = Some(config.saves_path().join("missing.sav"));
    assert!(!loadout.has_save(None));
    assert!(loadout.save_sram(None).is_err());
    loadout.save = None;
    assert!(!loadout.has_save(None));
}

#[test]
fn acknowledged_writes_preserve_the_editor_until_the_scanner_catches_up() {
    let (scanners, config, id) = setup(true, true);
    let (mut loadout, mut loaded) = choose(&scanners, &config, id);
    let state = &*loaded.as_ref().unwrap().state as *const dyn tango_gamesupport::SaveEditorState;
    // Selection receives the encoded output only after the editor acknowledges
    // its write. Its job here is to retain that editor through the rescan.
    loadout.package_save.saved(&[8, 8]);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(std::ptr::eq(state, &*loaded.as_ref().unwrap().state));
    saves(&scanners, &config, &[("good.sav", &[8, 8])]);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(std::ptr::eq(state, &*loaded.as_ref().unwrap().state));
    saves(&scanners, &config, &[("good.sav", &[9, 9])]);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert_eq!(loadout.save_sram(loaded.as_ref()).unwrap(), [9, 9]);
}

#[test]
fn editor_only_packages_are_selectable_and_validation_does_not_render_views() {
    let (scanners, config, id) = setup(true, false);
    saves(
        &scanners,
        &config,
        &[("good.sav", &[7, 7]), ("bad-view.sav", &[99, 99])],
    );
    let (mut loadout, mut loaded) = choose(&scanners, &config, id);
    assert!(loadout.gamemodes.selected.is_none());
    // A view exception does not hide a compatible file from the save picker.
    assert_eq!(loadout.package_save.saves().count(), 2);
    let options = loadout::save_options(&loadout, &config.language, &scanners, &config);
    let selected = options
        .into_iter()
        .find(|option| option.path.ends_with("bad-view.sav"))
        .unwrap();
    loadout.update(Message::SaveSelected(selected), &scanners, &config);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(loaded.is_none());
    assert!(loadout.package_save.error.as_deref().unwrap().contains("view failed"));
    assert!(!loadout.has_save(None));
}
