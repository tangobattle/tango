//! Bounded, game-neutral image conversion for package assets and exports.
use std::sync::{atomic::AtomicUsize, Arc};

use image::{ImageDecoder, ImageEncoder};
use mlua::{Buffer, Lua, Table, Value};

use crate::{
    decode,
    runtime::{charge, MAX_BUFFER},
    ui::Raster,
    Result,
};

pub(crate) fn install(lua: &Lua, host: &Table, budget: Arc<AtomicUsize>) -> Result<()> {
    let decode_budget = budget.clone();
    host.set(
        "decode_png",
        lua.create_function(move |lua, bytes: Buffer| {
            if bytes.len() > MAX_BUFFER {
                return Err(mlua::Error::runtime("PNG input exceeds 32 MiB"));
            }
            charge(&decode_budget, bytes.len() * 2).map_err(mlua::Error::external)?;
            // Apply limits before parsing: PNG metadata and decompression must
            // not allocate without a bound, even when the stream is malformed.
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(4096);
            limits.max_image_height = Some(4096);
            limits.max_alloc = Some(MAX_BUFFER as u64);
            let decoder = image::codecs::png::PngDecoder::with_limits(std::io::Cursor::new(bytes.to_vec()), limits)
                .map_err(mlua::Error::external)?;
            let (width, height) = decoder.dimensions();
            let rgba_bytes = width as usize * height as usize * 4;
            let decoded_bytes = usize::try_from(decoder.total_bytes()).map_err(mlua::Error::external)?;
            if rgba_bytes > MAX_BUFFER || decoded_bytes > MAX_BUFFER {
                return Err(mlua::Error::runtime("decoded PNG exceeds 32 MiB"));
            }
            charge(&decode_budget, decoded_bytes + rgba_bytes * 2).map_err(mlua::Error::external)?;
            let image = image::DynamicImage::from_decoder(decoder)
                .map_err(mlua::Error::external)?
                .into_rgba8();
            // mlua's Serde serializer represents bytes as strings/sequences;
            // this constructor needs a real Luau buffer for the Raster contract.
            lua.create_table_from([
                ("width", Value::Integer(width.into())),
                ("height", Value::Integer(height.into())),
                ("rgba", Value::Buffer(lua.create_buffer(image.as_raw())?)),
            ])
        })?,
    )?;
    let png_budget = budget.clone();
    host.set(
        "encode_png",
        lua.create_function(move |lua, value: Value| {
            let mut image: Raster = decode::from_value(
                lua,
                value,
                decode::Limits {
                    values: 32,
                    bytes: MAX_BUFFER,
                    depth: 1,
                    string: 64,
                    root: decode::Root::Map,
                },
                false,
            )
            .map_err(mlua::Error::external)?;
            let mut bytes_left = MAX_BUFFER;
            image.validate(&mut bytes_left).map_err(mlua::Error::external)?;
            charge(&png_budget, image.rgba.len() * 3).map_err(mlua::Error::external)?;
            let mut output = Vec::new();
            image::codecs::png::PngEncoder::new(&mut output)
                .write_image(&image.rgba, image.width, image.height, image::ExtendedColorType::Rgba8)
                .map_err(mlua::Error::external)?;
            if output.len() > MAX_BUFFER {
                return Err(mlua::Error::runtime("PNG exceeds 32 MiB"));
            }
            charge(&png_budget, output.len() * 2).map_err(mlua::Error::external)?;
            lua.create_buffer(output)
        })?,
    )?;
    host.set(
        "base64_encode",
        lua.create_function(move |_, bytes: Buffer| {
            use base64::Engine;
            let size = bytes.len().div_ceil(3) * 4;
            if size > MAX_BUFFER {
                return Err(mlua::Error::runtime("base64 output exceeds 32 MiB"));
            }
            charge(&budget, bytes.len() + size).map_err(mlua::Error::external)?;
            Ok(base64::engine::general_purpose::STANDARD.encode(bytes.to_vec()))
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_pngs_spend_the_shared_host_budget_too() {
        let lua = crate::runtime::new_lua();
        let host = lua.create_table().unwrap();
        install(&lua, &host, Arc::new(AtomicUsize::new(2 * 1024 * 1024))).unwrap();
        let decode: mlua::Function = host.get("decode_png").unwrap();
        let bytes = lua.create_buffer(vec![0; 1024 * 1024]).unwrap();
        let error = decode.call::<Table>(bytes.clone()).unwrap_err().to_string();
        assert!(
            !error.contains("host work budget"),
            "first attempt reaches the PNG decoder: {error}"
        );
        let error = decode.call::<Table>(bytes.clone()).unwrap_err().to_string();
        assert!(error.contains("host work budget"), "{error}");
        let encode: mlua::Function = host.get("base64_encode").unwrap();
        let error = encode.call::<String>(bytes).unwrap_err().to_string();
        assert!(error.contains("host work budget"), "{error}");
    }
}
