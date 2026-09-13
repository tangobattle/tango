use std::collections::BTreeMap;

use tango_script::{Action, Document, Inputs, Package, PackageRef, Profile};

fn load(
    name: &str,
    editor: bool,
    source: &str,
    files: &[(&str, &str)],
    dependency: Option<&str>,
) -> tango_script::Result<Package> {
    let mut manifest = format!("api = 1\nname = '{name}'\nversion = '1.0.0'\n");
    if editor {
        manifest.push_str("[[editor]]\nname = 'main'\npath = './init'\n");
    }
    if let Some(dependency) = dependency {
        manifest.push_str(&format!("[dependencies]\n'{dependency}' = '1.0.0'\n"));
    }
    let mut tree = BTreeMap::from([
        ("package.toml".into(), manifest.into_bytes()),
        ("init.luau".into(), format!("--!strict\n{source}").into_bytes()),
    ]);
    tree.extend(
        files
            .iter()
            .map(|(path, text)| (path.to_string(), text.as_bytes().to_vec())),
    );
    Package::load(tree)
}

fn resolve(packages: &[Package], selected: &str) -> Profile {
    Profile::resolve(
        packages,
        &PackageRef {
            name: selected.into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
}

fn library(source: &str, files: &[(&str, &str)]) -> Profile {
    let package = load(
        "test.messages",
        false,
        &format!("local t = tango.load_catalog(\"./locales\")\nreturn {{ test = function() {source} end }}"),
        files,
        None,
    )
    .unwrap();
    resolve(&[package], "test.messages")
}

#[test]
fn retained_failures_retranslate_catalog_chains_without_a_live_profile_or_vm() {
    let shared = load(
        "test.shared",
        false,
        "return {t = tango.load_catalog('./text')}",
        &[
            ("text/en-US/main.ftl", "failure = Shared { $n } { $name }\n"),
            ("text/ja-JP/main.ftl", "failure = 共通 { $n } { $name }\n"),
        ],
        None,
    )
    .unwrap();
    let app = load(
        "test.app",
        false,
        r#"
local t = tango.load_catalog("./locales", require("@test.shared").t)
return {test = function()
    local args: TranslationArgs = {n = 3, name = "Zoë"}
    tango.fail(t, "failure", args)
end}
"#,
        &[
            ("locales/en-US/main.ftl", "other = Other\n"),
            ("locales/ja-JP/main.ftl", "failure = ローカル { $n } { $name }\n"),
            ("locales/de-DE/main.ftl", "failure = { $missing }\n"),
        ],
        Some("test.shared"),
    )
    .unwrap();
    let profile = resolve(&[shared, app], "test.app");
    let error = profile.test().unwrap_err();
    // Reusing the VM must neither mutate the old error nor retain its upvalues.
    let japanese = profile
        .clone()
        .with_locale("ja-JP")
        .unwrap()
        .test()
        .unwrap_err()
        .to_string();
    assert!(japanese.contains("ローカル 3 Zoë"), "{japanese}");
    drop(profile);
    let original = error.to_string();
    assert!(original.contains("Shared 3 Zoë"), "{original}");
    assert!(original.contains("stack traceback"), "{original}");
    for (locale, expected) in [
        ("ja-JP", "ローカル 3 Zoë"),
        ("en-US", "Shared 3 Zoë"),
        ("fr", "Shared 3 Zoë"),
        ("de-DE", "Shared 3 Zoë"),
    ] {
        let rendered = error.localized(&locale.parse().unwrap());
        assert!(rendered.contains(expected), "{rendered}");
        assert!(rendered.contains("stack traceback"));
    }
    assert_eq!(error.to_string(), original);
}

#[test]
fn retained_failures_reject_dynamic_translators_and_unbounded_arguments() {
    let files = [("locales/en-US/main.ftl", "failure = Failure { $n }\n")];
    for source in [
        "tango.fail(function(_key: string, _args: TranslationArgs?): string return 'custom' end, 'failure')",
        "tango.fail(tango.load_catalog('./locales', function(_key: string, _args: TranslationArgs?): string return 'custom' end), 'failure')",
        "tango.fail(t, 'failure', {n = 0/0})",
        "tango.fail(t, 'failure', {n = string.rep('x', 8193)})",
        "tango.fail(t, string.rep('x', 257), {n = 1})",
    ] {
        let error = library(source, &files).test().unwrap_err();
        assert!(!error.to_string().contains("Failure"), "{error}");
        assert_eq!(error.localized(&"ja-JP".parse().unwrap()), error.to_string());
    }
    let error = library("tango.fail(t, 'missing')", &files).test().unwrap_err();
    assert!(error.localized(&"ja-JP".parse().unwrap()).contains("⟦missing⟧"));
}

#[test]
fn fluent_fallback_interpolation_attributes_terms_plurals_and_selectors() {
    let files = [
        ("locales/en-US/main.ftl", "fallback = English\nattribute = English label\n    .tooltip = English tooltip\n"),
        ("locales/fr-FR/main.ftl", "-product = Tango\nhello = Bonjour { $name }, { -product }\nattribute = Libellé\ncount = { $n ->\n    [one] Un fichier\n   *[other] { $n } fichiers\n}\nmode = { $mode ->\n    [practice] Entraînement\n   *[other] Partie\n}\nnumber = { NUMBER($n, minimumFractionDigits: 2) }\n"),
        ("locales/fr-FR/other.ftl", "other-file = Deuxième fichier\n"),
    ];
    let profile = library(
        r#"
assert(tango.locale() == "fr-CA")
assert(tango.text_direction() == "ltr")
assert(t("hello", {name = "Zoë"}) == "Bonjour Zoë, Tango")
assert(t("count", {n = 1}) == "Un fichier")
assert(t("count", {n = 3}) == "3 fichiers")
assert(t("mode", {mode = "practice"}) == "Entraînement")
assert(t("number", {n = 1.5}) == "1.50")
assert(t("fallback", {}) == "English")
assert(t("attribute") == "Libellé")
assert(t("attribute.tooltip") == "English tooltip")
assert(t("other-file") == "Deuxième fichier")
assert(t("absent") == "⟦absent⟧")
"#,
        &files,
    )
    .with_locale("fr-CA")
    .unwrap();
    profile.test().unwrap();
    library(
        "assert(t('fallback') == 'English'); assert(tango.text_direction() == 'rtl')",
        &files,
    )
    .with_locale("ar")
    .unwrap()
    .test()
    .unwrap();
    assert!(library("t('absent')", &[]).test().is_err());
    let plural = "count = { $n ->\n    [one] один\n    [few] несколько\n   *[other] много\n}\n";
    library("assert(t('count', {n = 1}) == 'один'); assert(t('count', {n = 3}) == 'несколько'); assert(t('count', {n = 1.5}) == 'много')", &[("locales/ru-RU/main.ftl", plural)])
        .with_locale("ru").unwrap().test().unwrap();
}

#[test]
fn dependency_translations_are_scoped_and_archived_without_manifest_declarations() {
    let shared = load(
        "test.shared",
        false,
        "return {t = tango.load_catalog(\"./locales\")}",
        &[("locales/ja-JP/main.ftl", "same = ライブラリ\nprivate = 非公開\n")],
        None,
    )
    .unwrap();
    let app = load(
        "test.app",
        false,
        r#"
local t = tango.load_catalog("./locales")
local shared_t = require("@test.shared").t
return {test = function()
    assert(t("same") == "アプリ")
    assert(shared_t("same") == "ライブラリ")
    assert(t("private") == "⟦private⟧")
    assert(t("../test.shared/private") == "⟦../test.shared/private⟧")
end}
"#,
        &[("locales/ja-JP/main.ftl", "same = アプリ\npackage-name = アプリ名\n")],
        Some("test.shared"),
    )
    .unwrap();
    let bytes = app.to_archive().unwrap();
    let reopened = Package::from_archive(&bytes).unwrap();
    assert_eq!(app.digest(), reopened.digest());
    let profile = resolve(&[shared, reopened], "test.app").with_locale("ja-JP").unwrap();
    assert_eq!(profile.display_name(), "アプリ名");
    profile.test().unwrap();
}

#[test]
fn explicit_catalogs_are_independent_module_relative_and_can_be_passed_to_other_packages() {
    let shared = load(
        "test.shared",
        false,
        "return {t = require(\"./translation\")}",
        &[
            ("text/en-US/main.ftl", "same = Shared\nshared-only = Shared only\n"),
            ("translation.luau", "--!strict\nreturn tango.load_catalog('./text')"),
            (
                "invoke.luau",
                "--!strict\nreturn function(t: Translate, key: string): string return t(key) end",
            ),
        ],
        None,
    )
    .unwrap();
    let app = load("test.app", false, r#"
local t = tango.load_catalog("./translations")
local shared_t = require("@test.shared").t
local child = require("./ui/panel")
local invoke = require("@test.shared/invoke")
return {test = function()
    assert(t("same") == "App")
    assert(shared_t("same") == "Shared")
    assert(child.parent("same") == "App")
    assert(child.own("same") == "Panel")
    assert(invoke(t, "same") == "App")
    assert(invoke(shared_t, "same") == "Shared")
    assert(child.own("shared-only") == "⟦shared-only⟧")
    assert(t("shared-only") == "⟦shared-only⟧")
end}
"#, &[
        ("translations/en-US/main.ftl", "same = App\n"),
        ("ui/labels/en-US/main.ftl", "same = Panel\n"),
        ("ui/panel.luau", "--!strict\nreturn {parent = tango.load_catalog('../translations'), own = tango.load_catalog('./labels')}"),
    ], Some("test.shared")).unwrap();
    resolve(&[shared, app], "test.app").test().unwrap();
}

#[test]
fn imported_translators_chain_with_local_overrides_and_locale_fallback() {
    let leaf = load(
        "test.leaf",
        false,
        "return {t = tango.load_catalog('./locales')}",
        &[(
            "locales/ja-JP/main.ftl",
            "leaf = こんにちは { $name }\ncount = { $n } 件\n",
        )],
        None,
    )
    .unwrap();
    let shared = load(
        "test.shared",
        false,
        "return {t = tango.load_catalog('./text', require('@test.leaf').t)}",
        &[("text/ja-JP/main.ftl", "same = 共有\nshared = 共通\nown-default = 共有の既定\nlabel = 共有ラベル\n    .tooltip = 共有の説明\nliteral-marker = Wrong\n")],
        Some("test.leaf"),
    ).unwrap();
    let app = load(
        "test.app",
        false,
        r#"
local t = tango.load_catalog("./locales", require("@test.shared").t)
return {test = function()
    assert(t("same") == "ローカル")
    assert(t("shared") == "共通")
    assert(t("leaf", {name = "Zoë"}) == "こんにちは Zoë")
    assert(t("count", {n = 3}) == "3 件")
    assert(t("own-default") == "App English")
    assert(t("label") == "ラベル")
    assert(t("label.tooltip") == "共有の説明")
    assert(t("literal-marker") == "⟦literal-marker⟧")
    assert(t("missing") == "⟦missing⟧")
    assert(require("@test.shared").t("same") == "共有")
end}
"#,
        &[
            (
                "locales/ja-JP/main.ftl",
                "same = ローカル\nlabel = ラベル\nliteral-marker = ⟦literal-marker⟧\n",
            ),
            ("locales/en-US/main.ftl", "own-default = App English\n"),
        ],
        Some("test.shared"),
    )
    .unwrap();
    resolve(&[leaf, shared, app], "test.app")
        .with_locale("ja-JP")
        .unwrap()
        .test()
        .unwrap();
}

#[test]
fn fallback_functions_receive_original_arguments_and_do_not_hide_format_errors() {
    let files = [("locales/en-US/main.ftl", "known = Local\nbroken = { $missing }\n")];
    library(
        r#"
local chained = tango.load_catalog("./locales", function(key: string, args: TranslationArgs?): string
    assert(key ~= "known")
    if key == "bare" then assert(args == nil); return "Bare" end
    assert(args and args.name == "Zoë" and args.n == 3)
    return key .. " fallback"
end)
assert(chained("known") == "Local")
assert(chained("bare") == "Bare")
assert(chained("custom", {name = "Zoë", n = 3}) == "custom fallback")
"#,
        &files,
    )
    .test()
    .unwrap();
    let error = library(
        r#"
local chained = tango.load_catalog("./locales", function(_key: string, _args: TranslationArgs?): string
    error("fallback must not run")
end)
chained("broken")
"#,
        &files,
    )
    .test()
    .unwrap_err();
    assert!(
        error.to_string().contains("cannot format translation broken"),
        "{error}"
    );
}

#[test]
fn fallback_functions_enforce_return_types_output_limits_and_recursion_bounds() {
    let files = [("locales/en-US/main.ftl", "known = Local\n")];
    library(
        r#"
local chain: Translate = function(_key: string, _args: TranslationArgs?): string return "End" end
for _ = 1, 32 do chain = tango.load_catalog("./locales", chain) end
for _ = 1, 64 do assert(chain("missing") == "End") end
"#,
        &files,
    )
    .test()
    .unwrap();
    for (body, expected) in [
        ("return 42", "translation fallback must return a string"),
        ("return string.rep('x', 16385)", "translated text exceeds 16 KiB"),
    ] {
        let source = format!("local chained = tango.load_catalog('./locales', function(_key: string, _args: TranslationArgs?): any {body} end); chained('missing')");
        let error = library(&source, &files).test().unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let error = library(
        r#"
local recursive: Translate
recursive = tango.load_catalog("./locales", function(key: string, args: TranslationArgs?): string
    return recursive(key, args)
end)
recursive("missing")
"#,
        &files,
    )
    .test()
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("translation fallback chain exceeds 32 calls"),
        "{error}"
    );
    for source in [
        "return {t = tango.load_catalog('./locales', 42)}",
        "return {t = tango.load_catalog('./locales', function(_key: string): number return 42 end)}",
    ] {
        assert!(load("test.types", false, source, &files, None).is_err(), "{source}");
    }
}

#[test]
fn catalog_paths_cannot_escape_or_reach_undeclared_transitive_dependencies() {
    let leaf = load(
        "test.leaf",
        false,
        "return {t = tango.load_catalog(\"./locales\")}",
        &[("locales/en-US/main.ftl", "key = Leaf\n")],
        None,
    )
    .unwrap();
    let shared = load(
        "test.shared",
        false,
        "return {t = tango.load_catalog(\"./locales\"), leaf_t = require(\"@test.leaf\").t}",
        &[("locales/en-US/main.ftl", "key = Shared\n")],
        Some("test.leaf"),
    )
    .unwrap();
    for request in [
        "../locales",
        "./../test.shared/locales",
        "/locales",
        "C:/locales",
        "./locales\\en-US",
        "test.shared/locales",
        "@test.shared/locales",
        "@test.shared/../locales",
        "test.shared/../locales",
        "test.shared/./locales",
        "test.shared/locales/../../test.leaf/locales",
        "test.leaf/locales",
        "test.unknown/locales",
        "./missing",
        "locales",
        "",
        "./locales//en-US",
    ] {
        let source = format!("return {{test = function() local t = tango.load_catalog({request:?}); t('key') end}}");
        let app = load(
            "test.app",
            false,
            &source,
            &[("locales/en-US/main.ftl", "key = App\n")],
            Some("test.shared"),
        )
        .unwrap();
        let error = resolve(&[leaf.clone(), shared.clone(), app], "test.app")
            .test()
            .unwrap_err();
        assert!(!error.to_string().contains("type check"), "{request}: {error}");
    }
    let app = load(
        "test.app",
        false,
        r#"
return {test = function()
    local shared = require("@test.shared")
    assert(shared.t("key") == "Shared")
    assert(shared.leaf_t("key") == "Leaf")
end}
"#,
        &[],
        Some("test.shared"),
    )
    .unwrap();
    resolve(&[leaf, shared, app], "test.app").test().unwrap();
}

#[test]
fn catalog_loader_is_typed_and_rejects_invalid_custom_catalogs() {
    for source in [
        "return {test = function() tango.load_catalog(42) end}",
        "local t = tango.load_catalog('./locales'); return {test = function() local args: TranslationArgs = {n = true}; t('key', args) end}",
        "return {test = function() tango.t('key') end}",
    ] {
        assert!(load("test.types", false, source, &[], None).is_err(), "{source}");
    }
    // Upstream's new solver (also used by stock luau-lsp) currently accepts
    // inferred record literals against this dictionary argument. The host
    // must reject them even when static inference misses the invalid value.
    let source = "local t = tango.load_catalog('./locales'); return {test = function() t('key', {n = true}) end}";
    if let Ok(package) = load(
        "test.types",
        false,
        source,
        &[("locales/en-US/main.ftl", "key = Value { $n }\n")],
        None,
    ) {
        let error = resolve(&[package], "test.types").test().unwrap_err();
        assert!(error.to_string().contains("untagged enum Argument"), "{error}");
    }
    for files in [
        vec![("text/en-us/main.ftl", "key = Text\n")],
        vec![
            ("text/en-US/main.ftl", "key = One\n"),
            ("text/en-US/extra.ftl", "key = Two\n"),
        ],
        vec![("text/en-US.ftl", "key = Text\n")],
    ] {
        let package = load(
            "test.catalog",
            false,
            "return {test = function() tango.load_catalog('./text') end}",
            &files,
            None,
        )
        .unwrap();
        assert!(resolve(&[package], "test.catalog").test().is_err(), "{files:?}");
    }
}

#[test]
fn catalog_cache_eviction_preserves_bound_functions_and_independent_operations() {
    let paths: Vec<_> = (0..66).map(|i| format!("text-{i}/en-US/main.ftl")).collect();
    let values: Vec<_> = (0..66).map(|i| format!("key = Catalog {i}\n")).collect();
    let files: Vec<_> = paths
        .iter()
        .zip(&values)
        .map(|(p, v)| (p.as_str(), v.as_str()))
        .collect();
    let package = load(
        "test.cache",
        false,
        r#"
return {test = function()
    local first = tango.load_catalog("./text-0")
    for i = 1, 65 do
        local t = tango.load_catalog("./text-" .. tostring(i))
        assert(t("key") == "Catalog " .. tostring(i))
    end
    assert(first("key") == "Catalog 0")
    local reload = tango.load_catalog("./text-0")
    assert(reload("key") == first("key"))
end}
"#,
        &files,
        None,
    )
    .unwrap();
    let profile = resolve(&[package], "test.cache");
    profile.test().unwrap();
    profile.test().unwrap();
}

#[test]
fn locale_changes_preserve_drafts_history_inputs_and_saved_state_transactionally() {
    let app = load(
        "test.editor",
        true,
        r#"
local t = tango.load_catalog("./locales")
return {
        decode = function(b: buffer): buffer return b end,
        encode = function(b: buffer): buffer return b end,
        validate = function(b: buffer): {string}
            if buffer.readu8(b, 0) == 255 then return {t("invalid")} end
            return {}
        end,
        view = function(b: buffer, s: ViewState): Node
            assert(tango.input_size("rom") == 1)
            local children: {Node} = {
                {kind = "input", id = "draft", text = t("value"), value = s.draft or tostring(buffer.readu8(b, 0))},
                {kind = "button", id = "apply", text = t("apply")},
            }
            return {kind = "column", children = children}
        end,
        update = function(b: buffer, s: ViewState, a: Action)
            if a.kind == "change" then s.draft = a.value
            elseif a.kind == "activate" then buffer.writeu8(b, 0, tonumber(s.draft or "0") or 0) end
            return nil
        end,
    }
"#,
        &[
            (
                "locales/en-US/main.ftl",
                "value = Value\napply = Apply\ninvalid = Invalid value\n",
            ),
            (
                "locales/ja-JP/main.ftl",
                "package-name = エディター\nvalue = 値\napply = 適用\ninvalid = 無効な値\n",
            ),
            ("locales/de-DE/main.ftl", "value = { $missing }\n"),
        ],
        None,
    )
    .unwrap();
    let profile = resolve(&[app], "test.editor").with_inputs(Inputs::new([("rom".into(), vec![7])].into()).unwrap());
    let original_digest = profile.digest();
    assert_eq!(profile.clone().with_locale("EN-us").unwrap().digest(), original_digest);
    let mut doc = Document::open(profile, &[1]).unwrap();
    doc.dispatch(Action::change("draft", "42")).unwrap();
    doc.set_locale("ja-JP").unwrap();
    assert!(!doc.is_dirty());
    assert!(!doc.can_undo());
    assert_eq!(doc.title(), "エディター");
    let view = serde_json::to_value(doc.view()).unwrap();
    assert_eq!(view["children"][0]["text"], "値");
    assert_eq!(view["children"][0]["value"], "42");
    assert_ne!(doc.profile_digest(), original_digest);
    let digest = doc.profile_digest();
    assert!(doc.set_locale("de-DE").is_err());
    assert_eq!(doc.locale().to_string(), "ja-JP");
    assert_eq!(doc.profile_digest(), digest);
    assert_eq!(serde_json::to_value(doc.view()).unwrap(), view);
    doc.dispatch(Action::activate("apply", "")).unwrap();
    assert_eq!(doc.encode().unwrap(), [42]);
    assert!(doc.is_dirty());
    doc.set_locale("en-US").unwrap();
    assert_eq!(doc.profile_digest(), original_digest);
    doc.undo().unwrap();
    assert_eq!(doc.encode().unwrap(), [1]);
    doc.set_locale("ja-JP").unwrap();
    assert!(doc.can_redo());
    doc.redo().unwrap();
    assert_eq!(doc.encode().unwrap(), [42]);
    doc.mark_saved();
    doc.set_locale("en-US").unwrap();
    assert!(!doc.is_dirty());
    doc.dispatch(Action::change("draft", "255")).unwrap();
    doc.dispatch(Action::activate("apply", "")).unwrap();
    assert_eq!(doc.diagnostics(), ["Invalid value"]);
    doc.set_locale("ja-JP").unwrap();
    assert_eq!(doc.diagnostics(), ["無効な値"]);
}

#[test]
fn catalogs_reject_malformed_duplicates_paths_and_unbounded_numbers() {
    for files in [
        vec![("locales/en-US/main.ftl", "broken = {\n")],
        vec![("locales/en-US/main.ftl", "same = First\nsame = Second\n")],
        vec![
            ("locales/en-US/main.ftl", "same = First\n"),
            ("locales/en-US/extra.ftl", "same = Second\n"),
        ],
        vec![(
            "locales/en-US/main.ftl",
            "a = Value\n    .title = First\n    .title = Second\n",
        )],
        vec![("locales/invalid_locale/main.ftl", "x = Text\n")],
        vec![("locales/en-us/main.ftl", "x = Text\n")],
        vec![("locales/en-US.ftl", "x = Text\n")],
        vec![(
            "locales/en-US/main.ftl",
            "x = { 999999999999999999999999999999999999 }\n",
        )],
        vec![("locales/en-US/main.ftl", "x = { 0.00000000000000000000001 }\n")],
        vec![(
            "locales/en-US/main.ftl",
            "x = { NUMBER(1, minimumFractionDigits: 999999999) }\n",
        )],
    ] {
        assert!(
            load("test.invalid", false, "return {}", &files, None).is_err(),
            "{files:?}"
        );
    }
    let profile = library("", &[]);
    for locale in ["", "../en-US", "en_US", "en-US/other", "not_a_locale"] {
        assert!(profile.clone().with_locale(locale).is_err(), "{locale}");
    }
}

#[test]
fn translation_calls_enforce_types_quotas_and_report_format_errors() {
    for (call, text) in [
        ("t('value')", "value = { $missing }\n"),
        ("t('value')", "value = { value }\n"),
        ("t('value', {n = 0/0})", "value = { $n }\n"),
        ("t('value', {n = 1e100})", "value = { $n }\n"),
        ("t('value', {n = true} :: any)", "value = { $n }\n"),
        ("t('value', {n = {}} :: any)", "value = { $n }\n"),
        ("t('value', setmetatable({}, {}) :: any)", "value = Text\n"),
        ("t(string.rep('x', 257))", "value = Text\n"),
        ("t('value', {n = string.rep('x', 8193)})", "value = { $n }\n"),
        ("for i = 1, 8193 do t('value') end", "value = Text\n"),
    ] {
        assert!(
            library(call, &[("locales/en-US/main.ftl", text)]).test().is_err(),
            "{call}: {text}"
        );
    }
    let long = format!("value = {}\n", "x".repeat(16 * 1024 + 1));
    assert!(library("t('value')", &[("locales/en-US/main.ftl", &long)])
        .test()
        .is_err());
    let oversized = "#".repeat(256 * 1024 + 1);
    assert!(load(
        "test.large",
        false,
        "return {}",
        &[("locales/en-US/main.ftl", &oversized)],
        None
    )
    .is_err());
}
