use std::collections::BTreeMap;
use tango_script::{Package, PackageRef, Profile};

fn files(
    name: &str,
    version: &str,
    source: &str,
    deps: &[(&str, &str)],
    extra: &[(&str, &[u8])],
) -> BTreeMap<String, Vec<u8>> {
    let dependencies: BTreeMap<_, _> = deps.iter().map(|(name, version)| (*name, *version)).collect();
    let manifest = serde_json::json!({
        "api":1, "name":name, "version":version,
        "dependencies":dependencies,
    });
    let mut files: BTreeMap<_, _> = extra.iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
    files.insert("package.toml".into(), toml::to_string(&manifest).unwrap().into_bytes());
    files.insert("init.luau".into(), source.as_bytes().to_vec());
    files
}

fn library(name: &str, source: &str, deps: &[(&str, &str)], extra: &[(&str, &[u8])]) -> Package {
    Package::load(files(name, "1.0.0", source, deps, extra)).unwrap()
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

const COMMON: &str = r#"--!strict
export type Options = {value: number}
return {
    make = function(value: number): Options return {value = value} end,
    contents = function(): string return buffer.tostring(tango.read_file("data.txt")) end,
}
"#;

#[test]
fn resolves_local_package_and_subpath_imports_with_dependency_owned_files() {
    let common = library(
        "test.common",
        COMMON,
        &[],
        &[
            ("data.txt", b"dependency"),
            (
                "math/double.luau",
                b"--!strict\nreturn function(n: number): number return n * 2 end",
            ),
        ],
    );
    let scoped = library(
        "test.tools",
        COMMON,
        &[],
        &[
            ("data.txt", b"scoped"),
            (
                "math/double.luau",
                b"--!strict\nreturn function(n: number): number return n * 2 end",
            ),
        ],
    );
    let consumer = library(
        "test.consumer",
        r#"--!strict
local common = require("@test.common")
local scoped = require("@test.tools")
local double = require("@test.tools/math/double")
local reader = require("./nested/reader")
local options: common.Options = common.make(21)
return {test = function()
    assert(options.value == 21 and double(options.value) == 42)
    assert(common == require("@TEST.COMMON/init.luau") and common ~= scoped)
    assert(scoped.contents() == "scoped")
    assert(reader() == "dependency")
    assert(buffer.tostring(tango.read_file("./data.txt")) == "consumer")
end}
"#,
        &[("test.common", "1.0.0"), ("test.tools", "1.0.0")],
        &[
            ("data.txt", b"consumer"),
            (
                "nested/reader.luau",
                b"--!strict\nlocal reader = require('../reader.luau')\nreturn reader",
            ),
            (
                "reader.luau",
                b"--!strict\nlocal common = require('@TEST.COMMON')\nreturn common.contents",
            ),
        ],
    );
    let profile = resolve(&[consumer.clone(), common.clone(), scoped.clone()], "test.consumer").unwrap();
    profile.test().unwrap();
    assert_eq!(profile.packages().count(), 3);
    let bundled = [consumer, common, scoped]
        .iter()
        .map(|p| Package::from_archive(&p.to_archive().unwrap()).unwrap())
        .collect::<Vec<_>>();
    let from_archives = resolve(&bundled, "test.consumer").unwrap();
    assert_eq!(profile.digest(), from_archives.digest());
    from_archives.test().unwrap();
}

#[test]
fn editor_alias_configuration_cannot_override_the_selected_dependency_graph() {
    let common = library("test.common", COMMON, &[], &[("data.txt", b"dependency")]);
    let decoy = library(
        "test.decoy",
        "--!strict\nreturn {make = function(): string return 'decoy' end}",
        &[],
        &[],
    );
    let config = br#"{"aliases":{"test.common":"../test.decoy","test.hidden":"../test.decoy"}}"#;
    let consumer = library(
        "test.consumer",
        "--!strict\nlocal common = require('@test.common')\nlocal value: common.Options = common.make(21)\nreturn {test = function() assert(value.value == 21 and common.contents() == 'dependency') end}",
        &[("test.common", "1.0.0")],
        &[(".luaurc", config)],
    );
    resolve(&[common.clone(), decoy.clone(), consumer], "test.consumer")
        .unwrap()
        .test()
        .unwrap();
    for source in [
        "--!strict\nreturn require('@test.hidden')",
        "--!strict\nreturn require('@test.decoy')",
    ] {
        let consumer = library(
            "test.consumer",
            source,
            &[("test.common", "1.0.0")],
            &[(".luaurc", config)],
        );
        assert!(resolve(&[common.clone(), decoy.clone(), consumer], "test.consumer").is_err());
    }
    let error = Package::load(files(
        "test.consumer",
        "1.0.0",
        "--!strict\nreturn {}",
        &[("test.common", "1.0.0"), ("TEST.COMMON", "2.0.0")],
        &[],
    ))
    .err()
    .unwrap();
    assert!(error.to_string().contains("unique ignoring ASCII case"), "{error}");
}

#[test]
fn directory_modules_keep_types_and_identity_across_package_imports() {
    let common = library(
        "test.common",
        "--!strict\nlocal editor = require('./editor')\nexport type Options = editor.Options\nreturn editor",
        &[],
        &[
            (
                "editor/init.luau",
                b"--!strict\nlocal fields = require('./fields')\nexport type Options = fields.Options\nreturn fields",
            ),
            ("editor/fields.luau", COMMON.as_bytes()),
            ("data.txt", b"dependency"),
        ],
    );
    let source = r#"--!strict
local common = require("@test.common")
local editor = require("@test.common/editor")
local explicit = require("@test.common/editor/init.luau")
local options: editor.Options = common.make(21)
return {test = function()
    assert(common == editor and editor == explicit)
    assert(options.value == 21 and editor.contents() == "dependency")
end}
"#;
    let consumer = library("test.consumer", source, &[("test.common", "1.0.0")], &[]);
    let packages = [common.clone(), consumer];
    resolve(&packages, "test.consumer").unwrap().test().unwrap();
    let archived = packages
        .iter()
        .map(|p| Package::from_archive(&p.to_archive().unwrap()).unwrap())
        .collect::<Vec<_>>();
    resolve(&archived, "test.consumer").unwrap().test().unwrap();
    for body in [
        "local bad: editor.Options = {value = 'wrong'}",
        "editor.make('wrong')",
        "local bad: string = editor.make(21).value",
    ] {
        // Check an unused nested consumer too: its imports must retain the
        // same types as the selected entry rather than falling back to any.
        let nested = format!("--!strict\nlocal editor = require('@test.common/editor')\n{body}\nreturn {{}}");
        let consumer = library(
            "test.consumer",
            "--!strict\nreturn {}",
            &[("test.common", "1.0.0")],
            &[("unused/consumer.luau", nested.as_bytes())],
        );
        let error = resolve(&[common.clone(), consumer], "test.consumer")
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("test.consumer@1.0.0/unused/consumer.luau"), "{error}");
        assert!(error.contains("string") && error.contains("number"), "{error}");
    }
}

