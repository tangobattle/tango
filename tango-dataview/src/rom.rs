use byteorder::ReadBytesExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExCodeEffect {
    MaxHP(u16),
    SuperArmor,
    BreakBuster,
    BreakCharge,
    ShadowShoes,
    FloatShoes,
    AirShoes,
    UnderShirt,
    Block,
    Shield,
    Reflect,
    AntiDamage,
    MegaFolder(u8),
    GigaFolder(u8),
    FastGauge,
    SneakRun,
    Humor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExCodeBug {
    Custom(u8),
    PoisonPanelStep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExCode {
    pub code: u8,
    pub effect: ExCodeEffect,
    pub bug: Option<ExCodeBug>,
}

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

/// A BN4 patch-card (modcard) effect, reverse-engineered from the game's
/// effect jump table at `0x8041e8c`: each modcard id dispatches to a small
/// handler that calls `set_effect(id, param)` (`0x800d78a`). The variant
/// names the effect; a numeric payload is the raw game parameter (a chip /
/// soul / panel index, or an amount), so it stays faithful to the binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchCard4Effect {
    /// No in-battle effect — the cosmetic PET-menu recolor cards.
    None,
    /// Max HP increased by this many points (direct `maxHP += N`).
    MaxHP(u16),
    /// Buster attack power set to this level (`set_effect 0x05`).
    BusterAttack(u8),
    /// B Button shortcut / normal-shot modifier (`set_effect 0x09`).
    BButton(PatchCard4Shot),
    /// B + charge fires this chip (`set_effect 0x0a`).
    BCharge(PatchCard4Shot),
    /// B + ← fires this chip (`set_effect 0x0c`).
    BLeft(PatchCard4Shot),
    /// +N Custom-screen slots, capped at 8 (`set_effect 0x12`).
    CustomSlots(u8),
    /// +N Mega-chip folder limit, capped at 10 (`set_effect 0x13`).
    MegaFolder(u8),
    /// +N Giga-chip folder limit, capped at 10 (`set_effect 0x14`).
    GigaFolder(u8),
    /// Triple Supporter — needs both the 1/2 and 2/2 cards (`set_effect 0x18`).
    TripleSupporter,
    /// Stepping off a panel leaves this terrain (`set_effect 0x1b`).
    PanelStep(PatchCard4Panel),
    /// Start the battle in Full Synchro (`set_effect 0x1f`).
    FullSynchro,
    /// Start the battle with this aura / barrier (`set_effect 0x21`).
    Aura(PatchCard4Aura),
    /// Start the battle in this Soul Unison (`set_effect 0x24`).
    Soul(PatchCard4Soul),
    /// MegaMan recolor (`set_effect 0x27`).
    Color(PatchCard4Color),
    /// All Guard — needs both the 1/2 and 2/2 cards (`set_effect 0x28`).
    AllGuard,
    /// Recolors the overworld PET menu — a non-battle cosmetic, so the
    /// modcard's battle handler is empty (no `set_effect`); the colour is
    /// taken from the card.
    PetMenu(PatchCard4PetColor),
}

/// Terrain laid by a [`PatchCard4Effect::PanelStep`] card (`set_effect 0x1b`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchCard4Panel {
    Broken,  // 1
    Cracked, // 3
    Metal,   // 5
    Holy,    // 9
}

/// Battle-start aura from a [`PatchCard4Effect::Aura`] card (`set_effect 0x21`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchCard4Aura {
    Barrier100, // 2
    Barrier200, // 3
    LifeAura,   // 6
}

/// Soul Unison from a [`PatchCard4Effect::Soul`] card (`set_effect 0x24`, 1-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchCard4Soul {
    Roll,
    Guts,
    Wind,
    Search,
    Fire,
    Thunder,
    Proto,
    Number,
    Metal,
    Junk,
    Aqua,
    Wood,
}

/// MegaMan recolor from a [`PatchCard4Effect::Color`] card (`set_effect 0x27`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchCard4Color {
    Red,    // 1
    Yellow, // 2
    White,  // 3
    Green,  // 4
}

/// PET-menu colour from a [`PatchCard4Effect::PetMenu`] card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchCard4PetColor {
    Blue,
    Pink,
    Green,
    Black,
}

/// The chip/shot a [`PatchCard4Effect::BButton`]/[`BCharge`](PatchCard4Effect::BCharge)/
/// [`BLeft`](PatchCard4Effect::BLeft) card assigns. The discriminant is the
/// raw game param the modcard's `set_effect` writes (so `as u16` recovers it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum PatchCard4Shot {
    ZapRing = 4,
    WaterGun = 32,
    Flower = 33,
    Reflect = 38,
    GutsMachineGun = 40,
    Cannon = 41,
    MiniBomb = 42,
    HeatShot = 43,
    Bubbler = 44,
    Thunder1 = 45,
    Sword = 46,
    Spreader = 47,
    RandomTrapChip = 49,
    CrackedPanel = 52,
    PoisonPanel = 53,
    Crackout = 54,
    CopyDamage = 55,
    WideShot1 = 56,
    Thunder2 = 58,
    DoubleCrack = 60,
    WideSword = 62,
    Recov10 = 64,
    Lance = 65,
    Hole = 66,
    WideShot2 = 67,
    SandRing = 68,
    EnergyBomb = 69,
    Thunder3 = 70,
    TripleCrack = 72,
    LongSword = 73,
    FullCustom = 75,
    WideShot3 = 76,
    WindRack = 78,
    MegaEnergyBomb = 79,
    Ball = 80,
    BugBomb = 81,
    WideBlade = 82,
    LongBlade = 83,
    NorthWind = 84,
    PanelReturn = 85,
    Blind = 90,
    Blizzard = 91,
    HeatBreath = 92,
    WoodyPowder = 93,
    Repair = 94,
    AirShot = 95,
    ElecShock = 96,
    Guard1 = 98,
    GrassPanel = 104,
    IcePanel = 105,
}

