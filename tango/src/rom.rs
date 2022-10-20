pub mod text;

use byteorder::{ByteOrder, ReadBytesExt};
use serde::Deserialize;

use crate::{game, scanner};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub enum ChipClass {
    Standard,
    Mega,
    Giga,
    None,
    ProgramAdvance,
}

pub trait Chip {
    fn name(&self) -> String;
    fn description(&self) -> String;
    fn icon(&self) -> image::RgbaImage;
    fn image(&self) -> image::RgbaImage;
    fn codes(&self) -> Vec<u8>;
    fn element(&self) -> usize;
    fn class(&self) -> ChipClass;
    fn dark(&self) -> bool;
    fn mb(&self) -> u8;
    fn damage(&self) -> u32;
}

pub struct Modcard56Effect {
    pub id: u8,
    pub name: String,
    pub parameter: u8,
    pub is_ability: bool,
    pub is_debuff: bool,
}

pub trait Modcard56 {
    fn name(&self) -> String;
    fn mb(&self) -> u8;
    fn effects(&self) -> Vec<Modcard56Effect>;
}

pub trait Modcard4 {
    fn name(&self) -> String;
    fn slot(&self) -> u8;
    fn effect(&self) -> String;
    fn bug(&self) -> Option<String>;
}

#[derive(Debug, Clone, PartialEq, Eq, std::hash::Hash, serde::Serialize)]
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

pub trait NavicustPart {
    fn name(&self) -> String;
    fn description(&self) -> String;
    fn color(&self) -> Option<NavicustPartColor>;
    fn is_solid(&self) -> bool;
    fn compressed_bitmap(&self) -> NavicustBitmap;
    fn uncompressed_bitmap(&self) -> NavicustBitmap;
}

pub trait Style {
    fn name(&self) -> String;
    fn extra_ncp_color(&self) -> Option<NavicustPartColor>;
}

#[derive(Debug, Clone)]
pub enum Modcard56EffectTemplatePart {
    String(String),
    PrintVar(usize),
}

pub type Modcard56EffectTemplate = Vec<Modcard56EffectTemplatePart>;

pub trait Navi {
    fn name(&self) -> String;
    fn emblem(&self) -> image::RgbaImage;
}

pub trait Assets {
    fn chip<'a>(&'a self, id: usize) -> Option<Box<dyn Chip + 'a>>;
    fn num_chips(&self) -> usize;
    fn element_icon(&self, id: usize) -> Option<image::RgbaImage>;
    fn modcard56<'a>(&'a self, id: usize) -> Option<Box<dyn Modcard56 + 'a>> {
        let _ = id;
        None
    }
    fn num_modcard56s(&self) -> usize {
        0
    }
    fn modcard4<'a>(&'a self, id: usize) -> Option<Box<dyn Modcard4 + 'a>> {
        let _ = id;
        None
    }
    fn num_modcard4s(&self) -> usize {
        0
    }
    fn navicust_part<'a>(&'a self, id: usize, variant: usize) -> Option<Box<dyn NavicustPart + 'a>> {
        let _ = id;
        let _ = variant;
        None
    }
    fn num_navicust_parts(&self) -> (usize, usize) {
        (0, 0)
    }
    fn navicust_bg(&self) -> Option<image::Rgba<u8>> {
        None
    }
    fn style<'a>(&'a self, id: usize) -> Option<Box<dyn Style + 'a>> {
        let _ = id;
        None
    }
    fn num_styles(&self) -> usize {
        0
    }
    fn navi<'a>(&'a self, id: usize) -> Option<Box<dyn Navi + 'a>> {
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

pub struct MemoryMapper {
    rom: Vec<u8>,
    wram: Vec<u8>,
    unlz77_cache: parking_lot::Mutex<std::collections::HashMap<u32, Vec<u8>>>,
}

impl MemoryMapper {
    pub fn new(rom: Vec<u8>, wram: Vec<u8>) -> Self {
        Self {
            rom,
            wram,
            unlz77_cache: parking_lot::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn get<'a>(&'a self, start: u32) -> std::borrow::Cow<'a, [u8]> {
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

fn deserialize_option_language_identifier<'de, D>(
    deserializer: D,
) -> Result<Option<unic_langid::LanguageIdentifier>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    if let Some(buf) = Option::<String>::deserialize(deserializer)? {
        buf.parse().map(|v| Some(v)).map_err(serde::de::Error::custom)
    } else {
        Ok(None)
    }
}

fn deserialize_option_modcard56_effect_template<'de, D>(
    deserializer: D,
) -> Result<Option<Modcard56EffectTemplate>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    struct ShortTemplatePart {
        t: Option<String>,
        p: Option<u32>,
    }

    Ok(Option::<Vec<ShortTemplatePart>>::deserialize(deserializer)?.map(|v| {
        v.into_iter()
            .flat_map(|v| {
                if let Some(t) = v.t {
                    vec![Modcard56EffectTemplatePart::String(t)]
                } else if let Some(p) = v.p {
                    vec![Modcard56EffectTemplatePart::PrintVar(p as usize)]
                } else {
                    vec![]
                }
            })
            .collect()
    }))
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct ChipOverride {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct NavicustPartOverride {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct Modcard56Override {
    pub name: Option<String>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct Modcard56EffectOverride {
    #[serde(deserialize_with = "deserialize_option_modcard56_effect_template")]
    pub name_template: Option<Modcard56EffectTemplate>,
}

#[derive(serde::Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct Overrides {
    #[serde(deserialize_with = "deserialize_option_language_identifier")]
    pub language: Option<unic_langid::LanguageIdentifier>,
    pub charset: Option<Vec<String>>,
    pub chips: Option<Vec<ChipOverride>>,
    pub navicust_parts: Option<Vec<NavicustPartOverride>>,
    pub modcard56s: Option<Vec<Modcard56Override>>,
    pub modcard56_effects: Option<Vec<Modcard56EffectOverride>>,
}
