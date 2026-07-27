use byteorder::ReadBytesExt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChipClass {
    Standard,
    Navi, // Only used for BN1 and 2.
    Mega,
    Giga,
    None,
    ProgramAdvance,
}

pub trait Chip {
    fn name(&self) -> Option<String>;
    fn description(&self) -> Option<String>;
    fn icon(&self) -> image::RgbaImage;
    fn image(&self) -> image::RgbaImage;
    fn codes(&self) -> Vec<char>;
    fn element(&self) -> usize;
    fn class(&self) -> ChipClass;
    fn dark(&self) -> bool;
    fn mb(&self) -> u8;
    fn attack_power(&self) -> u32;
    fn library_sort_order(&self) -> Option<usize>;
}

pub struct PatchCard56Effect {
    pub id: usize,
    pub kind: PatchCard56EffectKind,
    pub name: Option<String>,
    pub parameter: u8,
    pub is_ability: bool,
    pub is_debuff: bool,
}

pub trait PatchCard56 {
    fn name(&self) -> Option<String>;
    fn mb(&self) -> u8;
    fn effects(&self) -> Vec<PatchCard56Effect>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchCard56EffectKind {
    /// Max HP `+parameter×10`.
    MaxHpPlus,
    /// Max HP `+parameter%`.
    MaxHpPlusPercent,
    /// Max HP `-parameter×10`.
    MaxHpMinus,
    /// Max HP `-parameter%`.
    MaxHpMinusPercent,
    /// Set element to Normal.
    NormalBody,
    /// Set element to Fire.
    FireBody,
    /// Set element to Aqua.
    AquaBody,
    /// Set element to Elec.
    ElecBody,
    /// Set element to Wood.
    WoodBody,
    /// MegaBuster Attack `+parameter`.
    AttackPlus,
    /// MegaBuster Attack `-parameter`.
    AttackMinus,
    /// MegaBuster Attack `×parameter`.
    AttackTimes,
    /// Buster rapid (speed) `+parameter`.
    SpeedPlus,
    /// Buster rapid (speed) `-parameter`.
    SpeedMinus,
    /// Buster charge `+parameter`.
    ChargePlus,
    /// Buster charge `-parameter`.
    ChargeMinus,
    /// Custom screen `+parameter` chips.
    CustomPlus,
    /// Custom screen `-parameter` chips.
    CustomMinus,
    /// Mega-chip folder limit `+parameter`.
    MegaFolderPlus,
    /// Mega-chip folder limit `-parameter`.
    MegaFolderMinus,
    /// Giga-chip folder limit `+parameter`.
    GigaFolderPlus,
    /// Giga-chip folder limit `-parameter`.
    GigaFolderMinus,
    /// DoubleSoul duration `+parameter` turns (BN5 only).
    SoulTimePlus,
    /// DoubleSoul duration `-parameter` turns (BN5 only).
    SoulTimeMinus,
    /// Can't be pushed back.
    SuperArmor,
    /// Immune to status ailments.
    StatusGuard,
    /// Immune to panel-type effects.
    FloatShoes,
    /// Can move over holes.
    AirShoes,
    /// Survive a lethal hit on 1 HP.
    UnderShirt,
    /// The MegaBuster fires three shots at once.
    TripleBuster,
    /// The B-button tap is overridden to a fixed chip or guard. The specific
    /// chip is given by the effect's ROM name.
    BButtonChip,
    /// The MegaBuster's shots gain an added effect. The specific modifier is
    /// given by the effect's ROM name.
    BusterModifier,
    /// The charged B-button shot is overridden to a fixed chip. The specific
    /// chip is given by the effect's ROM name.
    BChargeChip,
}

#[derive(Debug, Clone, PartialEq, Eq, std::hash::Hash)]
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

pub type NavicustBitmap = ndarray::Array2<bool>;

pub trait NavicustPart {
    fn name(&self) -> Option<String>;
    fn description(&self) -> Option<String>;
    fn color(&self) -> Option<NavicustPartColor>;
    fn is_solid(&self) -> bool;
    fn compressed_bitmap(&self) -> Option<NavicustBitmap>;
    fn uncompressed_bitmap(&self) -> NavicustBitmap;
}

#[derive(Debug, Clone)]
pub enum PatchCard56EffectTemplatePart {
    String(String),
    PrintVar(usize),
}

pub type PatchCard56EffectTemplate = Vec<PatchCard56EffectTemplatePart>;

pub trait Navi {
    fn name(&self) -> Option<String>;
    fn emblem(&self) -> image::RgbaImage;

