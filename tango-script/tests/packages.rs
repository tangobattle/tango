use tango_script::{Action, Document, Package, PackageRef, Profile};

const SOURCE: &str = r#"--!strict
return {
        decode = function(bytes: buffer): buffer return bytes end,
        encode = function(bytes: buffer): buffer return bytes end,
        validate = function(_bytes: buffer): {string} return {} end,
        view = function(bytes: buffer, _state: ViewState): Node
            return { kind = "input", id = "value", text = "Value", value = tostring(buffer.readu8(bytes, 0)) }
        end,
        update = function(bytes: buffer, _state: ViewState, action: Action)
    if action.kind ~= "change" and action.kind ~= "activate" then return nil end
            local value = tonumber(action.value)
            assert(value and value >= 0 and value <= 255, "invalid byte")
            buffer.writeu8(bytes, 0, value)
            return nil
        end,
    }
"#;

fn load(id: &str, source: &str, dependency: Option<&str>) -> tango_script::Result<Package> {
    let mut manifest =
        serde_json::json!({"api":1,"editor":[{"name":"main","path":"./init"}],"name":id,"version":"1.0.0"});
    if let Some(dependency) = dependency {
        manifest["dependencies"] = serde_json::json!({(dependency):"1.0.0"});
    }
    Package::load(
        [
            ("package.toml".into(), toml::to_string(&manifest).unwrap().into_bytes()),
            ("init.luau".into(), source.as_bytes().to_vec()),
        ]
        .into(),
    )
}

