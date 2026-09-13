use super::*;

#[test]
fn bundled_gamemodes_are_separate_selections_without_a_nested_match_type() {
    use tango_script::{ExportKind, PackageRef, PreparedGameMode, Profile};
    let packages = bundled::packages().unwrap();
    let profile = Profile::resolve(
        packages,
        &PackageRef {
            name: "bn6".into(),
            version: "0.1.0".parse().unwrap(),
        },
    )
    .unwrap();
    // Runs the package's cartridge/mode/player/BGM hook behavior checks.
    profile.test().unwrap();
    assert_eq!(profile.selected_export(ExportKind::GameMode), Some("triple"));
    assert!(profile.clone().with_gamemode("bn6").is_err());
    let mut identities = std::collections::BTreeSet::new();
    for name in ["single", "triple", "random"] {
        for code in [b"BR5E", b"BR6E", b"BR5J", b"BR6J"] {
            let mut rom = vec![0; 0xc0];
            rom[0xac..0xb0].copy_from_slice(code);
            let prepared = PreparedGameMode::prepare(
                profile.clone().with_gamemode(name).unwrap(),
                tango_match::gamemode::identity::Runtime {
                    name: "test".into(),
                    revision: 1,
                },
                rom,
                false,
                Default::default(),
            )
            .unwrap();
            let configuration = prepared.configuration().unwrap();
            assert_eq!(configuration.identity.export, name);
            assert!(configuration.options.is_empty());
            assert!(identities.insert(configuration.identity.environment));
            for player in [1, 2] {
                use tango_match::gamemode::Program as _;
                assert_eq!(prepared.program(player, [7; 16]).unwrap().hooks().len(), 17);
            }
        }
    }
}

#[test]
fn host_locale_is_available_even_when_the_package_cannot_load() {
    let mut editor = Editor::open(
        "unused.sav".into(),
        Vec::new(),
        InputOptions {
            locale: Some("ja".into()),
            ..Default::default()
        },
        false,
        false,
    );
    assert!(editor.document.is_none() && editor.error.is_some());
    assert_eq!(editor.lang.to_string(), "ja-JP");
    assert_eq!(editor.title(), "パッケージエディター — Tango");
    editor.closing = true;
    drop(editor.view());
    assert_eq!(
        crate::i18n::negotiate_lang(&"es-MX".parse().unwrap()).to_string(),
        "es-419"
    );
    assert_eq!(
        crate::i18n::negotiate_lang(&"it-IT".parse().unwrap()),
        crate::i18n::FALLBACK_LANG
    );
}

#[test]
fn host_error_translation_keeps_the_underlying_diagnostic() {
    let detail = "save.sav: disk full";
    for locale in crate::i18n::SUPPORTED_LANGS {
        let formatted = crate::i18n::t_args_opt(locale, "package-editor-error", &[("error", detail.into())]).unwrap();
        assert!(formatted.contains(detail), "{locale}: {formatted}");
        assert!(!formatted.contains('⟦'));
    }
    assert_eq!(t!(&"ja-JP".parse().unwrap(), "package-editor-save"), "保存");
}