#[test]
fn directory_resolution_rejects_ambiguity_and_escape_in_both_checker_and_runtime() {
    let common = library(
        "test.common",
        "--!strict\nreturn {}",
        &[],
        &[
            ("component.luau", b"--!strict\nreturn {value = 1}"),
            ("component/init.luau", b"--!strict\nreturn {value = 2}"),
        ],
    );
    for request in [
        "@test.common/component",
        "@test.common/component/../../../escape",
        "./nested/../../escape",
    ] {
        let source = format!("--!strict\nreturn require({request:?})");
        let consumer = library("test.consumer", &source, &[("test.common", "1.0.0")], &[]);
        assert!(
            resolve(&[common.clone(), consumer], "test.consumer").is_err(),
            "{request}"
        );
        let source = format!("--!strict\nlocal load = require\nreturn {{test = function() load({request:?}) end}}");
        let consumer = library("test.consumer", &source, &[("test.common", "1.0.0")], &[]);
        assert!(
            resolve(&[common.clone(), consumer], "test.consumer")
                .unwrap()
                .test()
                .is_err(),
            "{request}"
        );
    }
    let consumer = library(
        "test.consumer",
        r#"--!strict
local file = require("@test.common/component.luau")
local directory = require("@test.common/component/init.luau")
return {test = function() assert(file.value == 1 and directory.value == 2) end}
"#,
        &[("test.common", "1.0.0")],
        &[],
    );
    resolve(&[common, consumer], "test.consumer").unwrap().test().unwrap();
}

