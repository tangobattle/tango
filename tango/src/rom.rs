use byteorder::ByteOrder;

#[derive(Clone, Debug)]
pub enum ChipClass {
    Standard,
    Mega,
    Giga,
}

#[derive(Clone, Debug)]
pub struct Chip {
    pub name: String,
    pub icon: image::RgbaImage,
    pub codes: Vec<u8>,
    pub element: usize,
    pub class: ChipClass,
    pub dark: bool,
    pub mb: u32,
    pub damage: u32,
}

pub trait Assets {
    fn chip(&self, id: usize) -> Option<&Chip>;
    fn element_icon(&self, id: usize) -> Option<&image::RgbaImage>;
}

pub fn bgr555_to_rgba(c: u16) -> image::Rgba<u8> {
    image::Rgba([
        (((c & 0b11111) * 0xff) / 0b11111) as u8,
        ((((c >> 5) & 0b11111) * 0xff) / 0b11111) as u8,
        ((((c >> 10) & 0b11111) * 0xff) / 0b11111) as u8,
        0xff,
    ])
}

pub fn read_palette(raw: &[u8]) -> [image::Rgba<u8>; 16] {
    [image::Rgba([0, 0, 0, 0])]
        .into_iter()
        .chain((1..16).map(|i| {
            bgr555_to_rgba(byteorder::LittleEndian::read_u16(
                &raw[(i * 2)..((i + 1) * 2)],
            ))
        }))
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

type PalettedImage = image::ImageBuffer<image::Luma<u8>, Vec<u8>>;

pub const TILE_WIDTH: usize = 8;
pub const TILE_HEIGHT: usize = 8;
pub const TILE_BYTES: usize = TILE_WIDTH * TILE_HEIGHT / 2;

pub fn read_tile(raw: &[u8]) -> Option<PalettedImage> {
    image::ImageBuffer::from_vec(
        TILE_WIDTH as u32,
        TILE_HEIGHT as u32,
        raw.iter().flat_map(|v| vec![v & 0xf, v >> 4]).collect(),
    )
}

pub fn merge_tiles(tiles: &[PalettedImage], cols: usize) -> PalettedImage {
    let rows = tiles.len() / cols;
    let mut img = image::ImageBuffer::new((cols * TILE_WIDTH) as u32, (rows * TILE_HEIGHT) as u32);
    for (i, tile) in tiles.iter().enumerate() {
        let x = i % cols;
        let y = i / cols;
        image::imageops::overlay(
            &mut img,
            tile,
            (x * TILE_WIDTH) as i64,
            (y * TILE_HEIGHT) as i64,
        );
    }
    img
}

pub fn apply_palette(paletted: PalettedImage, palette: &[image::Rgba<u8>; 16]) -> image::RgbaImage {
    image::ImageBuffer::from_vec(
        paletted.width(),
        paletted.height(),
        paletted
            .into_iter()
            .flat_map(|v| palette[*v as usize].0)
            .collect(),
    )
    .unwrap()
}

pub fn read_merged_tiles(raw: &[u8], cols: usize) -> Option<PalettedImage> {
    let tiles = raw
        .chunks(TILE_BYTES)
        .map(|raw_tile| read_tile(raw_tile))
        .collect::<Option<Vec<_>>>()?;
    Some(merge_tiles(&tiles, cols))
}
