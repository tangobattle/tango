use std::collections::BTreeMap;

use tango_script::{Action, Document, ExportKind, Package, PackageRef, Profile, TelemetryContext, TelemetryValue};

fn files(exports: &[(ExportKind, &str, &str)], init: &str, modules: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
    let mut manifest = serde_json::json!({"api":1,"name":"games","version":"1.0.0"});
    if let Some((_, name, _)) = exports.iter().find(|(kind, _, _)| *kind == ExportKind::GameMode) {
        manifest["default_gamemode"] = (*name).into();
    }
    for kind in ExportKind::ALL {
        manifest[kind.as_str()] = exports
            .iter()
            .filter(|(k, _, _)| *k == kind)
            .map(|(_, name, path)| serde_json::json!({"name":name,"path":path}))
            .collect();
    }
    let mut files = BTreeMap::from([
        ("package.toml".into(), toml::to_string(&manifest).unwrap().into_bytes()),
        ("init.luau".into(), init.as_bytes().to_vec()),
    ]);
    files.extend(
        modules
            .iter()
            .map(|(path, source)| (path.to_string(), source.as_bytes().to_vec())),
    );
    files
}

fn resolve(packages: &[Package], name: &str) -> tango_script::Result<Profile> {
    Profile::resolve(
        packages,
        &PackageRef {
            name: name.into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
}

#[test]
fn gamemode_packages_must_declare_an_existing_default() {
    let valid = capabilities();
    for default in [None, Some("missing")] {
        let mut files = valid.clone();
        let mut manifest: toml::Value = std::str::from_utf8(&files["package.toml"]).unwrap().parse().unwrap();
        match default {
            None => {
                manifest.as_table_mut().unwrap().remove("default_gamemode");
            }
            Some(name) => {
                manifest["default_gamemode"] = name.into();
            }
        }
        files.insert("package.toml".into(), toml::to_string(&manifest).unwrap().into_bytes());
        assert!(Package::load(files)
            .err()
            .unwrap()
            .to_string()
            .contains("default_gamemode"));
    }
    let mut files = valid;
    let text = std::str::from_utf8(&files["package.toml"])
        .unwrap()
        .replace("default_gamemode = \"one\"", "default_gamemode = \"two\"");
    files.insert("package.toml".into(), text.into_bytes());
    let profile = resolve(&[Package::load(files).unwrap()], "games").unwrap();
    assert_eq!(profile.selected_export(ExportKind::GameMode), Some("two"));
}

const FACTORY: &str = r#"--!strict
return function(n: number): Editor
    return {
        decode = function(bytes: buffer): buffer return bytes end,
        encode = function(bytes: buffer): buffer return bytes end,
        validate = function(_bytes: buffer): {string} return {} end,
        view = function(bytes: buffer, _state: ViewState): Node
            return {kind = "button", id = "add", text = tostring(n) .. ":" .. tostring(buffer.readu8(bytes, 0))}
        end,
        update = function(bytes: buffer, _state: ViewState, _action: Action)
            buffer.writeu8(bytes, 0, n + buffer.readu8(bytes, 0)); return nil
        end,
    }
end
"#;
const POLLER: &str = r#"--!strict
local count = 0
return {initial_state = function(): buffer return buffer.create(1) end,
poll = function(state: buffer, context: TelemetryContext): TelemetryFrame
    count += 1
    assert(count == 1) -- module-local state never survives an operation
    local current = buffer.readu8(tango.read_input("memory", 0, 1), 0)
    local previous = buffer.readu8(state, 0)
    buffer.writeu8(state, 0, current)
    local events: {TelemetryEvent} = {}
    if previous ~= current then table.insert(events, {name = "changed", fields = {previous = previous}}) end
    return {values = {value = current, player = context.player, tick = context.tick}, events = events}
end}
"#;

fn capabilities() -> BTreeMap<String, Vec<u8>> {
    files(&[
        (ExportKind::Editor,"one","./editors/one"), (ExportKind::Editor,"two","./editors/two"),
        (ExportKind::GameMode,"one","./modes/one"), (ExportKind::GameMode,"two","./modes/two"),
        (ExportKind::Telemetry,"one","./telemetry"),
    ], "--!strict\nreturn {answer = 42, test = function() assert(require('./editors/one') == require('./editors/one.luau')) end}", &[
        ("factory.luau", FACTORY),
        ("editors/one.luau", "--!strict\nreturn require('../factory')(1)"),
        ("editors/two/init.luau", "--!strict\nreturn require('../../factory')(2)"),
        ("modes/one.luau", "--!strict\nreturn {patch_rom = function(b: buffer): buffer buffer.writeu8(b, 0, 1); return b end}"),
        ("modes/two.luau", "--!strict\nreturn {patch_rom = function(b: buffer): buffer buffer.writeu8(b, 0, 2); return b end}"),
        ("telemetry.luau", POLLER),
    ])
}

#[test]
fn capabilities_select_independently_and_survive_archives_and_rebinding() {
    for kind in ExportKind::ALL {
        let wire = serde_json::to_string(&kind).unwrap();
        assert_eq!(wire, format!("\"{}\"", kind.as_str()));
        assert_eq!(serde_json::from_str::<ExportKind>(&wire).unwrap(), kind);
    }
    let package = Package::load(capabilities()).unwrap();
    let profile = resolve(&[package.clone()], "games").unwrap();
    profile.test().unwrap();
    assert_eq!(profile.selected_export(ExportKind::Editor), None);
    assert_eq!(profile.selected_export(ExportKind::GameMode), Some("one"));
    assert_eq!(profile.selected_export(ExportKind::Telemetry), Some("one"));
    assert!(Document::open(profile.clone(), &[0])
        .err()
        .unwrap()
        .to_string()
        .contains("multiple editor"));
    assert_eq!(profile.patch_rom(&[0]).unwrap(), [1]);
    assert!(profile.clone().with_editor("ONE").is_err());
    assert!(profile.clone().with_telemetry("two").is_err());
    let editor = profile.clone().with_editor("one").unwrap();
    // The default gamemode does not prevent independently selecting an editor.
    assert_eq!(Document::open(editor.clone(), &[7]).unwrap().encode().unwrap(), [7]);
    let selected = editor.clone().with_gamemode("two").unwrap();
    assert_eq!(selected.selected_export(ExportKind::Editor), Some("one"));
    assert_eq!(selected.patch_rom(&[0]).unwrap(), [2]);
    assert_ne!(selected.digest(), editor.digest());
    assert_eq!(
        selected.digest(),
        profile
            .clone()
            .with_gamemode("two")
            .unwrap()
            .with_editor("one")
            .unwrap()
            .digest()
    );
    for (name, expected) in [("one", 1), ("two", 2)] {
        let mut doc = Document::open(selected.clone().with_editor(name).unwrap(), &[0]).unwrap();
        doc.dispatch(Action::activate("add", "")).unwrap();
        doc.set_locale("ja-JP").unwrap();
        doc.dispatch(Action::activate("add", "")).unwrap();
        assert_eq!(doc.encode().unwrap(), [expected * 2]);
        doc.undo().unwrap();
        assert_eq!(doc.encode().unwrap(), [expected]);
    }
    let archived = Package::from_archive(&package.to_archive().unwrap()).unwrap();
    let reopened = resolve(&[archived], "games")
        .unwrap()
        .with_editor("one")
        .unwrap()
        .with_gamemode("two")
        .unwrap();
    assert_eq!(selected.digest(), reopened.digest());
    reopened.test().unwrap();
}

#[test]
fn unused_capabilities_are_not_executed_and_recognition_belongs_to_the_editor() {
    let mut contents = capabilities();
    contents.insert(
        "modes/one.luau".into(),
        b"--!strict\nerror('unused mode'); return {}".to_vec(),
    );
    contents.insert("telemetry.luau".into(), b"--!strict\nerror('unused telemetry'); return {poll = function(): TelemetryFrame return {values = {}, events = {}} end}".to_vec());
    let profile = resolve(&[Package::load(contents).unwrap()], "games")
        .unwrap()
        .with_editor("one")
        .unwrap();
    assert_eq!(Document::open(profile.clone(), &[9]).unwrap().encode().unwrap(), [9]);
    assert!(!profile.supports_editor(&[7]).unwrap());
    for (body, error) in [
        (
            "local matches = buffer.readu8(rom, 0) == 7; buffer.writeu8(rom, 0, 0); return matches",
            false,
        ),
        ("return 1", true),
        ("error('bad detector')", true),
    ] {
        let source = format!("--!strict\nlocal editor = require('./factory')(1);\n(editor :: any).detect_rom = function(rom: buffer): any {body} end\nreturn editor");
        let p = resolve(
            &[Package::load(files(
                &[(ExportKind::Editor, "main", "./init")],
                &source,
                &[("factory.luau", FACTORY)],
            ))
            .unwrap()],
            "games",
        )
        .unwrap();
        let rom = [7];
        let result = p.supports_editor(&rom);
        if error {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap());
            assert!(!p.supports_editor(&[8]).unwrap());
        }
        assert_eq!(rom, [7]);
    }
}

#[test]
fn every_export_uses_sandboxed_resolution_and_names_are_scoped_by_kind() {
    for kind in ExportKind::ALL {
        for path in [
            "./missing",
            "../export",
            "./../export",
            "@games/export",
            "export",
            "/export",
            "./export.lua",
            "./export\\init.luau",
            "./export:invalid",
            "./export//init",
            "",
        ] {
            assert!(
                Package::load(files(
                    &[(kind, "main", path)],
                    "--!strict\nreturn {}",
                    &[("export.luau", "--!strict\nreturn {}")]
                ))
                .is_err(),
                "{path}"
            );
        }
    }
    let modules = [
        ("export.luau", "--!strict\nreturn {}"),
        ("export/init.luau", "--!strict\nreturn {}"),
        ("export/quoted\"保存\u{2028}.luau", "--!strict\nreturn {}"),
    ];
    assert!(Package::load(files(
        &[(ExportKind::GameMode, "main", "./export")],
        "--!strict\nreturn {}",
        &modules
    ))
    .is_err());
    for path in [
        "./export.luau",
        "./export/init.luau",
        "./export/quoted\"保存\u{2028}.luau",
    ] {
        let p = resolve(
            &[Package::load(files(
                &[(ExportKind::GameMode, "main", path)],
                "--!strict\nreturn 42",
                &modules,
            ))
            .unwrap()],
            "games",
        )
        .unwrap();
        assert_eq!(p.patch_rom(&[3]).unwrap(), [3]);
    }
    for names in [
        vec!["main", "main"],
        vec!["../main"],
        vec![""],
        vec!["a/b"],
        vec!["main"; 65],
    ] {
        let exports: Vec<_> = names
            .into_iter()
            .map(|name| (ExportKind::GameMode, name, "./init"))
            .collect();
        assert!(Package::load(files(&exports, "--!strict\nreturn {}", &[])).is_err());
    }
    // Same export name in different categories is intentional.
    Package::load(capabilities()).unwrap();
    for old in ["driver", "training", "ruleset", "match_mode", "game_mode", "netplay"] {
        let mut contents = files(&[], "--!strict\nreturn {}", &[]);
        contents
            .get_mut("package.toml")
            .unwrap()
            .extend_from_slice(format!("\n[[{old}]]\nname='main'\npath='./init'\n").as_bytes());
        assert!(Package::load(contents).is_err());
    }
}

#[test]
fn every_declared_contract_is_checked_including_unselected_dependency_exports() {
    for (path, kind, bad) in [
        ("editors/two/init.luau", "editor", "--!strict\nreturn {}"),
        (
            "modes/two.luau",
            "gamemode",
            "--!strict\nreturn {patch_rom = function(b: buffer): number return 1 end}",
        ),
        (
            "telemetry.luau",
            "telemetry",
            "--!strict\nreturn {poll = function(): number return 1 end}",
        ),
    ] {
        let mut contents = capabilities();
        contents.insert(path.into(), bad.as_bytes().to_vec());
        let error = Package::load(contents.clone()).err().unwrap();
        assert!(
            error.to_string().contains(&format!("{path}:1:1: {kind} contract")),
            "{error}"
        );
        contents
            .get_mut("package.toml")
            .unwrap()
            .extend_from_slice(b"\n[dependencies]\ncommon='1.0.0'\n");
        let bad = Package::load(contents).unwrap();
        let mut common = files(&[], "--!strict\nreturn {}", &[]);
        common.insert(
            "package.toml".into(),
            b"api=1\nname='common'\nversion='1.0.0'\n".to_vec(),
        );
        let error = resolve(&[bad, Package::load(common).unwrap()], "games").err().unwrap();
        assert!(error.to_string().contains(&format!("{kind} contract")), "{error}");
    }
    for (kind, value) in [
        (ExportKind::Editor, "{decode = true}"),
        (ExportKind::Telemetry, "{poll = true}"),
        (ExportKind::GameMode, "{patch_rom = true}"),
        (ExportKind::GameMode, "{setup = true}"),
        (ExportKind::GameMode, "{on_hook = function() return {} end}"),
    ] {
        let source = format!("--!strict\nreturn {value} :: any");
        let p = resolve(
            &[Package::load(files(
                &[(kind, "main", "./export")],
                "--!strict\nreturn {test = function() require('./export') end}",
                &[("export.luau", &source)],
            ))
            .unwrap()],
            "games",
        )
        .unwrap();
        assert!(p.test().is_err());
    }
}

#[test]
fn telemetry_has_explicit_rewindable_state_and_bounded_serde_output() {
    let profile = resolve(&[Package::load(capabilities()).unwrap()], "games")
        .unwrap()
        .with_inputs(tango_script::Inputs::new([("memory".into(), vec![7])].into()).unwrap());
    let context = TelemetryContext {
        tick: 12,
        round: 1,
        player: 1,
    };
    let state = profile.initial_telemetry_state().unwrap();
    assert_eq!(state, [0]);
    let sample = profile.poll_telemetry(&state, context).unwrap();
    assert_eq!(state, [0]);
    assert_eq!(sample.state, [7]);
    assert_eq!(sample.frame.events[0].name, "changed");
    assert_eq!(sample.frame.values["value"], TelemetryValue::Number(7.0));
    assert!(profile
        .poll_telemetry(&sample.state, context)
        .unwrap()
        .frame
        .events
        .is_empty());
    assert_eq!(sample, profile.poll_telemetry(&state, context).unwrap());
    assert!(profile.poll_telemetry(&[0; 65537], context).is_err());
    assert!(profile
        .poll_telemetry(&state, TelemetryContext { player: 0, ..context })
        .is_err());
    for frame in [
        "{values = {value = 0/0}, events = {}}",
        "{values = {}, events = {[2] = {name = 'x', fields = {}}}}",
        "{values = {}, events = {}, extra = true}",
        "{values = {bad = function() end}, events = {}}",
        "{values = {big = string.rep('x', 4097)}, events = {}}",
    ] {
        let source = format!("--!strict\nreturn {{poll = function(state: buffer): any buffer.writeu8(state, 0, 99); return {frame} end}}");
        let p = resolve(
            &[Package::load(files(&[(ExportKind::Telemetry, "main", "./init")], &source, &[])).unwrap()],
            "games",
        )
        .unwrap();
        assert!(p.poll_telemetry(&state, context).is_err(), "{frame}");
        assert_eq!(state, [0]);
    }
}

#[test]
fn dependencies_are_ordinary_modules_and_never_implicitly_export_capabilities() {
    let base = Package::load(capabilities()).unwrap();
    let mut extension = files(
        &[
            (ExportKind::Editor, "custom", "./editor"),
            (ExportKind::GameMode, "custom", "./mode"),
        ],
        "--!strict\nreturn {test = function() assert(require('@games').answer == 42) end}",
        &[
            ("editor.luau", "--!strict\nreturn require('@games/editors/two')"),
            (
                "mode.luau",
                r#"--!strict
local base = require("@games/modes/one")
return {patch_rom = function(b: buffer): buffer local out = base.patch_rom(b); buffer.writeu8(out, 0, buffer.readu8(out, 0) + 10); return out end}
"#,
            ),
        ],
    );
    let manifest = String::from_utf8(extension["package.toml"].clone())
        .unwrap()
        .replace("name = \"games\"", "name = \"extension\"");
    extension.insert(
        "package.toml".into(),
        format!("{manifest}\n[dependencies]\ngames='1.0.0'\n").into_bytes(),
    );
    let p = resolve(&[base.clone(), Package::load(extension).unwrap()], "extension").unwrap();
    p.test().unwrap();
    assert_eq!(p.patch_rom(&[0]).unwrap(), [11]);
    assert_eq!(p.selected_export(ExportKind::Telemetry), None);
    assert!(p.clone().with_telemetry("one").is_err());
    assert!(p.clone().with_editor("one").is_err());
    let mut doc = Document::open(p, &[0]).unwrap();
    doc.dispatch(Action::activate("add", "")).unwrap();
    assert_eq!(doc.encode().unwrap(), [2]);
}