#[test]
fn checker_keeps_different_dependency_versions_types_separate() {
    let one = Package::load(files(
        "test.common",
        "1.0.0",
        "--!strict\nreturn function(value: number): number return value + 1 end",
        &[],
        &[],
    ))
    .unwrap();
    let two = Package::load(files(
        "test.common",
        "2.0.0",
        "--!strict\nreturn function(value: string): string return value .. '!' end",
        &[],
        &[],
    ))
    .unwrap();
    let first = library(
        "test.first",
        "--!strict\nreturn require('@test.common')",
        &[("test.common", "1.0.0")],
        &[],
    );
    let second = library(
        "test.second",
        "--!strict\nreturn require('@test.common')",
        &[("test.common", "2.0.0")],
        &[],
    );
    for (body, valid) in [
        (
            "local n: number = one(20)\nlocal s: string = two('hello')\nassert(n == 21 and s == 'hello!')",
            true,
        ),
        ("one('wrong')", false),
        ("two(42)", false),
        ("local bad: string = one(42)", false),
        ("local bad: number = two('wrong')", false),
    ] {
        let source = format!("--!strict\nlocal one = require('@test.first')\nlocal two = require('@test.second')\nreturn {{test = function() {body} end}}");
        let consumer = library(
            "test.consumer",
            &source,
            &[("test.first", "1.0.0"), ("test.second", "1.0.0")],
            &[],
        );
        let result = resolve(
            &[one.clone(), two.clone(), first.clone(), second.clone(), consumer],
            "test.consumer",
        );
        if valid {
            result.unwrap().test().unwrap()
        } else {
            let error = result.err().unwrap().to_string();
            assert!(error.contains("string") && error.contains("number"), "{body}: {error}");
        }
    }
}

#[test]
fn imported_function_types_and_exported_aliases_are_checked_before_execution() {
    for body in [
        "local options: common.Options = {value = 'wrong'}",
        "common.make('wrong')",
        "local bad: string = common.make(1).value",
    ] {
        let common = library("test.common", COMMON, &[], &[("data.txt", b"common")]);
        let consumer = library(
            "test.consumer",
            &format!("--!strict\nlocal common = require('@test.common')\n{body}\nreturn {{}}"),
            &[("test.common", "1.0.0")],
            &[],
        );
        let error = resolve(&[common, consumer], "test.consumer").err().unwrap().to_string();
        assert!(error.contains("test.consumer@1.0.0/init.luau"), "{error}");
        assert!(error.contains("string") && error.contains("number"), "{error}");
    }
}

#[test]
fn checker_rejects_undeclared_and_escaping_imports() {
    let common = library("test.common", "--!strict\nreturn {}", &[], &[]);
    for request in [
        "../escape",
        "../../escape",
        "/etc/passwd",
        "C:/outside",
        "./a\\b",
        "./missing",
        "missing",
        "test.common",
        "../test.common",
        "@undeclared",
        "@",
        "@@test.common",
        "@test.common/",
        "@test.common/../main",
        "@test.common/../../escape",
        "@test.tools/../main",
        "@test.common//main",
        "./nested/../../escape",
    ] {
        let source = format!(
            "--!strict\nlocal m = require({})\nreturn {{}}",
            serde_json::to_string(request).unwrap()
        );
        let consumer = library("test.consumer", &source, &[("test.common", "1.0.0")], &[]);
        assert!(
            resolve(&[common.clone(), consumer], "test.consumer").is_err(),
            "{request}"
        );
    }
}

#[test]
fn runtime_resolver_also_confines_computed_imports() {
    let common = library("test.common", "--!strict\nreturn {value = 7}", &[], &[]);
    for request in [
        "../escape",
        "/etc/passwd",
        "undeclared",
        "test.common",
        "../test.common",
        "@undeclared",
        "@",
        "@@test.common",
        "@test.common/",
        "@test.common/../main",
        "./nested/../../main",
    ] {
        let source = format!(
            r#"--!strict
local load = require
return {{test = function() load({}) end}}
"#,
            serde_json::to_string(request).unwrap()
        );
        let consumer = library("test.consumer", &source, &[("test.common", "1.0.0")], &[]);
        let profile = resolve(&[common.clone(), consumer], "test.consumer").unwrap();
        assert!(profile.test().is_err(), "{request}");
    }
}

#[test]
fn packages_do_not_gain_access_to_transitive_or_unrelated_dependencies() {
    let leaf = library("test.leaf", "--!strict\nreturn {}", &[], &[]);
    let common = library(
        "test.common",
        "--!strict\nlocal leaf = require('@test.leaf')\nreturn leaf",
        &[("test.leaf", "1.0.0")],
        &[],
    );
    let consumer = library(
        "test.consumer",
        "--!strict\nreturn require('@test.leaf')",
        &[("test.common", "1.0.0")],
        &[],
    );
    assert!(resolve(&[leaf, common, consumer], "test.consumer").is_err());
}

