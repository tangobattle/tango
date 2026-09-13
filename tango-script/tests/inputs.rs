use tango_script::{Action, Document, Inputs, Package, PackageRef, Profile};

fn profile(editor: bool, source: &str) -> Profile {
    let export = if editor {
        "\n[[editor]]\nname = 'main'\npath = './init'\n"
    } else {
        ""
    };
    let package = Package::load(
        [
            (
                "package.toml".into(),
                format!("api = 1\nname = 'test.inputs'\nversion = '1.0.0'{export}").into_bytes(),
            ),
            ("init.luau".into(), format!("--!strict\n{source}").into_bytes()),
        ]
        .into(),
    )
    .unwrap();
    Profile::resolve(
        &[package],
        &PackageRef {
            name: "test.inputs".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
}

fn inputs(bytes: &[u8]) -> Inputs {
    Inputs::new([("rom".into(), bytes.to_vec())].into()).unwrap()
}

#[test]
fn native_image_encoders_enforce_types_dimensions_and_shared_work_limits() {
    for expression in [
        "tango.encode_png({width = 1, height = 1, rgba = buffer.create(3)})",
        "tango.encode_png({width = 0, height = 1, rgba = buffer.create(0)})",
        "tango.encode_png({width = -1, height = 1, rgba = buffer.create(4)})",
        "tango.encode_png({width = 1.5, height = 1, rgba = buffer.create(4)})",
        "tango.encode_png({width = 1, height = 0/0, rgba = buffer.create(4)})",
        "tango.encode_png({width = 4097, height = 1, rgba = buffer.create(16388)})",
        "tango.encode_png({width = 1, height = 1, rgba = 'rgba'} :: any)",
        "tango.encode_png({width = 1, height = 1, rgba = string.rep(string.char(255), 4)} :: any)",
        "tango.base64_encode('text' :: any)",
        "tango.base64_encode(buffer.create(25165825))",
    ] {
        assert!(
            profile(false, &format!("return {{test = function() {expression} end}}"))
                .test()
                .is_err(),
            "{expression}"
        );
    }
    for source in [
        "local image = {width = 1024, height = 1024, rgba = buffer.create(4194304)}; for _ = 1, 12 do tango.encode_png(image) end",
        "local bytes = buffer.create(16777216); for _ = 1, 4 do tango.base64_encode(bytes) end",
    ] {
        let error = profile(false, &format!("return {{test = function() {source} end}}")).test().unwrap_err();
        assert!(error.to_string().contains("host work budget"), "{error}");
    }
}

#[test]
fn native_lz77_handles_offsets_without_copying_cartridges() {
    profile(
        false,
        r#"return {test = function()
        local compressed = "\x10\x05\x00\x00\x40a\xf0\x00"
        local source = buffer.fromstring("prefix" .. compressed .. "trailing")
        local decoded = tango.unlz77(source, 6)
        assert(decoded and buffer.tostring(decoded) == "aaaaa")
        buffer.writeu8(decoded, 0, 0)
        assert(buffer.tostring(source) == "prefix" .. compressed .. "trailing")
        local direct = tango.unlz77(buffer.fromstring(compressed))
        assert(direct and buffer.tostring(direct) == "aaaaa")
        assert(tango.unlz77(source) == nil)
        assert(tango.unlz77(source, buffer.len(source)) == nil)
        assert(tango.unlz77(buffer.fromstring("\x10\x03\x00\x00\x80\x00\x00")) == nil)
        assert(tango.unlz77(buffer.fromstring("\x10\x05\x00\x00\x40a\xf0")) == nil)
        local empty = tango.unlz77(buffer.fromstring("\x10\x00\x00\x00"))
        assert(empty and buffer.len(empty) == 0)

        -- Repeated small streams inside a maximum-sized ROM must cost only
        -- their compressed/output bytes, not one cartridge copy per call.
        local rom = buffer.create(33554432)
        buffer.writestring(rom, 33554432 - #compressed, compressed)
        for _ = 1, 100 do
            local bytes = tango.unlz77(rom, 33554432 - #compressed)
            assert(bytes and buffer.tostring(bytes) == "aaaaa")
        end
    end}"#,
    )
    .test()
    .unwrap();
}

#[test]
fn native_lz77_rejects_invalid_arguments_and_charges_malformed_streams() {
    for offset in ["-1", "0.5", "0/0", "math.huge", "5"] {
        let source = format!("return {{test = function() tango.unlz77(buffer.create(4), {offset}) end}}");
        let error = profile(false, &source).test().unwrap_err().to_string();
        assert!(error.contains("invalid LZ77 offset"), "{offset}: {error}");
    }
    for (expression, expected) in [
        ("tango.unlz77('bad' :: any)", "buffer"),
        ("tango.unlz77(buffer.create(33554433))", "LZ77 buffer exceeds 32 MiB"),
        // Incomplete payloads still charge their advertised expansion before
        // allocation. Catching nil must not allow unlimited native work.
        (
            r#"for _ = 1, 3 do assert(tango.unlz77(buffer.fromstring("\x10\xff\xff\xff")) == nil) end"#,
            "host work budget exceeded",
        ),
    ] {
        let source = format!("return {{test = function() {expression} end}}");
        let error = profile(false, &source).test().unwrap_err().to_string();
        assert!(error.contains(expected), "{expression}: {error}");
    }
}

#[test]
fn unicode_lowercase_is_locale_independent_and_bounded() {
    let source = r#"return {test = function()
        assert(tango.lowercase("ÉCLAIR ΟΣ ΟΣΑ İ Å 保存") == "éclair ος οσα i\u{307} å 保存")
        assert(tango.lowercase("") == "")
    end}"#;
    profile(false, source).with_locale("tr-TR").unwrap().test().unwrap();
    for (expression, expected) in [
        ("tango.lowercase(42 :: any)", "expects a string"),
        ("tango.lowercase(string.char(255))", "invalid utf-8"),
        ("tango.lowercase(string.rep('x', 16385))", "text exceeds 16 KiB"),
        (
            "tango.lowercase(string.rep('İ', 8192))",
            "lowercase text exceeds 16 KiB",
        ),
        (
            "for _ = 1, 10000 do tango.lowercase(string.rep('X', 16000)) end",
            "host work budget exceeded",
        ),
    ] {
        let source = format!("return {{test = function() {expression} end}}");
        let error = profile(false, &source).test().unwrap_err();
        assert!(error.to_string().contains(expected), "{expression}: {error}");
    }
}

#[test]
fn snapshots_survive_transactions_and_rebinding_changes_only_the_new_environment() {
    let profile = profile(
        true,
        r#"
local function value(): number
    assert(tango.input_size("rom") == 3)
    assert(tango.input_size("absent") == nil)
    local copy = tango.read_input("rom", 1, 1)
    local n = buffer.readu8(copy, 0)
    buffer.writeu8(copy, 0, 255)
    return n
end
return {
        decode = function(b: buffer): buffer assert(value() == 8); return b end,
        encode = function(b: buffer): buffer assert(value() == 8); return b end,
        validate = function(b: buffer): {string} assert(value() == 8); return {} end,
        view = function(b: buffer, s: ViewState): Node
            assert(value() == 8)
            return {kind = "button", id = "copy", text = "Copy"}
        end,
        update = function(b: buffer, s: ViewState, a: Action) buffer.writeu8(b, 0, value()); return nil end,
    }
"#,
    );
    let bound = profile.clone().with_inputs(inputs(&[7, 8, 9]));
    assert_ne!(profile.digest(), bound.digest());
    assert_eq!(bound.digest(), bound.clone().with_inputs(inputs(&[7, 8, 9])).digest());
    assert_ne!(bound.digest(), bound.clone().with_inputs(inputs(&[7, 10, 9])).digest());
    assert_eq!(profile.digest(), bound.clone().with_inputs(Inputs::default()).digest());
    let mut doc = Document::open(bound, &[1]).unwrap();
    doc.dispatch(Action::activate("copy", "")).unwrap();
    assert_eq!(doc.encode().unwrap(), [8]);
    doc.undo().unwrap();
    assert_eq!(doc.encode().unwrap(), [1]);
    doc.redo().unwrap();
    assert_eq!(doc.encode().unwrap(), [8]);
}

#[test]
fn input_reads_reject_missing_slots_invalid_numbers_ranges_and_excess_work() {
    for request in [
        "tango.read_input('missing', 0, 1)",
        "tango.read_input('../rom', 0, 1)",
        "tango.read_input('rom', -1, 1)",
        "tango.read_input('rom', 0.5, 1)",
        "tango.read_input('rom', 0, 0.5)",
        "tango.read_input('rom', 0/0, 1)",
        "tango.read_input('rom', 0, math.huge)",
        "tango.read_input('rom', 3, 1)",
        "tango.read_input('rom', 4, 0)",
    ] {
        let profile =
            profile(false, &format!("return {{test = function() {request} end}}")).with_inputs(inputs(&[7, 8, 9]));
        assert!(profile.test().is_err(), "{request}");
    }
    profile(
        false,
        "return {test = function() assert(buffer.len(tango.read_input('rom', 3, 0)) == 0) end}",
    )
    .with_inputs(inputs(&[7, 8, 9]))
    .test()
    .unwrap();
    let profile = profile(
        false,
        "return {test = function() for i = 1, 129 do local b = tango.read_input('rom', 0, 1048576) end end}",
    )
    .with_inputs(inputs(&vec![0; 1024 * 1024]));
    let error = profile.test().unwrap_err().to_string();
    assert!(error.contains("host work budget"), "{error}");
    for name in ["../rom", "a/b", "a\\b", "/rom", ""] {
        assert!(Inputs::new([(name.into(), vec![])].into()).is_err());
    }
    assert!(Inputs::new((0..17).map(|i| (i.to_string(), vec![])).collect()).is_err());
}

#[test]
fn native_crc32_is_standard_and_charges_work() {
    profile(
        false,
        r#"return {test = function()
        assert(tango.crc32(buffer.create(0)) == 0)
        assert(tango.crc32(buffer.fromstring('123456789')) == 0xcbf43926)
    end}"#,
    )
    .test()
    .unwrap();
    for source in [
        "tango.crc32(buffer.create(33554433))",
        "local bytes = buffer.create(33554432); for _ = 1, 5 do tango.crc32(bytes) end",
    ] {
        assert!(profile(false, &format!("return {{test = function() {source} end}}"))
            .test()
            .is_err());
    }
}