    /// The navi's intrinsic base max HP, when the game keeps a navi HP table in
    /// the ROM. Games without one return `None`, and callers fall back to the
    /// HP recorded in the save.
    fn base_max_hp(&self) -> Option<u16> {
        None
    }
}

pub struct NavicustLayout {
    pub command_line: usize,
    pub has_out_of_bounds: bool,
    pub background: image::Rgba<u8>,
}

pub trait Assets: crate::save::AsAny {
    fn chip(&self, id: usize) -> Option<Box<dyn Chip + '_>>;
    fn num_chips(&self) -> usize;
    fn chips_have_mb(&self) -> bool {
        true
    }
    fn element_icon(&self, id: usize) -> Option<image::RgbaImage>;
    fn patch_card56(&self, id: usize) -> Option<Box<dyn PatchCard56 + '_>> {
        let _ = id;
        None
    }
    fn num_patch_card56s(&self) -> usize {
        0
    }
    fn navicust_part(&self, id: usize) -> Option<Box<dyn NavicustPart + '_>> {
        let _ = id;
        None
    }
    fn num_navicust_parts(&self) -> usize {
        0
    }
    /// The display name of the style `NavicustView::style()` points at.
    /// The style *system* (types, elements, the extra-NCP color) is
    /// BN3's own model; the equipped style's name is part of the shared
    /// navicust view (the grid's color bar is titled with it), so it
    /// stays a narrow hook here — and patches can override it.
    fn style_name(&self, id: usize) -> Option<String> {
        let _ = id;
        None
    }
    fn navi(&self, id: usize) -> Option<Box<dyn Navi + '_>> {
        let _ = id;
        None
    }
    fn num_navis(&self) -> usize {
        0
    }
    /// The navis the navi-edit grid should show, grouped into the rows it
    /// should lay them out in. Each inner slice is one row of navi ids; the
    /// UI renders rows top-to-bottom and ids left-to-right exactly as given.
    fn navi_order(&self) -> &[&[usize]] {
        &[]
    }
    fn navicust_layout(&self) -> Option<NavicustLayout> {
        None
    }
    /// The game's own concrete assets, for game-specific UIs to downcast
    /// (BN4's Mod Card catalog lives there, not on this trait). Layering
    /// wrappers (the patch-override layer) forward to what they wrap;
    /// everything else answers with itself, which the default provides.
    /// Only name-override-able entities are on the shared trait, so
    /// bypassing a wrapper here loses nothing.
    fn underlying_any(&self) -> &dyn std::any::Any {
        self.as_any()
    }
}

#[repr(transparent)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
pub struct Bgr555 {
    #[bitfield(name = "r", ty = "u8", bits = "0..=4")]
    #[bitfield(name = "g", ty = "u8", bits = "5..=9")]
    #[bitfield(name = "b", ty = "u8", bits = "10..=14")]
    raw: [u8; 2],
}

impl Bgr555 {
    pub const fn new(raw: [u8; 2]) -> Self {
        Self { raw }
    }

    pub const fn to_le(&self) -> u16 {
        u16::from_le_bytes(self.raw)
    }

    pub const fn to_rgba8(&self) -> image::Rgba<u8> {
        let raw = self.to_le();
        image::Rgba([
            ((raw & 0x1f) * 0xff / 0x1f) as u8,
            (((raw >> 5) & 0x1f) * 0xff / 0x1f) as u8,
            (((raw >> 10) & 0x1f) * 0xff / 0x1f) as u8,
            0xff,
        ])
    }
}

pub type Palette = [Bgr555; 16];

/// Canonical BGR555 → RGBA8 expansion, indexed by the 15-bit value
/// (`r | g << 5 | b << 10`). Built from [`Bgr555::to_rgba8`] at compile
/// time so bulk conversion and per-color sprite/palette rendering can't
/// drift apart.
static BGR555_RGBA8_LUT: [image::Rgba<u8>; 0x8000] = {
    let mut arr = [image::Rgba([0, 0, 0, 0]); 0x8000];
    let mut i = 0u16;
    while i < 0x8000 {
        arr[i as usize] = Bgr555::new(i.to_le_bytes()).to_rgba8();
        i += 1;
    }
    arr
};