#[test]
fn transitive_dependencies_can_select_different_versions_of_one_package() {
    let one = Package::load(files("test.common", "1.0.0", "--!strict\nreturn {value = 1}", &[], &[])).unwrap();
    let two = Package::load(files("test.common", "2.0.0", "--!strict\nreturn {value = 2}", &[], &[])).unwrap();
    let first = library(
        "test.first",
        "--!strict\nreturn require('@test.common')",
        &[("test.common", "1.0.0")],
        &[],
    );
    let second = library(
        "test.second",
        "--!strict\nreturn require('@test.common')",
        &[("test.common", "2.0.0")],
        &[],
    );
    let consumer = library(
        "test.consumer",
        r#"--!strict
local one = require("@test.first")
local two = require("@test.second")
return {test = function()
    assert(one.value == 1 and two.value == 2)
    assert(one ~= two)
end}
"#,
        &[("test.first", "1.0.0"), ("test.second", "1.0.0")],
        &[],
    );
    let packages = [one, two, first, second, consumer];
    let original = resolve(&packages, "test.consumer").unwrap();
    original.test().unwrap();
    let mut reversed = packages.to_vec();
    reversed.reverse();
    assert_eq!(original.digest(), resolve(&reversed, "test.consumer").unwrap().digest());
}

#[test]
fn all_files_are_readable_and_participate_in_identity_without_declarations() {
    let source = r#"--!strict
return {test = function()
    assert(buffer.tostring(tango.read_file("notes/素材 file.txt")) == "hello")
    assert(buffer.tostring(tango.read_file("./notes/../notes/素材 file.txt")) == "hello")
    local copy = tango.read_file("notes/素材 file.txt")
    buffer.writeu8(copy, 0, 42)
    assert(buffer.tostring(tango.read_file("notes/素材 file.txt")) == "hello")
    assert(string.find(buffer.tostring(tango.read_file("package.toml")), "test.files") ~= nil)
    assert(string.find(buffer.tostring(tango.read_file("init.luau")), "--!strict", 1, true) ~= nil)
end}
"#;
    let mut contents = files("test.files", "1.0.0", source, &[], &[("notes/素材 file.txt", b"hello")]);
    let package = Package::load(contents.clone()).unwrap();
    resolve(&[package.clone()], "test.files").unwrap().test().unwrap();
    let bytes = package.to_archive().unwrap();
    let reopened = Package::from_archive(&bytes).unwrap();
    assert_eq!(reopened.digest(), package.digest());
    assert_eq!(bytes, reopened.to_archive().unwrap());
    resolve(&[reopened], "test.files").unwrap().test().unwrap();
    contents.insert("notes/素材 file.txt".into(), b"changed".to_vec());
    assert_ne!(Package::load(contents.clone()).unwrap().digest(), package.digest());
    contents.insert("unused.dat".into(), vec![0, 1, 2]);
    assert_ne!(Package::load(contents).unwrap().digest(), package.digest());
}

#[test]
fn file_reads_cannot_escape_the_owning_package() {
    for path in [
        "../data.txt",
        "a/../../data.txt",
        "/data.txt",
        "C:/data.txt",
        "a\\data.txt",
        "test.common/data.txt",
    ] {
        let source = format!(
            "--!strict\nreturn {{test = function() tango.read_file({}) end}}",
            serde_json::to_string(path).unwrap()
        );
        let package = library("test.files", &source, &[], &[("data.txt", b"own data")]);
        assert!(resolve(&[package], "test.files").unwrap().test().is_err(), "{path}");
    }
}

#[test]
fn unused_luau_files_are_checked_and_legacy_manifest_fields_are_rejected() {
    let mut contents = files(
        "test.files",
        "1.0.0",
        "--!strict\nreturn {}",
        &[],
        &[("unused.luau", b"--!strict\nlocal value: number = 'bad'\nreturn value")],
    );
    assert!(Package::load(contents.clone()).is_err());
    contents.remove("unused.luau");
    for field in [
        "assets = []",
        "modules = []",
        "extends = {name = 'test.base', version = '1.0.0'}",
        "entry = 'init.luau'",
    ] {
        let mut contents = contents.clone();
        contents
            .get_mut("package.toml")
            .unwrap()
            .splice(0..0, format!("{field}\n").bytes());
        assert!(Package::load(contents).is_err(), "{field}");
    }
}