fn profile(packages: &[Package], id: &str) -> Profile {
    Profile::resolve(
        packages,
        &PackageRef {
            name: id.into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
}

#[test]
fn read_only_context_allows_view_state_but_rejects_document_changes_atomically() {
    let source = r#"--!strict
return {
    decode = function(bytes: buffer): buffer return bytes end,
    encode = function(bytes: buffer): buffer return bytes end,
    validate = function(_bytes: buffer): {string} return {} end,
    view = function(_bytes: buffer, state: ViewState, context: EditorContext): Node
        assert(context.read_only)
        local children: {Node} = {
            {kind = "text", text = state.group or "initial"},
            {kind = "button", id = "group", text = "Group"},
            {kind = "button", id = "bad", text = "Mutate"},
        }
        return {kind = "column", children = children}
    end,
    update = function(bytes: buffer, state: ViewState, action: Action, context: EditorContext)
        assert(context.read_only)
        context.read_only = false -- This cannot change the host's capabilities.
        state.group = action.id
        if action.id == "bad" then buffer.writeu8(bytes, 0, 99) end
        return nil
    end,
}
"#;
    let package = load("test.readonly", source, None).unwrap();
    let mut doc = Document::open_with_context(
        profile(&[package], "test.readonly"),
        &[7],
        tango_script::EditorContext {
            read_only: true,
            ..Default::default()
        },
    )
    .unwrap();
    let initial = doc.view().clone();
    doc.dispatch(Action::activate("group", "")).unwrap();
    assert_ne!(doc.view(), &initial);
    let grouped = doc.view().clone();
    let error = doc.dispatch(Action::activate("bad", "")).unwrap_err();
    assert!(error.to_string().contains("read-only"), "{error}");
    assert_eq!(doc.view(), &grouped);
    assert_eq!(doc.encode().unwrap(), [7]);
    assert!(doc.is_read_only() && !doc.is_dirty() && !doc.can_undo() && !doc.can_redo());
    doc.set_locale("ja-JP").unwrap();
    assert_eq!(doc.view(), &grouped);
}

#[test]
fn checked_editor_roundtrip_and_transactions() {
    let package = load("test.bytes", SOURCE, None).unwrap();
    let mut doc = Document::open(profile(&[package], "test.bytes"), &[7, 8, 9]).unwrap();
    assert!(!doc.is_dirty());
    doc.dispatch(Action::change("value", "21")).unwrap();
    assert_eq!(doc.encode().unwrap(), [21, 8, 9]);
    assert!(doc.is_dirty());
    doc.undo().unwrap();
    assert_eq!(doc.encode().unwrap(), [7, 8, 9]);
    doc.redo().unwrap();
    assert_eq!(doc.encode().unwrap(), [21, 8, 9]);
    doc.mark_saved();
    assert!(!doc.is_dirty());
    assert!(doc.dispatch(Action::change("value", "999")).is_err());
    assert_eq!(doc.encode().unwrap(), [21, 8, 9]);
    assert!(doc.dispatch(Action::change("hidden", "12")).is_err());
}

#[test]
fn undo_discards_drafts_that_would_hide_the_restored_document() {
    let source = r#"--!strict
return {
        decode = function(bytes: buffer): buffer return bytes end,
        encode = function(bytes: buffer): buffer return bytes end,
        validate = function(_bytes: buffer): {string} return {} end,
        view = function(bytes: buffer, state: ViewState): Node
            local children: {Node} = {
                {kind = "input", id = "draft", value = state.draft or tostring(buffer.readu8(bytes, 0))},
                {kind = "button", id = "apply", value = "", text = "Apply"},
            }
            return {kind = "column", children = children}
        end,
        update = function(bytes: buffer, state: ViewState, action: Action)
    if action.kind ~= "change" and action.kind ~= "activate" then return nil end
            if action.id == "draft" then state.draft = action.value; return nil end
            buffer.writeu8(bytes, 0, tonumber(state.draft or "0") or 0)
            state.draft = nil
            return nil
        end,
    }
"#;
    let package = load("test.drafts", source, None).unwrap();
    let mut doc = Document::open(profile(&[package], "test.drafts"), &[7]).unwrap();
    let initial_view = doc.view().clone();
    doc.dispatch(Action::change("draft", "21")).unwrap();
    assert!(!doc.is_dirty());
    doc.dispatch(Action::activate("apply", "")).unwrap();
    let edited_view = doc.view().clone();
    assert_eq!(doc.encode().unwrap(), [21]);
    doc.undo().unwrap();
    assert_eq!(doc.view(), &initial_view);
    assert_eq!(doc.encode().unwrap(), [7]);
    doc.redo().unwrap();
    assert_eq!(doc.view(), &edited_view);
    assert_eq!(doc.encode().unwrap(), [21]);
}

#[test]
fn rejects_type_errors_before_execution() {
    assert!(load("test.bad", &SOURCE.replace("return bytes end", "return 4 end"), None).is_err());
    assert!(load("test.bad", &SOURCE.replace("--!strict", "--!nonstrict"), None).is_err());
}

#[test]
fn user_defined_type_functions_run_in_the_linked_luau_vm() {
    let prelude = r#"--!strict
type function Box(value)
    local box = types.newtable()
    box:setproperty(types.singleton("value"), value)
    return box
end
local box: Box<number> = {value = 7}
assert(box.value == 7)
"#;
    let source = SOURCE.replacen("--!strict", prelude, 1);
    let package = load("type-functions", &source, None).unwrap();
    let document = Document::open(profile(&[package], "type-functions"), &[7]).unwrap();
    assert_eq!(document.encode().unwrap(), [7]);
    let invalid = source.replace("Box<number>", "Box<string>");
    let error = load("type-functions", &invalid, None).err().unwrap().to_string();
    assert!(error.contains("string") && error.contains("number"), "{error}");
}

#[test]
fn simultaneous_package_loads_can_check_and_execute() {
    // Exercise the production synchronization, not a serial test-runner setting.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..12 {
                    let package = load("test.concurrent", SOURCE, None).unwrap();
                    let doc = Document::open(profile(&[package], "test.concurrent"), &[5]).unwrap();
                    assert_eq!(doc.encode().unwrap(), [5]);
                }
            });
        }
    });
}

#[test]
fn editor_extensions_compose_through_ordinary_imports() {
    let base = load("test.base", SOURCE, None).unwrap();
    let extension = load(
        "test.extension",
        r#"--!strict
local base = require("@test.base")
local editor = table.clone(base)
editor.update = function(bytes: buffer, state: ViewState, action: Action)
    base.update(bytes, state, action)
    buffer.writeu8(bytes, 1, 99)
    return nil
end
return editor
"#,
        Some("test.base"),
    )
    .unwrap();
    let selected = profile(&[base.clone(), extension], "test.extension");
    let mut document = Document::open(selected, &[1, 2]).unwrap();
    document.dispatch(Action::change("value", "21")).unwrap();
    assert_eq!(document.encode().unwrap(), [21, 99]);
    let mut original = Document::open(profile(&[base], "test.base"), &[1, 2]).unwrap();
    original.dispatch(Action::change("value", "21")).unwrap();
    assert_eq!(original.encode().unwrap(), [21, 2]);
}