/// Convert an mGBA `BGR5` framebuffer — what `COLOR_16_BIT` builds emit: one
/// little-endian `u16` per pixel holding the GBA-native 15-bit color — into
/// RGBA8.
///
/// `src` is 2 bytes per pixel and `dst` 4 bytes per pixel; conversion runs over
/// whole pixels and stops when either buffer is exhausted. Backed by the same
/// table [`Bgr555::to_rgba8`] feeds, so emulated frames and in-app ROM imagery
/// share identical colors, at one lookup per pixel. Alpha is forced opaque.
pub fn bgr555_to_rgba8(src: &[u8], dst: &mut [u8]) {
    for (s, d) in bytemuck::cast_slice::<u8, u16>(src)
        .iter()
        .zip(bytemuck::cast_slice_mut::<_, u32>(dst).iter_mut())
    {
        // Mask to 15 bits: bit 15 is unused in GBA BGR555 (mGBA emits 0), so
        // this is a no-op on the value, but it lets the compiler prove the
        // index is < 0x8000 and elide the per-pixel bounds check.
        *d = bytemuck::cast(BGR555_RGBA8_LUT[(*s & 0x7fff) as usize].0);
    }
}

type PalettedImage = image::ImageBuffer<image::Luma<u8>, Vec<u8>>;

pub const TILE_WIDTH: usize = 8;
pub const TILE_HEIGHT: usize = 8;
pub const TILE_BYTES: usize = TILE_WIDTH * TILE_HEIGHT / 2;

pub fn read_tile(raw: &[u8]) -> Result<PalettedImage, std::io::Error> {
    image::ImageBuffer::from_vec(
        TILE_WIDTH as u32,
        TILE_HEIGHT as u32,
        raw.iter().flat_map(|v| vec![v & 0xf, v >> 4]).collect(),
    )
    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "buffer too small"))
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

pub fn apply_palette(paletted: PalettedImage, palette: &Palette) -> image::RgbaImage {
    image::ImageBuffer::from_vec(
        paletted.width(),
        paletted.height(),
        paletted
            .iter()
            .flat_map(|v| {
                if *v > 0 {
                    palette[*v as usize].to_rgba8()
                } else {
                    image::Rgba([0, 0, 0, 0])
                }
                .0
            })
            .collect(),
    )
    .unwrap()
}

pub fn read_merged_tiles(raw: &[u8], cols: usize) -> Result<PalettedImage, std::io::Error> {
    Ok(merge_tiles(
        &raw.chunks(TILE_BYTES).map(read_tile).collect::<Result<Vec<_>, _>>()?,
        cols,
    ))
}

pub fn unlz77(r: &mut impl std::io::Read) -> std::io::Result<Vec<u8>> {
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
    unlz77_cache: std::sync::Mutex<std::collections::HashMap<u32, Vec<u8>>>,
}

impl MemoryMapper {
    pub fn new(rom: Vec<u8>, wram: Vec<u8>) -> Self {
        Self {
            rom,
            wram,
            unlz77_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn get(&self, start: u32) -> std::borrow::Cow<'_, [u8]> {
        #[allow(clippy::manual_range_contains)]
        if start >= 0x02000000 && start < 0x04000000 {
            std::borrow::Cow::Borrowed(&self.wram[(start & !0x02000000) as usize..])
        } else if start >= 0x08000000 && start < 0x0a000000 {
            std::borrow::Cow::Borrowed(&self.rom[(start & !0x08000000) as usize..])
        } else if start >= 0x88000000 && start < 0x8a000000 {
            std::borrow::Cow::Owned(
                self.unlz77_cache
                    .lock()
                    .unwrap()
                    .entry(start)
                    .or_insert_with(|| unlz77(&mut &self.rom[(start & !0x88000000) as usize..]).unwrap()[4..].to_vec())
                    .clone(),
            )
        } else {
            panic!("could not get slice")
        }
    }
}