#[test]
fn archives_reject_escaping_paths_links_and_conflicting_file_trees() {
    use std::io::{Cursor, Write};
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in ["../outside", "/outside", "C:/outside", "a\\outside", "a//outside"] {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive.start_file(name, options).unwrap();
        archive.write_all(b"bad").unwrap();
        let error = Package::from_archive(&archive.finish().unwrap().into_inner())
            .err()
            .unwrap();
        assert!(error.to_string().contains("invalid or duplicate"), "{name}: {error}");
    }
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive.add_symlink("linked", "../outside", options).unwrap();
    let error = Package::from_archive(&archive.finish().unwrap().into_inner())
        .err()
        .unwrap();
    assert!(error.to_string().contains("regular files"), "{error}");

    for nested in ["file/child", "file/dir/"] {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive.start_file("file", options).unwrap();
        archive.write_all(b"file").unwrap();
        if nested.ends_with('/') {
            archive.add_directory(nested, options).unwrap();
        } else {
            archive.start_file(nested, options).unwrap();
        }
        let error = Package::from_archive(&archive.finish().unwrap().into_inner())
            .err()
            .unwrap();
        assert!(error.to_string().contains("both a file and a directory"), "{error}");
    }
}

#[test]
fn archive_expansion_has_a_total_budget_across_files() {
    use std::io::{Cursor, Write};
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let chunk = vec![0; 1024 * 1024];
    for name in ["first", "second"] {
        archive.start_file(name, options).unwrap();
        for _ in 0..32 {
            archive.write_all(&chunk).unwrap();
        }
        archive.write_all(&[0]).unwrap();
    }
    let error = Package::from_archive(&archive.finish().unwrap().into_inner())
        .err()
        .unwrap();
    assert!(error.to_string().contains("byte limit"), "{error}");
}

#[test]
fn cyclic_module_imports_cannot_run() {
    let contents = files(
        "test.cycle",
        "1.0.0",
        "--!strict\nreturn require('./other')",
        &[],
        &[("other.luau", b"--!strict\nreturn require('./init')")],
    );
    match Package::load(contents) {
        Ok(package) => match resolve(&[package], "test.cycle") {
            Ok(profile) => assert!(profile.test().is_err()),
            Err(tango_script::Error::Typecheck(_)) => {}
            Err(error) => panic!("{error}"),
        },
        Err(tango_script::Error::Typecheck(_)) => {}
        Err(error) => panic!("{error}"),
    }
}

#[test]
fn manifest_names_are_flat_and_dependencies_are_name_to_version() {
    let contents = files("common", "1.0.0", "--!strict\nreturn {}", &[], &[]);
    let manifest = String::from_utf8(contents["package.toml"].clone()).unwrap();
    let package = Package::load(contents.clone()).unwrap();
    assert_eq!(package.manifest().name, "common");
    assert_eq!(resolve(&[package], "common").unwrap().display_name(), "common");

    for name in ["namespace/package", "../escape", "/absolute", "a\\b", "C:drive"] {
        let mut contents = contents.clone();
        contents.insert(
            "package.toml".into(),
            manifest
                .replace("name = \"common\"", &format!("name = {name:?}"))
                .into_bytes(),
        );
        assert!(Package::load(contents).is_err(), "{name}");
    }
    let mut obsolete_id: toml::Value = toml::from_str(&manifest).unwrap();
    obsolete_id
        .as_table_mut()
        .unwrap()
        .insert("id".into(), "obsolete".into());
    let mut nested_id: toml::Value = toml::from_str(&manifest).unwrap();
    nested_id["dependencies"] = toml::from_str::<toml::Value>("[common]\nid = 'common'\nversion = '1.0.0'").unwrap();
    let mut nested_name = nested_id.clone();
    nested_name["dependencies"] =
        toml::from_str::<toml::Value>("[common]\nname = 'common'\nversion = '1.0.0'").unwrap();
    for manifest in [obsolete_id, nested_id, nested_name] {
        let mut contents = contents.clone();
        contents.insert("package.toml".into(), toml::to_string(&manifest).unwrap().into_bytes());
        assert!(Package::load(contents).is_err(), "{manifest}");
    }
}

#[test]
fn entry_point_is_always_root_init_luau() {
    for path in ["main.luau", "nested/init.luau", "Init.luau"] {
        let mut contents = files("test.entry", "1.0.0", "--!strict\nreturn {}", &[], &[]);
        let source = contents.remove("init.luau").unwrap();
        contents.insert(path.into(), source);
        let error = Package::load(contents).err().unwrap().to_string();
        assert!(
            error.contains("missing package entry point: init.luau"),
            "{path}: {error}"
        );
    }
}