#[test]
fn editor_imports_preserve_extra_values_and_exported_types() {
    let source = SOURCE.replacen(
        "return {",
        "export type Label = {text: string}\nreturn {\n    label = function(n: number): Label return {text = tostring(n)} end,",
        1,
    );
    let base = load("test.bytes", &source, None).unwrap();
    let consumer = r#"--!strict
local base = require("@test.bytes")
local label: base.Label = base.label(21)
assert(label.text == "21")
return table.clone(base)
"#;
    let extension = load("test.extension", consumer, Some("test.bytes")).unwrap();
    assert_eq!(
        profile(&[base.clone(), extension], "test.extension")
            .patch_rom(&[1, 2])
            .unwrap(),
        [1, 2]
    );
    for body in ["local label: base.Label = {text = 21}", "base.label('wrong')"] {
        let source = format!("--!strict\nlocal base = require('@test.bytes')\n{body}\nreturn base");
        let extension = load("test.extension", &source, Some("test.bytes")).unwrap();
        let error = Profile::resolve(
            &[base.clone(), extension],
            &PackageRef {
                name: "test.extension".into(),
                version: "1.0.0".parse().unwrap(),
            },
        )
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("string") && error.contains("number"), "{error}");
    }
}

#[test]
fn rejects_missing_cyclic_and_duplicate_dependencies() {
    let a = load("test.a", SOURCE, Some("test.b")).unwrap();
    let b = load("test.b", SOURCE, Some("test.a")).unwrap();
    let selected = PackageRef {
        name: "test.a".into(),
        version: "1.0.0".parse().unwrap(),
    };
    assert!(Profile::resolve(&[a.clone()], &selected).is_err());
    assert!(Profile::resolve(&[a.clone(), b], &selected).is_err());
    assert!(Profile::resolve(&[a.clone(), a], &selected).is_err());
}

#[test]
fn declaring_a_dependency_does_not_implicitly_apply_its_patch() {
    let base = Package::load([
        ("package.toml".into(), b"api=1\nname='test.base'\nversion='1.0.0'\ndefault_gamemode='main'\n[[gamemode]]\nname='main'\npath='./init'\n".to_vec()),
        ("init.luau".into(), b"--!strict\nreturn {patch_rom = function(bytes: buffer): buffer buffer.writeu8(bytes, 0, 42); return bytes end}".to_vec()),
    ].into()).unwrap();
    assert_eq!(
        profile(&[base.clone()], "test.base").patch_rom(&[1, 2]).unwrap(),
        [42, 2]
    );
    let selected = load("test.selected", SOURCE, Some("test.base")).unwrap();
    assert_eq!(
        profile(&[base, selected], "test.selected").patch_rom(&[1, 2]).unwrap(),
        [1, 2]
    );
}

#[test]
fn archive_roundtrip_preserves_package_identity() {
    let package = load("test.bytes", SOURCE, None).unwrap();
    let bytes = package.to_archive().unwrap();
    let reopened = Package::from_archive(&bytes).unwrap();
    assert_eq!(package.digest(), reopened.digest());
    assert_eq!(bytes, reopened.to_archive().unwrap());
}

#[test]
fn bundled_packages_pass_their_script_tests() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages");
    let mut paths: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    paths.sort();
    let packages: Vec<_> = paths
        .iter()
        .map(|path| {
            let package = load_directory(path);
            assert_eq!(
                path.strip_prefix(&root).unwrap().to_str().unwrap().replace('\\', "/"),
                package.manifest().name
            );
            package
        })
        .collect();
    // Keep stock Luau tooling pointed at exactly these authoring sources.
    // Runtime dependency permissions and versions still come from manifests.
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join(".luaurc")).unwrap()).unwrap();
    let expected: std::collections::BTreeMap<_, _> = packages
        .iter()
        .map(|package| {
            let name = &package.manifest().name;
            (name, format!("./{name}"))
        })
        .collect();
    assert_eq!(config["aliases"], serde_json::to_value(expected).unwrap());
    for package in &packages {
        let manifest = package.manifest();
        let profile = Profile::resolve(
            &packages,
            &PackageRef {
                name: manifest.name.clone(),
                version: manifest.version.clone(),
            },
        )
        .unwrap();
        profile.test().unwrap();
        let locales = root.join(&manifest.name).join("locales");
        if locales.is_dir() {
            for locale in std::fs::read_dir(locales).unwrap() {
                let locale = locale.unwrap().file_name().into_string().unwrap();
                if locale != "en-US" {
                    profile.clone().with_locale(&locale).unwrap().test().unwrap();
                }
            }
        }
    }
}

