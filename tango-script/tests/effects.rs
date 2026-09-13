use tango_script::{Action, Document, EditorContext, Effect, Package, PackageRef, Profile};

fn document(update: &str, read_only: bool) -> Document {
    let source = format!(
        r#"--!strict
return {{
    decode = function(bytes: buffer): buffer return bytes end,
    encode = function(bytes: buffer): buffer return bytes end,
    validate = function(_bytes: buffer): {{string}} return {{}} end,
    view = function(bytes: buffer, state: ViewState): Node
        assert(state.fail ~= "yes", "view failed")
        return {{kind = "button", id = "copy", text = tostring(buffer.readu8(bytes, 0)) .. (state.value or "")}}
    end,
    update = function(bytes: buffer, state: ViewState, _action: Action): {{Effect}}?
        {update}
    end,
}}"#
    );
    let package = Package::load(
        [
            (
                "package.toml".into(),
                b"api = 1\nname = 'effects'\nversion = '1.0.0'\n[[editor]]\nname = 'main'\npath = './init'\n".to_vec(),
            ),
            ("init.luau".into(), source.into_bytes()),
        ]
        .into(),
    )
    .unwrap();
    let profile = Profile::resolve(
        &[package],
        &PackageRef {
            name: "effects".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap();
    Document::open_with_context(
        profile,
        &[7],
        EditorContext {
            read_only,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn effects_are_returned_once_after_committing_and_are_allowed_in_read_only_views() {
    for read_only in [false, true] {
        let mut doc = document(
            r#"
            if state.value then return nil end
            state.value = "copied"
            return {
                {kind = "copy_text", text = "text"} :: Effect,
                {kind = "copy_html", text = "plain", html = "<b>rich</b>"} :: Effect,
                {kind = "copy_image", image = {width = 1, height = 1, rgba = buffer.fromstring("rgba")}} :: Effect,
                {kind = "copy_image", image = {width = 8, height = 4, commands = {
                    {kind = "rect", width = 8, height = 4, fill = {1, 0, 0, 1}} :: Draw,
                }}} :: Effect,
                {kind = "scroll_to", id = "body", x = 0.5, y = 0} :: Effect,
            }
        "#,
            read_only,
        );
        let before = doc.view().clone();
        let effects = doc.dispatch(Action::activate("copy", "")).unwrap();
        assert_eq!(effects.len(), 5);
        assert_eq!(
            effects[4],
            Effect::ScrollTo {
                id: "body".into(),
                x: 0.5,
                y: 0.0
            }
        );
        assert_eq!(effects[0], Effect::CopyText { text: "text".into() });
        assert_eq!(
            effects[1],
            Effect::CopyHtml {
                text: "plain".into(),
                html: "<b>rich</b>".into()
            }
        );
        assert!(
            matches!(&effects[2], Effect::CopyImage {image: tango_script::ui::canvas::ImageSource::Raster(image)} if image.rgba.as_ref() == b"rgba" && image.digest != [0; 32])
        );
        assert!(
            matches!(&effects[3], Effect::CopyImage {image: tango_script::ui::canvas::ImageSource::Drawing(image)} if image.commands.len() == 1)
        );
        assert_ne!(doc.view(), &before);
        assert!(!doc.is_dirty() && !doc.can_undo());
        doc.set_locale("ja-JP").unwrap();
        assert!(doc.dispatch(Action::activate("copy", "")).unwrap().is_empty());
        assert_eq!(doc.encode().unwrap(), [7]);
    }
}

#[test]
fn failed_updates_discard_effects_and_all_document_state() {
    for (update, read_only) in [
        (
            r#"state.value = "bad"; buffer.writeu8(bytes, 0, 8); return {{kind = "edit", command = "begin"} :: Effect}"#,
            false,
        ),
        (
            r#"state.fail = "yes"; buffer.writeu8(bytes, 0, 8); return {{kind = "copy_text", text = "hidden"} :: Effect}"#,
            false,
        ),
        (
            r#"state.value = "bad"; buffer.writeu8(bytes, 0, 8); return {{kind = "unknown"} :: any}"#,
            false,
        ),
        (
            r#"state.value = "bad"; buffer.writeu8(bytes, 0, 8); return {{kind = "copy_text", text = "hidden"} :: Effect}"#,
            true,
        ),
        (
            r#"state.value = "bad"; buffer.writeu8(bytes, 0, 8); error("update failed"); return nil"#,
            false,
        ),
    ] {
        let mut doc = document(update, read_only);
        let view = doc.view().clone();
        assert!(doc.dispatch(Action::activate("copy", "")).is_err());
        assert_eq!(doc.view(), &view);
        assert_eq!(doc.encode().unwrap(), [7]);
        assert!(!doc.is_dirty() && !doc.can_undo() && !doc.can_redo());
    }
}

#[test]
fn malformed_or_excessive_effects_are_rejected_before_commit() {
    for output in [
        "{[2] = {kind = 'copy_text', text = 'gap'}}",
        "{named = {kind = 'copy_text', text = 'named'}}",
        "{{kind = 'copy_html', html = '<b>no fallback</b>'}}",
        "{{kind = 'scroll_to', id = '', x = 0, y = 0}}",
        "{{kind = 'scroll_to', id = string.rep('s', 1025), x = 0, y = 0}}",
        "{{kind = 'scroll_to', id = 'tabs', x = -0.1, y = 0}}",
        "{{kind = 'scroll_to', id = 'tabs', x = 0/0, y = 0}}",
        "{{kind = 'scroll_to', id = 'tabs', x = 0, y = 1.1}}",
        "{{kind = 'copy_image', image = {width = 1, height = 1, rgba = buffer.create(3)}}}",
        "{{kind = 'copy_image', image = {width = 1, height = 1, rgba = buffer.create(4), commands = {}}}}",
        "{{kind = 'copy_image', image = {width = 8192, height = 1, commands = {}}}}",
        "table.create(17, {kind = 'copy_text', text = 'many'})",
        "{{kind = 'copy_text', text = string.rep('x', 4194305)}}",
        "{{kind = 'copy_image', image = {width = 4096, height = 4096, commands = {}}} :: any, {kind = 'copy_image', image = {width = 1, height = 1, rgba = buffer.create(4)}} :: any}",
    ] {
        let mut doc = document(&format!("state.value = 'bad'; buffer.writeu8(bytes, 0, 8); return {output} :: any"), false);
        let view = doc.view().clone();
        assert!(doc.dispatch(Action::activate("copy", "")).is_err(), "{output}");
        assert_eq!(doc.view(), &view);
        assert_eq!(doc.encode().unwrap(), [7]);
    }
}

#[test]
fn png_and_base64_encoding_preserve_pixels_and_match_native_png_output() {
    use base64::Engine;
    let mut doc = document(
        r#"
        assert(tango.base64_encode(buffer.fromstring("")) == "")
        assert(tango.base64_encode(buffer.fromstring("f")) == "Zg==")
        assert(tango.base64_encode(buffer.fromstring("fo")) == "Zm8=")
        assert(tango.base64_encode(buffer.fromstring("foo")) == "Zm9v")
        local rgba = buffer.fromstring("\xff\x00\x80\x00\x10\x20\x30\x80")
        local png = tango.encode_png({width = 2, height = 1, rgba = rgba})
        buffer.fill(rgba, 0, 0)
        return {{kind = "copy_text", text = tango.base64_encode(png)} :: Effect}
    "#,
        true,
    );
    let [Effect::CopyText { text }] = &doc.dispatch(Action::activate("copy", "")).unwrap()[..] else {
        panic!()
    };
    let png = base64::engine::general_purpose::STANDARD.decode(text).unwrap();
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .unwrap()
        .to_rgba8();
    assert_eq!(image.dimensions(), (2, 1));
    assert_eq!(image.as_raw(), &[255, 0, 128, 0, 16, 32, 48, 128]);
    let mut native = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut native, image::ImageFormat::Png)
        .unwrap();
    assert_eq!(png, native.into_inner());
}
