use tango_script::{Inputs, Package, PackageRef, Profile};

fn profile(source: &str, png: &[u8]) -> Profile {
    let package = Package::load(
        [
            (
                "package.toml".into(),
                b"api=1\nname='image-test'\nversion='1.0.0'".to_vec(),
            ),
            (
                "init.luau".into(),
                format!("--!strict\nreturn {{test = function() {source} end}}").into_bytes(),
            ),
        ]
        .into(),
    )
    .unwrap();
    Profile::resolve(
        &[package],
        &PackageRef {
            name: "image-test".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
    .with_inputs(Inputs::new([("png".into(), png.to_vec())].into()).unwrap())
}

fn encode(image: &image::DynamicImage) -> Vec<u8> {
    let mut output = std::io::Cursor::new(Vec::new());
    image.write_to(&mut output, image::ImageFormat::Png).unwrap();
    output.into_inner()
}

#[test]
fn png_decode_returns_independent_rgba_buffers_across_color_formats() {
    use image::{DynamicImage, ImageBuffer};
    let cases = [
        DynamicImage::ImageLuma8(ImageBuffer::from_raw(2, 1, vec![0, 128]).unwrap()),
        DynamicImage::ImageLumaA8(ImageBuffer::from_raw(2, 1, vec![42, 0, 128, 255]).unwrap()),
        DynamicImage::ImageRgb8(ImageBuffer::from_raw(2, 1, vec![255, 0, 128, 16, 32, 48]).unwrap()),
        DynamicImage::ImageRgba8(ImageBuffer::from_raw(2, 1, vec![255, 0, 128, 0, 16, 32, 48, 128]).unwrap()),
        DynamicImage::ImageLuma16(ImageBuffer::from_raw(2, 1, vec![0, 0xffff]).unwrap()),
        DynamicImage::ImageRgba16(
            ImageBuffer::from_raw(2, 1, vec![0xffff, 0, 0x8080, 0, 0x1010, 0x2020, 0x3030, 0x8080]).unwrap(),
        ),
    ];
    for image in cases {
        let expected = image
            .to_rgba8()
            .as_raw()
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let source = format!(
            r#"
            local input = tango.read_input("png", 0, tango.input_size("png") :: number)
            local original = buffer.tostring(input)
            local decoded: Raster = tango.decode_png(input)
            assert(decoded.width == 2 and decoded.height == 1 and typeof(decoded.rgba) == "buffer")
            local expected = {{{expected}}}
            assert(buffer.len(decoded.rgba) == #expected)
            for index, byte in expected do assert(buffer.readu8(decoded.rgba, index - 1) == byte) end
            local roundtrip = tango.decode_png(tango.encode_png(decoded))
            assert(buffer.tostring(roundtrip.rgba) == buffer.tostring(decoded.rgba))
            buffer.fill(decoded.rgba, 0, 7)
            assert(buffer.tostring(input) == original)
            local again = tango.decode_png(input)
            for index, byte in expected do assert(buffer.readu8(again.rgba, index - 1) == byte) end
        "#
        );
        profile(&source, &encode(&image)).test().unwrap();
    }
}

#[test]
fn png_decode_rejects_malformed_inputs_and_dimensions_before_large_allocations() {
    let png = encode(&image::DynamicImage::new_rgba8(1, 1));
    let decode = "tango.decode_png(tango.read_input('png', 0, tango.input_size('png') :: number))";
    for bytes in [&[][..], b"not a png", &png[..16], &png[..png.len() / 2]] {
        assert!(profile(decode, bytes).test().is_err());
    }
    for (width, height) in [(0, 1), (4097, 1), (1, 4097), (4096, 4096)] {
        let mut oversized = png.clone();
        oversized[16..20].copy_from_slice(&u32::to_be_bytes(width));
        oversized[20..24].copy_from_slice(&u32::to_be_bytes(height));
        let crc = crc32fast::hash(&oversized[12..29]);
        oversized[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(profile(decode, &oversized).test().is_err(), "{width}x{height}");
    }
    for source in [
        "tango.decode_png('text' :: any)",
        "tango.decode_png(buffer.create(33554433))",
    ] {
        assert!(profile(source, &[]).test().is_err());
    }
}

#[test]
fn png_decode_charges_expanded_images() {
    let png = encode(&image::DynamicImage::new_rgba8(1024, 1024));
    let error = profile(
        r#"
        local bytes = tango.read_input("png", 0, tango.input_size("png") :: number)
        for _ = 1, 12 do tango.decode_png(bytes) end
    "#,
        &png,
    )
    .test()
    .unwrap_err();
    assert!(error.to_string().contains("host work budget"), "{error}");
}