/// A BN4 patch-card drawback ("bug"), from `set_bug(category, param)`
/// (`0x80476e0`). A card can carry more than one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchCard4Bug {
    /// Start the battle Confused (`set_bug 0x01, 1`).
    Confused,
    /// MegaMan auto-moves forward (`set_bug 0x01, 2`).
    AutoMove,
    /// HP drains during the battle (`set_bug 0x02, n`) — `n` is the severity.
    HP(u8),
    /// Custom gauge HP bug (`set_bug 0x03, 1`).
    CustomHP,
    /// Custom gauge −1 (`set_bug 0x03, 2`).
    CustomMinus1,
    /// Poison panels appear as you step (`set_bug 0x04, 3`).
    PoisonPanelStep,
}

pub trait PatchCard4 {
    fn name(&self) -> Option<String>;
    fn slot(&self) -> u8;
    fn effect(&self) -> PatchCard4Effect;
    fn bugs(&self) -> &[PatchCard4Bug];
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

pub trait Style {
    fn name(&self) -> Option<String>;
    fn typ(&self) -> StyleType;
    fn element(&self) -> usize;
    fn extra_ncp_color(&self) -> Option<NavicustPartColor>;
}

#[derive(Clone, Copy)]
pub enum StyleType {
    Normal,
    Guts,
    Custom,
    Team,
    Shield,
    Ground,
    Shadow,
    Bug,
    Hub,
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
}

pub struct NavicustLayout {
    pub command_line: usize,
    pub has_out_of_bounds: bool,
    pub background: image::Rgba<u8>,
}

pub trait Assets {
    fn chip(&self, id: usize) -> Option<Box<dyn Chip + '_>>;
    fn num_chips(&self) -> usize;
    fn can_set_regular_chip(&self) -> bool {
        false
    }
    fn can_set_tag_chips(&self) -> bool {
        false
    }
    fn regular_chip_is_in_place(&self) -> bool {
        false
    }
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
    fn patch_card4(&self, id: usize) -> Option<Box<dyn PatchCard4 + '_>> {
        let _ = id;
        None
    }
    fn num_patch_card4s(&self) -> usize {
        0
    }
    fn navicust_part(&self, id: usize) -> Option<Box<dyn NavicustPart + '_>> {
        let _ = id;
        None
    }
    fn num_navicust_parts(&self) -> usize {
        0
    }
    fn style(&self, id: usize) -> Option<Box<dyn Style + '_>> {
        let _ = id;
        None
    }
    fn num_styles(&self) -> usize {
        0
    }
    fn navi(&self, id: usize) -> Option<Box<dyn Navi + '_>> {
        let _ = id;
        None
    }
    fn num_navis(&self) -> usize {
        0
    }
    fn navicust_layout(&self) -> Option<NavicustLayout> {
        None
    }
    fn ex_code(&self, _code: u8) -> Option<ExCode> {
        None
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
    /// Expand to RGB888 (`c * 255 / 31` per channel). Routed through the shared
    /// 32768-entry table that also backs [`bgr555_to_rgba8`], so the per-color
    /// and whole-buffer conversions can never diverge.
    pub fn to_rgba8(&self) -> image::Rgba<u8> {
        let idx = self.r() as usize | (self.g() as usize) << 5 | (self.b() as usize) << 10;
        BGR555_RGBA8_LUT[idx]
    }
}

/// Canonical BGR555 → RGBA8 expansion, indexed by the 15-bit value
/// (`r | g << 5 | b << 10`): each channel scaled `c * 255 / 31`, alpha opaque.
///
/// The single source of truth for the conversion — both [`Bgr555::to_rgb888`]
/// and [`bgr555_to_rgba8`] read from this table rather than recomputing it.
static BGR555_RGBA8_LUT: [image::Rgba<u8>; 0x8000] = {
    const fn expand(c: u16) -> u8 {
        (c * 0xff / 0x1f) as u8
    }

    let mut arr = [image::Rgba([0, 0, 0, 0]); 0x8000];
    let mut i = 0u16;
    while i < 0x8000 {
        arr[i as usize] = image::Rgba([
            expand(i & 0x1f),
            expand((i >> 5) & 0x1f),
            expand((i >> 10) & 0x1f),
            0xff,
        ]);
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
/// shared table [`Bgr555::to_rgb888`] uses to render ROM sprites and palettes,
/// so emulated frames and in-app ROM imagery share identical colors, at one
/// lookup per pixel. Alpha is forced opaque.
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

pub type Palette = [Bgr555; 16];

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
