use std::collections::BTreeMap;

use sha3::{Digest, Sha3_256};
use tango_match::gamemode::{
    identity::{Identity, Runtime},
    Error, Memory, Program, Reader, Value, Write,
};
use tango_script::{Inputs, Package, PackageRef, PreparedGameMode, Profile};

const MODE: &str = r#"--!strict
local common = require("@common")
assert(tango.locale() == "en-US")
assert(tango.input_size("editor_secret") == nil)
return {
    patch_rom = function(rom: buffer): buffer
        assert(buffer.readu8(rom, 0) == buffer.readu8(tango.read_input("rom", 0, 1), 0))
        buffer.writeu8(rom, 0, buffer.readu8(rom, 0) + common.value)
        return rom
    end,
    setup = function(context: GameModeContext, game: Game)
        local delta = context.options.delta
        assert(type(delta) == "number")
        local value = delta + context.player + context.seed[1] + 1
        game.hook(256, function()
            local bytes = buffer.create(1)
            buffer.writeu8(bytes, 0, value)
            game.write("main", 0, bytes)
            game.write("main", 1, tango.read_input("rom", 0, 1))
        end)
    end,
}
"#;

const EDITOR: &str = r#"--!strict
error("a gamemode must not load the editor")
return {} :: Editor
"#;