#[test]
fn generic_controls_roundtrip_typed_events_through_luau_and_serde() {
    use tango_script::ui::{Event, Modifiers, PointerButton, PointerPhase};
    let source = r#"--!strict
return {
    decode = function(bytes: buffer): buffer return bytes end,
    encode = function(bytes: buffer): buffer return bytes end,
    validate = function(_bytes: buffer): {string} return {} end,
    view = function(bytes: buffer, state: ViewState): Node
        local items: {Node} = {}
        for _, label in string.split(state.order or "one,two,three", ",") do
            local item: Node = {kind = "text", text = label}
            table.insert(items, item)
        end
        local children: {Node} = {
            {kind = "checkbox", id = "grid", text = "Grid", checked = bit32.band(buffer.readu8(bytes, 2), 1) ~= 0},
            {kind = "slider", id = "x", number = buffer.readu8(bytes, 0), min = 0, max = 255},
            {kind = "canvas", id = "position", canvas_width = 256, canvas_height = 256,
                pointer = true, keyboard = true, commands = {}},
            {kind = "list", id = "order", reorder = true, children = items},
        }
        return {kind = "column", children = children}
    end,
    update = function(bytes: buffer, state: ViewState, action: Action)
        if action.kind == "reorder" then
            local items = string.split(state.order or "one,two,three", ",")
            local moved = table.remove(items, action.from)
            assert(moved)
            table.insert(items, action.to, moved)
            state.order = table.concat(items, ",")
            return nil
        end
        if action.kind == "toggle" then
            local flags = bit32.band(buffer.readu8(bytes, 2), 0xfe)
            buffer.writeu8(bytes, 2, flags + (if action.checked then 1 else 0))
        elseif action.kind == "slide" then
            buffer.writeu8(bytes, 0, action.number)
        elseif action.kind == "pointer" and action.button == "left" and action.phase == "down" then
            buffer.writeu8(bytes, 0, math.clamp(math.round(action.x), 0, 255))
            buffer.writeu8(bytes, 1, math.clamp(math.round(action.y), 0, 255))
        elseif action.kind == "key" and action.pressed and action.key == "ArrowRight" then
            buffer.writeu8(bytes, 0, math.min(buffer.readu8(bytes, 0) + 1, 255))
        end
        return nil
    end,
}
"#;
    let package = load("controls", source, None).unwrap();
    let mut doc = Document::open(profile(&[package], "controls"), &[10, 20, 0xfe]).unwrap();
    for (id, event) in [
        ("grid", Event::Toggle { checked: true }),
        ("x", Event::Slide { number: 42.0 }),
        (
            "position",
            Event::Pointer {
                phase: PointerPhase::Down,
                x: 30.0,
                y: 50.0,
                button: PointerButton::Left,
                dx: 0.0,
                dy: 0.0,
                modifiers: Modifiers::default(),
            },
        ),
        (
            "position",
            Event::Key {
                key: "ArrowRight".into(),
                pressed: true,
                modifiers: Modifiers::default(),
            },
        ),
        ("order", Event::Reorder { from: 1, to: 3 }),
    ] {
        doc.dispatch(Action { id: id.into(), event }).unwrap();
    }
    let view = serde_json::to_value(doc.view()).unwrap();
    assert_eq!(view["children"][3]["children"][0]["text"], "two");
    assert_eq!(view["children"][3]["children"][2]["text"], "one");
    for (from, to) in [(0, 1), (1, 0), (4, 1), (1, 4), (usize::MAX, 1)] {
        assert!(doc
            .dispatch(Action {
                id: "order".into(),
                event: Event::Reorder { from, to }
            })
            .is_err());
        assert_eq!(serde_json::to_value(doc.view()).unwrap(), view);
    }
    assert_eq!(doc.encode().unwrap(), [31, 50, 0xff]);
    doc.undo().unwrap();
    assert_eq!(doc.encode().unwrap(), [30, 50, 0xff]);
    doc.redo().unwrap();
    assert_eq!(doc.encode().unwrap(), [31, 50, 0xff]);
    assert!(doc.dispatch(Action::change("grid", "true")).is_err());
    assert_eq!(doc.encode().unwrap(), [31, 50, 0xff]);
}

