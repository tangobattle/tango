pub mod text;

use byteorder::{ByteOrder, ReadBytesExt};

use crate::{game, scanner};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChipClass {
    Standard,
    Mega,
    Giga,
    None,
    ProgramAdvance,
}

#[derive(Clone, Debug)]
pub struct Chip {
    pub name: String,
    pub description: String,
    pub icon: image::RgbaImage,
    pub image: image::RgbaImage,
    pub codes: Vec<u8>,
    pub element: usize,
    pub class: ChipClass,
    pub dark: bool,
    pub mb: u8,
    pub damage: u32,
}

#[derive(Clone, Debug)]
pub struct Modcard56Effect {
    pub id: u8,
    pub name: String,
    pub parameter: u8,
    pub is_ability: bool,
    pub is_debuff: bool,
}

#[derive(Clone, Debug)]
pub struct Modcard56 {
    pub name: String,
    pub mb: u8,
    pub effects: Vec<Modcard56Effect>,
}

#[derive(Clone, Debug)]
pub struct Modcard4 {
    pub name: &'static str,
    pub slot: u8,
    pub effect: &'static str,
    pub bug: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub enum NavicustPartColor {
    White,
    Yellow,
    Pink,
    Red,
    Blue,
    Green,
    Orange,
    Purple,
    Gray,
}

pub type NavicustBitmap = image::ImageBuffer<image::Luma<u8>, Vec<u8>>;

#[derive(Clone, Debug)]
pub struct NavicustPart {
    pub name: String,
    pub description: String,
    pub color: Option<NavicustPartColor>,
    pub is_solid: bool,
    pub compressed_bitmap: NavicustBitmap,
    pub uncompressed_bitmap: NavicustBitmap,
}

#[derive(Clone, Debug)]
pub struct Style {
    pub name: String,
    pub extra_ncp_color: Option<NavicustPartColor>,
}

#[derive(Debug, Clone)]
pub enum Modcard56EffectTemplatePart {
    String(String),
    PrintVar(usize),
}

pub type Modcard56EffectTemplate = Vec<Modcard56EffectTemplatePart>;

#[derive(Clone, Debug)]
pub struct Navi {
    pub name: String,
    pub emblem: image::RgbaImage,
}

pub trait Assets {
    fn chip(&self, id: usize) -> Option<&Chip>;
    fn num_chips(&self) -> usize;
    fn element_icon(&self, id: usize) -> Option<&image::RgbaImage>;
    fn modcard56(&self, id: usize) -> Option<&Modcard56> {
        let _ = id;
        None
    }
    fn num_modcard56s(&self) -> usize {
        0
    }
    fn modcard4(&self, id: usize) -> Option<&Modcard4> {
        let _ = id;
        None
    }
    fn num_modcard4s(&self) -> usize {
        0
    }
    fn navicust_part(&self, id: usize, variant: usize) -> Option<&NavicustPart> {
        let _ = id;
        let _ = variant;
        None
    }
    fn num_navicust_parts(&self) -> (usize, usize) {
        (0, 0)
    }
    fn style(&self, id: usize) -> Option<&Style> {
        let _ = id;
        None
    }
    fn num_styles(&self) -> usize {
        0
    }
    fn navi(&self, id: usize) -> Option<&Navi> {
        let _ = id;
        None
    }
    fn num_navis(&self) -> usize {
        0
    }
}

pub fn bgr555_to_rgba(c: u16) -> image::Rgba<u8> {
    image::Rgba([
        {
            let r = c & 0b11111;
            (r << 3 | r >> 2) as u8
        },
        {
            let g = (c >> 5) & 0b11111;
            (g << 3 | g >> 2) as u8
        },
        {
            let b = (c >> 10) & 0b11111;
            (b << 3 | b >> 2) as u8
        },
        0xff,
    ])
}

pub fn read_palette(raw: &[u8]) -> [image::Rgba<u8>; 16] {
    [image::Rgba([0, 0, 0, 0])]
        .into_iter()
        .chain((1..16).map(|i| bgr555_to_rgba(byteorder::LittleEndian::read_u16(&raw[(i * 2)..((i + 1) * 2)]))))
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
        image::imageops::replace(&mut img, tile, (x * TILE_WIDTH) as i64, (y * TILE_HEIGHT) as i64);
    }
    img
}

pub fn apply_palette(paletted: PalettedImage, palette: &[image::Rgba<u8>; 16]) -> image::RgbaImage {
    image::ImageBuffer::from_vec(
        paletted.width(),
        paletted.height(),
        paletted.into_iter().flat_map(|v| palette[*v as usize].0).collect(),
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

pub fn unlz77(mut r: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = vec![];

    let header = r.read_u32::<byteorder::LittleEndian>()?;
    if (header & 0xff) != 0x10 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid header"));
    }

    let n = (header >> 8) as usize;
    while out.len() < n {
        let ref_ = r.read_u8()?;

        for i in 0..8 {
            if out.len() >= n {
                break;
            }

            if (ref_ & (0x80 >> i)) == 0 {
                out.push(r.read_u8()?);
                continue;
            }

            // Yes that's right, it's big endian here!
            let info = r.read_u16::<byteorder::BigEndian>()?;

            let m = info >> 12;
            let offset = info & 0x0fff;

            for _ in 0..(m + 3) {
                out.push(out[out.len() - offset as usize - 1]);
            }
        }
    }

    out.truncate(n);
    Ok(out)
}

pub struct MemoryMapper<'a> {
    rom: &'a [u8],
    wram: &'a [u8],
    unlz77_cache: parking_lot::Mutex<std::collections::HashMap<u32, Vec<u8>>>,
}

impl<'a> MemoryMapper<'a> {
    pub fn new(rom: &'a [u8], wram: &'a [u8]) -> Self {
        Self {
            rom,
            wram,
            unlz77_cache: parking_lot::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn get(&self, start: u32) -> std::borrow::Cow<'a, [u8]> {
        if start >= 0x02000000 && start < 0x04000000 {
            std::borrow::Cow::Borrowed(&self.wram[(start & !0x02000000) as usize..])
        } else if start >= 0x08000000 && start < 0x0a000000 {
            std::borrow::Cow::Borrowed(&self.rom[(start & !0x08000000) as usize..])
        } else if start >= 0x88000000 && start <= 0x8a000000 {
            std::borrow::Cow::Owned(
                self.unlz77_cache
                    .lock()
                    .entry(start)
                    .or_insert_with(|| unlz77(&self.rom[(start & !0x88000000) as usize..]).unwrap()[4..].to_vec())
                    .clone(),
            )
        } else {
            panic!("could not get slice")
        }
    }
}

pub type Scanner = scanner::Scanner<std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<u8>>>;