fn profile(mode: &str, asset: u8) -> Profile {
    let common = Package::load(BTreeMap::from([
        ("package.toml".into(), b"api=1\nname='common'\nversion='1.0.0'".to_vec()),
        ("init.luau".into(), b"--!strict\nreturn {value = 1}".to_vec()),
        ("unused.bin".into(), vec![asset]),
    ]))
    .unwrap();
    let package = Package::load(BTreeMap::from([
        ("package.toml".into(), b"api=1\nname='game'\nversion='1.0.0'\ndefault_gamemode='main'\n[dependencies]\ncommon='1.0.0'\n[[gamemode]]\nname='main'\npath='./mode'\n[[gamemode]]\nname='other'\npath='./mode'\n[[editor]]\nname='one'\npath='./editor'\n[[editor]]\nname='two'\npath='./editor'\n".to_vec()),
        ("init.luau".into(), b"--!strict\nreturn {}".to_vec()),
        ("mode.luau".into(), mode.as_bytes().to_vec()),
        ("editor.luau".into(), EDITOR.as_bytes().to_vec()),
    ])).unwrap();
    Profile::resolve(
        &[package, common],
        &PackageRef {
            name: "game".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
    .with_gamemode("main")
    .unwrap()
}

fn engine() -> Runtime {
    Runtime {
        name: "test-engine".into(),
        revision: 1,
    }
}

fn options() -> BTreeMap<String, Value> {
    BTreeMap::from([("delta".into(), Value::Number(3.0))])
}

fn prepare(profile: Profile) -> PreparedGameMode {
    PreparedGameMode::prepare(profile, engine(), vec![7], false, options()).unwrap()
}

struct NoMemory;
impl Memory for NoMemory {
    fn read(&mut self, _: &str, _: u32, _: &mut [u8]) -> Result<(), Error> {
        Err("no memory".into())
    }
}
impl Reader for NoMemory {
    fn read_register(&mut self, _: &str) -> Result<u32, Error> {
        Err("no registers".into())
    }
}

fn observe(program: &mut dyn Program) -> (u8, u8) {
    let update = program
        .on_hook(&program.hooks()[0].name.clone(), &mut NoMemory)
        .unwrap();
    let [Write::Memory { data: state, .. }, Write::Memory { data: rom, .. }] = &update.writes[..] else {
        panic!("expected two writes")
    };
    (state[0], rom[0])
}

#[test]
fn preparation_freezes_patch_output_and_options_without_ui_state() {
    let profile = profile(MODE, 0);
    let one = prepare(profile.clone().with_editor("one").unwrap());
    let two = prepare(
        profile
            .with_editor("two")
            .unwrap()
            .with_locale("ja-JP")
            .unwrap()
            .with_inputs(Inputs::new(BTreeMap::from([("editor_secret".into(), vec![99])])).unwrap()),
    );
    assert_eq!(one.identity(), two.identity());
    assert_eq!(one.rom(), [8]);
    assert_eq!(one.identity().rom, <[u8; 32]>::from(Sha3_256::digest([8])));
    let mut a = one.program(1, [2; 16]).unwrap();
    let mut b = two.program(1, [2; 16]).unwrap();
    assert_eq!(a.source(), b.source());
    assert_eq!(observe(&mut a), (7, 8));
    assert_eq!(observe(&mut a), (7, 8));
    // Reopening recreates constants and never runs the patch again.
    assert_eq!(observe(&mut b), (7, 8));
    assert_eq!(observe(&mut one.program(1, [2; 16]).unwrap()), (7, 8));
    assert_ne!(one.program(2, [2; 16]).unwrap().source(), a.source());
    assert_ne!(one.program(1, [3; 16]).unwrap().source(), a.source());
    assert!(one.program(0, [2; 16]).is_err());
    assert!(one.program(3, [2; 16]).is_err());
}

#[test]
fn pins_cover_all_files_options_runtime_and_export_and_verify_each_seat() {
    let original = profile(MODE, 0);
    let one = prepare(original.clone());
    let mut changed_engine = engine();
    changed_engine.revision += 1;
    let different_engine =
        PreparedGameMode::prepare(original.clone(), changed_engine, vec![7], false, options()).unwrap();
    let different_rom = PreparedGameMode::prepare(original.clone(), engine(), vec![6], false, options()).unwrap();
    let different_bgm = PreparedGameMode::prepare(original.clone(), engine(), vec![7], true, options()).unwrap();
    let frozen = different_bgm.configuration().unwrap();
    assert!(frozen.disable_bgm && !frozen.options.contains_key("disable_bgm"));
    let reproduced = PreparedGameMode::prepare(
        original.clone(),
        engine(),
        vec![7],
        frozen.disable_bgm,
        frozen.options(),
    )
    .unwrap();
    assert_eq!(reproduced.identity(), different_bgm.identity());
    for changed in [
        prepare(profile(MODE, 1)), // Even files not read in this execution are pinned.
        prepare(profile(&format!("{MODE}\n-- updated source\n"), 0)),
        prepare(original.clone().with_gamemode("other").unwrap()),
        PreparedGameMode::prepare(
            original,
            engine(),
            vec![7],
            false,
            BTreeMap::from([("delta".into(), Value::Number(4.0))]),
        )
        .unwrap(),
        different_engine.clone(),
        different_rom.clone(),
        different_bgm,
    ] {
        assert!(changed.verify(one.identity()).is_err());
        assert_ne!(
            changed.program(1, [2; 16]).unwrap().source(),
            one.program(1, [2; 16]).unwrap().source()
        );
    }
    // Peers reproduce each seat; different cartridges need not share a digest.
    one.verify(one.identity()).unwrap();
    different_rom.verify(different_rom.identity()).unwrap();
    assert_ne!(one.identity().rom, different_rom.identity().rom);
    let encoded = serde_json::to_vec(one.identity()).unwrap();
    let decoded: Identity = serde_json::from_slice(&encoded).unwrap();
    one.verify(&decoded).unwrap();
    let configuration = one.configuration().unwrap();
    let decoded_configuration = tango_match::gamemode::Configuration::decode(&configuration.encode().unwrap()).unwrap();
    let reopened = PreparedGameMode::prepare(
        profile(MODE, 0),
        engine(),
        vec![7],
        false,
        decoded_configuration.options(),
    )
    .unwrap();
    reopened.verify(&decoded_configuration.identity).unwrap();
    assert_eq!(
        reopened.program(1, [2; 16]).unwrap().source(),
        one.program(1, [2; 16]).unwrap().source()
    );
    let mut unsupported_script = decoded;
    unsupported_script.script.revision += 1;
    assert!(one.verify(&unsupported_script).is_err());
}

#[test]
fn malformed_claims_and_options_do_not_enter_a_simulation() {
    let profile = profile(MODE, 0);
    let prepared = prepare(profile.clone());
    for options in [
        BTreeMap::from([("delta".into(), Value::Number(f64::NAN))]),
        BTreeMap::from([("delta".into(), Value::Number(f64::INFINITY))]),
        BTreeMap::from([("".into(), Value::Boolean(true))]),
        BTreeMap::from([("delta".into(), Value::String("x".repeat(4097)))]),
        (0..129).map(|n| (n.to_string(), Value::Boolean(true))).collect(),
    ] {
        assert!(PreparedGameMode::prepare(profile.clone(), engine(), vec![7], false, options).is_err());
    }
    // A missing option remains a script validation error at setup.
    let missing = PreparedGameMode::prepare(profile.clone(), engine(), vec![7], false, BTreeMap::new()).unwrap();
    assert!(missing.program(1, [2; 16]).is_err());
    for mutate in [
        |identity: &mut Identity| identity.export = "../mode".into(),
        |identity: &mut Identity| identity.engine.name = "".into(),
        |identity: &mut Identity| identity.package.name = ".".into(),
        |identity: &mut Identity| identity.package.version = "".into(),
        |identity: &mut Identity| identity.dependencies.push(identity.dependencies[0].clone()),
        |identity: &mut Identity| identity.dependencies.push(identity.package.clone()),
        |identity: &mut Identity| identity.dependencies = vec![identity.dependencies[0].clone(); 128],
    ] {
        let mut identity = prepared.identity().clone();
        mutate(&mut identity);
        assert!(identity.validate().is_err(), "{identity:?}");
        assert!(prepared.verify(&identity).is_err());
    }
}

#[test]
fn option_hashing_preserves_types_signed_zero_and_string_boundaries() {
    let profile = profile(MODE, 0);
    let variants = [
        BTreeMap::from([("x".into(), Value::Boolean(true))]),
        BTreeMap::from([("x".into(), Value::String("true".into()))]),
        BTreeMap::from([("x".into(), Value::Number(1.0))]),
        BTreeMap::from([("x".into(), Value::Number(0.0))]),
        BTreeMap::from([("x".into(), Value::Number(-0.0))]),
        BTreeMap::from([("x".into(), Value::String("yz".into()))]),
        BTreeMap::from([("xy".into(), Value::String("z".into()))]),
    ];
    let digests: std::collections::BTreeSet<_> = variants
        .iter()
        .map(|options| {
            PreparedGameMode::prepare(profile.clone(), engine(), vec![7], false, options.clone())
                .unwrap()
                .identity()
                .environment
        })
        .collect();
    assert_eq!(digests.len(), variants.len());
}

#[test]
fn legacy_replay_import_is_explicit_typed_and_confined_to_the_gamemode() {
    let game = tango_replay::metadata::GameInfo {
        rom_family: "custom".into(),
        rom_variant: 7,
        sim_version: 19,
        patch: None,
    };
    let engine = engine();
    let legacy = tango_script::LegacyReplay {
        game: &game,
        match_type: 3,
        match_subtype: 4,
        engine: &engine,
    };
    assert!(profile(MODE, 0).import_replay(&legacy, &[7]).unwrap().is_none());
    let source = MODE.replacen(
        "return {",
        r#"return {
        import_replay = function(old: LegacyReplay, rom: buffer): ReplayImport?
            assert(tango.locale() == 'en-US' and tango.input_size('editor_secret') == nil)
            assert(old.game.patch == nil)
            assert(old.game.rom_family == 'custom' and old.game.rom_variant == 7 and old.game.sim_version == 19)
            assert(old.engine.name == 'test-engine' and old.engine.revision == 1)
            assert(buffer.readu8(rom, 0) == 7 and buffer.readu8(tango.read_input('rom', 0, 1), 0) == 7)
            buffer.writeu8(rom, 0, 99) -- Cannot change preparation's source.
            return {disable_bgm = true, options = {delta = old.match_type + old.match_subtype}}
        end,
"#,
        1,
    );
    let profile = profile(&source, 0)
        .with_locale("ja-JP")
        .unwrap()
        .with_inputs(Inputs::new([("editor_secret".into(), vec![9])].into()).unwrap());
    let rom = vec![7];
    let imported = profile.import_replay(&legacy, &rom).unwrap().unwrap();
    assert!(imported.disable_bgm);
    assert_eq!(imported.options["delta"], Value::Number(7.0));
    assert_eq!(rom, [7]);
    let prepared = PreparedGameMode::prepare(profile, engine, rom, imported.disable_bgm, imported.options).unwrap();
    assert_eq!(prepared.rom(), [8]);
    assert!(prepared.configuration().unwrap().disable_bgm);
}

#[test]
fn legacy_import_outputs_cannot_bypass_option_or_table_limits() {
    let game = tango_replay::metadata::GameInfo::default();
    let engine = engine();
    let legacy = tango_script::LegacyReplay {
        game: &game,
        match_type: 0,
        match_subtype: 0,
        engine: &engine,
    };
    for output in [
        "{extra = true}",
        "{disable_bgm = 'yes'}",
        "{options = {disable_bgm = true}}",
        "{options = {[''] = 1}}",
        "{options = {value = 0/0}}",
        "{options = {value = {nested = 1}}}",
        "setmetatable({}, {})",
        "{options = {value = string.rep('x', 4097)}}",
    ] {
        let source = MODE.replacen(
            "return {",
            &format!("return {{import_replay = function(): ReplayImport? return ({output} :: any) end,"),
            1,
        );
        assert!(profile(&source, 0).import_replay(&legacy, &[7]).is_err(), "{output}");
    }
}