#[test]
fn bounds_script_execution_memory_and_ui_recursion() {
    let looping = SOURCE.replace("return bytes end", "while true do end; return bytes end");
    let package = load("test.loop", &looping, None).unwrap();
    let error = Document::open(profile(&[package], "test.loop"), &[1]).err().unwrap();
    assert!(error.to_string().contains("budget exceeded"), "{error}");

    let allocating = SOURCE.replace("return bytes end", "return buffer.create(256 * 1024 * 1024) end");
    let package = load("test.memory", &allocating, None).unwrap();
    assert!(Document::open(profile(&[package], "test.memory"), &[1]).is_err());

    let cyclic = SOURCE.replace(
        "return { kind = \"input\", id = \"value\", text = \"Value\", value = tostring(buffer.readu8(bytes, 0)) }",
        "local children: {Node} = {}; local node: Node = {kind = \"column\", children = children}; table.insert(children, node); return node",
    );
    let package = load("test.cyclic", &cyclic, None).unwrap();
    let error = Document::open(profile(&[package], "test.cyclic"), &[1]).err().unwrap();
    assert!(error.to_string().contains("recursive table"), "{error}");
}

#[test]
fn rejects_paths_before_reading_outside_package() {
    for path in ["../escape.luau", "/absolute.luau", "a\\b.luau"] {
        let manifest = serde_json::json!({"api":1,"editor":[{"name":"main","path":"./init"}],"name":"test.paths","version":"1.0.0"});
        assert!(Package::load(
            [
                ("package.toml".into(), toml::to_string(&manifest).unwrap().into_bytes()),
                ("init.luau".into(), SOURCE.as_bytes().to_vec()),
                (path.into(), SOURCE.as_bytes().to_vec()),
            ]
            .into()
        )
        .is_err());
    }
}

#[test]
fn observations_cannot_mutate_document_or_survive_as_hidden_state() {
    let source = SOURCE
        .replacen("return {", "local counter = 0\nreturn {", 1)
        .replace(
            "local value = tonumber(action.value)",
            "counter += 1\nlocal value = tonumber(action.value)",
        )
        .replace(
            "buffer.writeu8(bytes, 0, value)",
            "buffer.writeu8(bytes, 0, value + counter)",
        );
    let package = load("test.state", &source, None).unwrap();
    let mut doc = Document::open(profile(&[package], "test.state"), &[1]).unwrap();
    for _ in 0..3 {
        doc.dispatch(Action::change("value", "10")).unwrap();
        assert_eq!(doc.encode().unwrap(), [11]);
    }
}

fn load_directory(root: &std::path::Path) -> Package {
    fn collect(
        root: &std::path::Path,
        path: &std::path::Path,
        files: &mut std::collections::BTreeMap<String, Vec<u8>>,
    ) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), files);
            } else {
                let name = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");
                files.insert(name, std::fs::read(entry.path()).unwrap());
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    collect(root, root, &mut files);
    Package::load(files).unwrap()
}

#[test]
fn bundled_bn5_editor_interactions_use_public_modules() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut packages: Vec<_> = ["gba", "editor-common", "bn-common", "bn5"]
        .into_iter()
        .map(|name| load_directory(&root.join("../packages").join(name)))
        .collect();
    packages.push(load_directory(&root.join("tests/fixtures/bn5-editor")));
    let profile = profile(&packages, "bn5-editor");
    for fixture in std::fs::read_dir(root.join("../packages/bn5/editor/save/fixtures")).unwrap() {
        let path = fixture.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "raw") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap();
        for locale in ["en-US", "ja-JP"] {
            let inputs = tango_script::Inputs::new([("fixture".into(), name.as_bytes().to_vec())].into()).unwrap();
            profile
                .clone()
                .with_inputs(inputs)
                .with_locale(locale)
                .unwrap()
                .test()
                .unwrap();
        }
    }
}

#[test]
fn dependent_package_method_replacements_reach_existing_readers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for (game, extension, template) in [
        ("bn6", "method-extension", "default"),
        ("bn5", "bn5-method-extension", "light"),
    ] {
        let mut packages: Vec<_> = ["gba", "editor-common", "bn-common", game]
            .into_iter()
            .map(|name| load_directory(&root.join("../packages").join(name)))
            .collect();
        packages.push(load_directory(&root.join("tests/fixtures").join(extension)));
        let profile = profile(&packages, extension);
        let rom = profile.patch_rom(&[]).unwrap();
        let profile = profile.with_inputs(tango_script::Inputs::new([("rom".into(), rom)].into()).unwrap());
        // Each callback gets one fresh application of the wrapper even though
        // prototypes are cached across calls.
        profile.test().unwrap();
        profile.test().unwrap();
        let save = profile.create_save(template).unwrap();
        let mut document = Document::open(profile, &save).unwrap();
        document.dispatch(Action::activate("editor.tab:folder", "")).unwrap();
        assert!(serde_json::to_string(document.view()).unwrap().contains("Replacement"));
        assert!(document
            .diagnostics()
            .iter()
            .any(|warning| warning.contains("Replacement")));
    }
}

