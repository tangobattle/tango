use super::*;
use tango_script::{Action, Package, PackageRef};

fn bundled_catalog() -> tango_library::package::Catalog {
    futures::executor::block_on(tango_library::package::Catalog::scan(
        &tango_library::storage::StdStorage,
        std::path::Path::new("packages"),
        &Default::default(),
        crate::package::editor::bundled::packages().unwrap(),
    ))
}

fn package() -> Package {
    Package::load([
        ("package.toml".into(), b"api=1\nname='embedded-test'\nversion='1.0.0'\n[[editor]]\nname='main'\npath='./init'".to_vec()),
        ("init.luau".into(), br#"--!strict
local t = tango.load_catalog("./locales")
local editor: Editor = {
    detect_rom = function(rom: buffer): boolean return buffer.len(rom) == 1 and buffer.readu8(rom, 0) == 42 end,
        decode = function(b: buffer): buffer assert(buffer.len(b) == 2 and buffer.readu8(b, 0) == buffer.readu8(b, 1)); return b end,
        encode = function(b: buffer): buffer buffer.writeu8(b, 1, buffer.readu8(b, 0)); return b end,
        validate = function(_b: buffer): {string} return {} end,
        view = function(_b: buffer, _state: ViewState, context: EditorContext): Node
            assert(context.embedding.inline_actions, "failed panel configuration")
            local children: {Node} = {
                {kind = "button", id = "begin", text = "Edit"},
                {kind = "button", id = "add", text = "Add"},
                {kind = "button", id = "cancel", text = "Cancel"},
                {kind = "button", id = "save", text = "Save"},
                {kind = "button", id = "launch", text = "Launch"},
                {kind = "button", id = "fail", text = "Fail"},
                {kind = "scroll", id = "body", child = {kind = "text", text = "Body"} :: Node},
            }
            return {kind = "column", children = children}
        end,
        update = function(b: buffer, _state: ViewState, action: Action): {Effect}?
            if action.id == "fail" then
                assert(tango.locale() == "en-US", "failed edit must not be retried")
                buffer.writeu8(b, 0, 99)
                tango.fail(t, "failure", {value = 7})
            end
            if action.id == "add" then buffer.writeu8(b, 0, buffer.readu8(b, 0) + 1); return nil end
            if action.id == "launch" then return {
                {kind = "scroll_to", id = "body", x = 0, y = 0} :: Effect,
                {kind = "invoke", id = "play"} :: Effect, {kind = "invoke", id = "play"} :: Effect,
            } end
            return {{kind = "edit", command = action.id :: any} :: Effect}
        end,
}
return editor
"#.to_vec()),
        ("locales/en-US/main.ftl".into(), b"failure = Cannot edit { $value }\n".to_vec()),
        ("locales/ja-JP/main.ftl".into(), "failure = 編集できません { $value }\n".as_bytes().to_vec()),
    ].into()).unwrap()
}

fn profile() -> Profile {
    Profile::resolve(
        &[package()],
        &PackageRef {
            name: "embedded-test".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
}

fn loaded() -> LoadedSave {
    LoadedSave {
        editor: &EDITOR,
        native_game: None,
        chips: Vec::new(),
        save_path: "test.sav".into(),
        patch: None,
        state: Box::new(State::open(profile(), &[7, 7]).unwrap()),
        payload: Box::new(Payload),
    }
}
fn language() -> LanguageIdentifier {
    "en-US".parse().unwrap()
}
fn input(data: &LoadedSave, action: &str) -> Input {
    let state = state(data);
    Input {
        identity: state.identity.clone(),
        epoch: state.controller.lock().unwrap().epoch,
        action: Some(Action::activate(action, "")),
    }
}
fn render(data: &LoadedSave, editable: bool, inline: bool) {
    drop(EDITOR.view(&language(), data, false, Some(true), inline, editable));
}
fn send(data: &mut LoadedSave, action: &str) -> Vec<SaveEditorEvent> {
    let input = input(data, action);
    EDITOR.update(&language(), data, &input, &Theme::Dark).1
}

#[test]
fn embedded_save_keeps_the_draft_until_storage_acknowledges_it() {
    let mut data = loaded();
    // Session snapshots don't depend on a prior render.
    assert_eq!(EDITOR.sram(&data).unwrap(), [7, 7]);
    render(&data, true, true);
    send(&mut data, "begin");
    send(&mut data, "add");
    let events = send(&mut data, "save");
    assert!(matches!(events.as_slice(), [SaveEditorEvent::Commit { sram }] if sram == &[8, 8]));
    assert!(state(&data).controller.lock().unwrap().session.is_saving());
    EDITOR.save_finished(&mut data, Err("disk full".into()));
    assert!(state(&data).controller.lock().unwrap().session.is_editing());
    assert_eq!(EDITOR.sram(&data).unwrap(), [8, 8]);
    send(&mut data, "cancel");
    assert_eq!(EDITOR.sram(&data).unwrap(), [7, 7]);
    send(&mut data, "begin");
    send(&mut data, "add");
    send(&mut data, "save");
    EDITOR.save_finished(&mut data, Ok(()));
    assert!(!state(&data).controller.lock().unwrap().session.is_editing());
    assert_eq!(EDITOR.sram(&data).unwrap(), [8, 8]);
}

#[test]
fn retained_embedded_failure_changes_language_without_retrying_the_edit() {
    let mut data = loaded();
    render(&data, true, true);
    send(&mut data, "begin");
    send(&mut data, "fail");
    assert_eq!(EDITOR.sram(&data).unwrap(), [7, 7]);
    let japanese = "ja-JP".parse().unwrap();
    drop(EDITOR.view(&japanese, &data, false, Some(true), true, true));
    let mut controller = state(&data).controller.lock().unwrap();
    assert!(!controller.blocked);
    let error = controller.error.as_ref().unwrap();
    assert!(error.to_string().contains("Cannot edit 7"));
    let rendered = crate::package::error::localized(error, &japanese);
    assert!(rendered.contains("編集できません 7"), "{rendered}");
    assert!(!rendered.contains("Cannot edit"));
    assert!(rendered.contains("stack traceback"));
    assert_eq!(controller.session.document().snapshot().unwrap(), [7, 7]);
    let error = controller.error.take().unwrap().context("Editing save");
    let rendered = crate::package::error::localized(&error, &japanese);
    assert!(rendered.starts_with("Editing save: 編集できません 7"), "{rendered}");
}

#[test]
fn embedded_messages_cannot_cross_saves_or_permission_changes() {
    let mut data = loaded();
    render(&data, true, true);
    let stale = input(&data, "begin");
    let mut other = loaded();
    render(&other, true, true);
    let _ = EDITOR.update(&language(), &mut other, &stale, &Theme::Dark);
    assert!(!state(&other).controller.lock().unwrap().session.is_editing());
    render(&data, false, true);
    let _ = EDITOR.update(&language(), &mut data, &stale, &Theme::Dark);
    send(&mut data, "begin");
    assert!(!state(&data).controller.lock().unwrap().session.is_editing());
    render(&data, true, true);
    send(&mut data, "begin");
    let stale = input(&data, "add");
    render(&data, false, false); // The script fails while revoking permissions.
    assert!(state(&data).controller.lock().unwrap().blocked);
    let _ = EDITOR.update(&language(), &mut data, &stale, &Theme::Dark);
    assert!(EDITOR.sram(&data).is_err());
    render(&data, false, true);
    assert_eq!(EDITOR.sram(&data).unwrap(), [7, 7]);
    send(&mut data, "add");
    assert_eq!(EDITOR.sram(&data).unwrap(), [7, 7]);
}

#[test]
fn editor_updates_keep_scroll_tasks_and_all_host_requests() {
    let mut data = loaded();
    render(&data, true, true);
    let input = input(&data, "launch");
    let (task, events) = EDITOR.update(&language(), &mut data, &input, &Theme::Light);
    assert!(task.units() > 0);
    assert!(matches!(
        events.as_slice(),
        [SaveEditorEvent::Play, SaveEditorEvent::Play]
    ));
    send(&mut data, "begin");
    assert!(send(&mut data, "launch").is_empty());
}

#[test]
fn automatic_selection_rejects_ambiguity_and_bundled_packages_resolve() {
    assert!(select(&[profile()], &[42]).unwrap().is_some());
    assert!(select(&[profile()], &[0]).unwrap().is_none());
    assert!(select(&[profile(), profile()], &[42]).is_err());
    let catalog = bundled_catalog();
    assert!(!catalog.exports().is_empty());
    for export in catalog.exports() {
        assert!(!export.profile.supports_export(export.reference.kind, &[]).unwrap());
    }
}

#[test]
fn installed_editor_reaches_the_normal_embedded_adapter_through_the_scanner() {
    use tango_library::storage::Storage;
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root = Directory(std::env::temp_dir().join(format!(
            "tango-package-scan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let config = crate::config::Config {
        library: tango_library::config::Config::with_data_path(root.0.clone()),
        ..Default::default()
    };
    let path = config.packages_path().join("embedded-test/1.0.0.tangopkg");
    tango_library::storage::write_atomic(
        &tango_library::storage::StdStorage,
        &path,
        &package().to_archive().unwrap(),
    )
    .unwrap();
    let listing = futures::executor::block_on(tango_library::storage::StdStorage.list(&[config.packages_path()]));
    let scanners = crate::library::Scanners::new();
    scanners.rescan_packages(&config, &listing);
    let catalog = scanners.packages.read();
    assert!(catalog.issues().is_empty(), "{:?}", catalog.issues());
    assert!(catalog
        .exports()
        .iter()
        .any(|export| export.reference.package.name == "embedded-test"));
    let mut data = loaded();
    let original = state(&data).identity.clone();
    attach(&mut data, &[42], &[9, 9], &catalog);
    assert!(!Arc::ptr_eq(&state(&data).identity, &original));
    assert_eq!(EDITOR.sram(&data).unwrap(), [9, 9]);
    render(&data, true, true);
    send(&mut data, "begin");
    send(&mut data, "add");
    assert_eq!(EDITOR.sram(&data).unwrap(), [10, 10]);
    send(&mut data, "cancel");
    assert_eq!(EDITOR.sram(&data).unwrap(), [9, 9]);
}

/// Uses the regular selection constructor and an actual local ROM/save pair.
/// All edits stay in memory; the source save is never written.
#[test]
#[ignore = "set TANGO_TEST_ROM and TANGO_TEST_SAVE to local fixtures"]
fn bundled_editor_in_normal_panels_with_a_real_save() {
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let rom = std::fs::read(std::env::var_os("TANGO_TEST_ROM").expect("TANGO_TEST_ROM")).unwrap();
    let sram = std::fs::read(std::env::var_os("TANGO_TEST_SAVE").expect("TANGO_TEST_SAVE")).unwrap();
    let directory =
        Directory(std::env::temp_dir().join(format!("tango-editor-selection-{:016x}", rand::random::<u64>())));
    let config = crate::config::Config {
        library: tango_library::config::Config::with_data_path(directory.0.clone()),
        ..Default::default()
    };
    let scanners = crate::library::Scanners::new();
    let mut roms = crate::library::rom::Catalog::default();
    let rom_id = roms.insert(rom, None);
    scanners.roms.rescan(|| Some(roms));
    scanners.rescan_packages(&config, &Default::default());
    let save_path = config.saves_path().join("selected.sav");
    std::fs::create_dir_all(save_path.parent().unwrap()).unwrap();
    std::fs::write(&save_path, &sram).unwrap();
    let listing = futures::executor::block_on(crate::library::storage().list(&[config.saves_path()]));
    scanners
        .saves
        .rescan(|| Some(crate::library::save::scan_saves(crate::library::storage(), &listing)));
    let option = crate::loadout::game_options(&config.language, &scanners)
        .into_iter()
        .find(|option| option.choice == crate::loadout::GameChoice::Package(rom_id))
        .unwrap();
    let mut loadout = crate::loadout::Loadout::default();
    let mut loaded = None;
    loadout.update(crate::loadout::Message::GameSelected(option), &scanners, &config);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(loadout.gamemodes.selected.is_some());
    assert!(loadout.gamemodes.error.is_none(), "{:?}", loadout.gamemodes.error);
    let option = crate::loadout::save_options(&loadout, &config.language, &scanners, &config)
        .into_iter()
        .find(|option| option.path == save_path)
        .unwrap();
    loadout.update(crate::loadout::Message::SaveSelected(option), &scanners, &config);
    loadout.refresh_package(&scanners, &config, &mut loaded);
    assert!(loadout.package_save.error.is_none(), "{:?}", loadout.package_save.error);
    assert!(loadout.has_save(loaded.as_ref()));
    assert_eq!(loadout.save_sram(loaded.as_ref()).unwrap(), sram);
    assert!(loadout
        .make_local_settings(&config, &Default::default())
        .game_info
        .is_none());
    let mut data = loaded.unwrap();
    assert!((data.state.as_ref() as &dyn std::any::Any).is::<State>());
    let baseline = EDITOR.sram(&data).unwrap();
    for locale in ["en-US", "ja-JP"] {
        let lang = locale.parse().unwrap();
        for (inline, editable, streamer) in [(true, true, false), (true, false, false), (false, false, true)] {
            drop(data.editor.view(&lang, &data, streamer, None, inline, editable));
            let controller = state(&data).controller.lock().unwrap();
            assert!(!controller.blocked, "{:?}", controller.error);
            assert_eq!(
                controller
                    .session
                    .document()
                    .view()
                    .accepts(&Action::activate("editor.session:begin", "")),
                editable && !streamer
            );
        }
    }
    render(&data, true, true);
    send(&mut data, "editor.session:begin");
    assert!(state(&data).controller.lock().unwrap().session.is_editing());
    send(&mut data, "navi.picker");
    send(&mut data, "navi.picker");
    send(&mut data, "editor.session:cancel");
    assert!(!state(&data).controller.lock().unwrap().session.is_editing());
    assert_eq!(EDITOR.sram(&data).unwrap(), baseline);
    assert!(data.native_game.is_none());
    assert!(data.chips.is_empty());
    assert_eq!(std::fs::read(save_path).unwrap(), sram);
}

#[test]
#[ignore = "set TANGO_TEST_ROM and TANGO_TEST_SAVE to a ROM and one of its bundled save templates"]
fn bundled_template_matches_reference_and_boots_solo() {
    use tango_match::Backend as _;
    let rom = std::fs::read(std::env::var_os("TANGO_TEST_ROM").expect("TANGO_TEST_ROM")).unwrap();
    let expected = std::fs::read(std::env::var_os("TANGO_TEST_SAVE").expect("TANGO_TEST_SAVE")).unwrap();
    let catalog = bundled_catalog();
    let profile = super::super::selection::resolve(&catalog, &rom, None)
        .unwrap()
        .unwrap()
        .with_inputs(tango_script::Inputs::new([("rom".into(), rom.clone())].into()).unwrap());
    let templates = profile.save_templates().unwrap();
    assert!(!templates.is_empty());
    let localized = profile.clone().with_locale("ja-JP").unwrap().save_templates().unwrap();
    assert_eq!(
        templates.iter().map(|template| &template.name).collect::<Vec<_>>(),
        localized.iter().map(|template| &template.name).collect::<Vec<_>>()
    );
    assert!(templates
        .iter()
        .chain(&localized)
        .all(|template| !template.label.is_empty()));
    let sram = templates
        .iter()
        .map(|template| profile.create_save(&template.name).unwrap())
        .find(|bytes| *bytes == expected)
        .expect("no bundled template matches the supplied reference save");
    // Run cartridge-specific package checks with the same ROM input.
    profile.test().unwrap();
    let mode = catalog
        .latest_exports(tango_script::ExportKind::GameMode)
        .into_iter()
        .find(|export| {
            export
                .profile
                .supports_export(tango_script::ExportKind::GameMode, &rom)
                .unwrap()
        })
        .unwrap();
    let prepared = Arc::new(
        tango_script::PreparedGameMode::prepare(
            mode.profile.clone(),
            tango_backend_mgba::gamemode::runtime(),
            rom.clone(),
            false,
            Default::default(),
        )
        .unwrap(),
    );
    let backend = tango_backend_mgba::gamemode::Backend::new([prepared.clone(), prepared]).unwrap();
    let solo = backend
        .start_solo(tango_match::SoloConfig {
            rom: &rom,
            save: Some(&sram),
            rtc: None,
            audio: None,
        })
        .unwrap();
    for _ in 0..180 {
        solo.tick(Default::default()).unwrap();
    }
    let frame = solo.frame().unwrap();
    assert!(frame.chunks_exact(4).any(|pixel| pixel[0..3] != [0, 0, 0]));
    assert_eq!(solo.export_save().unwrap(), sram);
}