#[test]
fn bundled_bn4_save_references_run_in_separate_callbacks() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages");
    let packages: Vec<_> = ["editor-common", "gba", "bn-common", "bn4"]
        .into_iter()
        .map(|name| load_directory(&root.join(name)))
        .collect();
    let profile = Profile::resolve(
        &packages,
        &PackageRef {
            name: "bn4".into(),
            version: "0.1.0".parse().unwrap(),
        },
    )
    .unwrap();
    let mut paths: Vec<_> = std::fs::read_dir(root.join("bn4/editor/save/templates"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "raw"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 12);
    for path in paths {
        let name = path.file_stem().unwrap().to_str().unwrap();
        for locale in ["en-US", "ja-JP"] {
            profile
                .clone()
                .with_locale(locale)
                .unwrap()
                .with_inputs(
                    tango_script::Inputs::new([("test.fixture".into(), name.as_bytes().to_vec())].into()).unwrap(),
                )
                .test()
                .unwrap_or_else(|error| panic!("{name}/{locale}: {error}"));
        }
    }
}

#[test]
fn bundled_save_codecs_reject_invalid_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages");
    let packages: Vec<_> = ["editor-common", "gba", "bn-common", "bn4", "bn5"]
        .into_iter()
        .map(|name| load_directory(&root.join(name)))
        .collect();
    for name in ["bn4", "bn5"] {
        let profile = Profile::resolve(
            &packages,
            &PackageRef {
                name: name.into(),
                version: "0.1.0".parse().unwrap(),
            },
        )
        .unwrap();
        let mut cases = vec![
            (1, "Save file is too short"),
            (2, "Save checksum does not match"),
            (3, "Unsupported save shift"),
        ];
        if name == "bn4" {
            cases.extend([
                (4, "Unsupported save shift"),
                (5, "Unrecognized Battle Network 4 save"),
                (6, "Unrecognized Battle Network 4 save"),
            ]);
        }
        for (case, expected) in cases {
            let profile = profile
                .clone()
                .with_inputs(tango_script::Inputs::new([("test.invalid_save".into(), vec![case])].into()).unwrap());
            let error = profile.test().unwrap_err().to_string();
            assert!(error.contains(expected), "{name} invalid save case {case}: {error}");
        }
    }
}

#[test]
fn save_templates_are_optional_typed_and_validated_before_creation() {
    let original = profile(&[load("templates", SOURCE, None).unwrap()], "templates");
    assert!(original.save_templates().unwrap().is_empty());
    assert!(original.create_save("default").is_err());
    let make = |templates: &str, create: &str| {
        let source = SOURCE.replacen("return {", "local editor = {", 1)
            + &format!("\neditor.templates = function(): {{SaveTemplate}} return ({templates} :: any) end\neditor.create_save = function(name: string): buffer {create} end\nreturn editor");
        profile(&[load("templates", &source, None).unwrap()], "templates")
    };
    let good = make(
        "{{name='default', label='New save'}}",
        "return buffer.fromstring('saved')",
    );
    assert_eq!(good.save_templates().unwrap()[0].label, "New save");
    assert_eq!(good.create_save("default").unwrap(), b"saved");
    assert!(good.create_save("missing").is_err());
    for templates in [
        "{{name='default', label='A'}, {name='default', label='B'}}",
        "{{name='', label='A'}}",
        "{{name='default', label='A', extra=true}}",
        "{[2]={name='default', label='A'}}",
    ] {
        assert!(make(templates, "error('must not run')").save_templates().is_err());
    }
    let large = make(
        "{{name='default', label='A'}}",
        "return buffer.create(8 * 1024 * 1024 + 1)",
    );
    assert!(large.create_save("default").is_err());
    let invalid = SOURCE.replacen("return {", "local editor = {", 1)
        + r#"
editor.templates = function() return {{name='default', label='New save'}} end
editor.create_save = function(_name: string): buffer return buffer.create(0) end
editor.decode = function(bytes: buffer): buffer assert(buffer.len(bytes) > 0, 'empty save'); return bytes end
return editor
"#;
    let invalid = profile(&[load("templates", &invalid, None).unwrap()], "templates");
    assert!(invalid
        .create_save("default")
        .unwrap_err()
        .to_string()
        .contains("empty save"));
}
