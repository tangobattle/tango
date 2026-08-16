pub(crate) mod ex_codes;
mod msg;
pub mod navicust;

use tango_gamesupport_common_dataview::rom::LegalChips;

const WHITE_LEGAL_CHIPS: LegalChips = LegalChips::from_ranges(&[1..=301, 304..=308, 312..=312]);
const BLUE_LEGAL_CHIPS: LegalChips = LegalChips::from_ranges(&[1..=303, 309..=312]);

pub struct Offsets {
    legal_chips: LegalChips,
    chip_data: u32,
    chip_names_pointers: u32,
    chip_descriptions_pointers: u32,
    chip_icon_palette_pointer: u32,
    ncp_data: u32,
    ncp_names_pointer: u32,
    ncp_descriptions_pointer: u32,
    element_icon_palette_pointer: u32,
    element_icons_pointer: u32,
    key_items_names_pointer: u32,
    navicust_bg: image::Rgba<u8>,
}

const NAVICUST_BG_W: image::Rgba<u8> = image::Rgba([0x4a, 0x63, 0x7b, 0xff]);
const NAVICUST_BG_B: image::Rgba<u8> = image::Rgba([0x5a, 0x5a, 0x5a, 0xff]);

#[rustfmt::skip]
pub static A6BJ_01: Offsets = Offsets {
    legal_chips:                    WHITE_LEGAL_CHIPS,
    chip_data:                      0x08011474,
    chip_names_pointers:            0x08027c34,
    chip_descriptions_pointers:     0x0800e3e8,
    chip_icon_palette_pointer:      0x080335ec,
    element_icon_palette_pointer:   0x080335ec,
    element_icons_pointer:          0x080335e0,
    ncp_data:                       0x080398d8,
    ncp_names_pointer:              0x08027c44,
    ncp_descriptions_pointer:       0x0802ef4c,
    key_items_names_pointer:        0x08027c30,

    navicust_bg: NAVICUST_BG_W,
};

#[rustfmt::skip]
pub static A3XJ_01: Offsets = Offsets {
    legal_chips:                    BLUE_LEGAL_CHIPS,
    chip_data:                      0x08011474,
    chip_names_pointers:            0x08027c1c,
    chip_descriptions_pointers:     0x0800e3e8,
    chip_icon_palette_pointer:      0x080335d4,
    element_icon_palette_pointer:   0x080335d4,
    element_icons_pointer:          0x080335c8,
    ncp_data:                       0x080398c0,
    ncp_names_pointer:              0x08027c2c,
    ncp_descriptions_pointer:       0x0802ef34,
    key_items_names_pointer:        0x08027c18,

    navicust_bg: NAVICUST_BG_B,
};

#[rustfmt::skip]
pub static A6BE_00: Offsets = Offsets {
    legal_chips:                    WHITE_LEGAL_CHIPS,
    chip_data:                      0x08011510,
    chip_names_pointers:            0x08027ad4,
    chip_descriptions_pointers:     0x0800e46c,
    chip_icon_palette_pointer:      0x08033134,
    element_icon_palette_pointer:   0x08033134,
    element_icons_pointer:          0x08033128,
    ncp_data:                       0x08039420,
    ncp_names_pointer:              0x08027ae4,
    ncp_descriptions_pointer:       0x0802ea94,
    key_items_names_pointer:        0x08027ad0,

    navicust_bg: NAVICUST_BG_W,
};

#[rustfmt::skip]
pub static A3XE_00: Offsets = Offsets {
    legal_chips:                    BLUE_LEGAL_CHIPS,
    chip_data:                      0x08011510,
    chip_names_pointers:            0x08027abc,
    chip_descriptions_pointers:     0x0800e46c,
    chip_icon_palette_pointer:      0x0803311c,
    element_icon_palette_pointer:   0x0803311c,
    element_icons_pointer:          0x08033110,
    ncp_data:                       0x08039408,
    ncp_names_pointer:              0x08027acc,
    ncp_descriptions_pointer:       0x0802ea7c,
    key_items_names_pointer:        0x08027ab8,

    navicust_bg: NAVICUST_BG_B,
};

pub struct Assets {
    offsets: &'static Offsets,
    msg_parser: msg::Parser,
    mapper: tango_gamesupport_common_dataview::rom::MemoryMapper,
    chip_icon_palette: tango_gamesupport_common_dataview::rom::Palette,
    element_icon_palette: tango_gamesupport_common_dataview::rom::Palette,
}

struct Chip<'a> {
    id: usize,
    assets: &'a Assets,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy, c2rust_bitfields::BitfieldStruct)]
struct RawChip {
    codes: [u8; 6],
    element: u8,
    _family: u8,
    _subfamily: u8,
    _rarity: u8,
    mb: u8,
    _unk_0a: u8,
    attack_power: u16,
    library_number: u16,
    _unk_0e: [u8; 3],

    #[bitfield(name = "giga", ty = "bool", bits = "1..=1")]
    #[bitfield(name = "mega", ty = "bool", bits = "0..=0")]
    flags: [u8; 1],

    icon_ptr: u32,
    image_ptr: u32,
    palette_ptr: u32,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x20);

impl<'a> Chip<'a> {
    fn raw(&'a self) -> RawChip {
        bytemuck::pod_read_unaligned(
            &self.assets.mapper.get(self.assets.offsets.chip_data)[self.id * std::mem::size_of::<RawChip>()..]
                [..std::mem::size_of::<RawChip>()],
        )
    }
}

impl Chip<'_> {
    /// This chip's list icon. `None` when a (patched) ROM's icon
    /// pointer or sheet runs out of mappable range — the trait method
    /// renders those blank instead of panicking.
    fn try_icon(&self) -> Option<image::RgbaImage> {
        let raw = self.raw();
        let tiles = self.assets.mapper.get(raw.icon_ptr);
        let paletted = tango_gamesupport_common_dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common_dataview::rom::TILE_BYTES * 2 * 2)?,
            2,
        )
        .ok()?;
        Some(tango_gamesupport_common_dataview::rom::apply_palette(
            paletted,
            &self.assets.chip_icon_palette,
        ))
    }

    /// This chip's artwork, same fallible-read rule as
    /// [`try_icon`](Self::try_icon).
    fn try_image(&self) -> Option<image::RgbaImage> {
        let raw = self.raw();
        let tiles = self.assets.mapper.get(raw.image_ptr);
        let paletted = tango_gamesupport_common_dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common_dataview::rom::TILE_BYTES * 8 * 7)?,
            8,
        )
        .ok()?;
        let palette_raw = self.assets.mapper.get(raw.palette_ptr);
        let palette = bytemuck::pod_read_unaligned::<tango_gamesupport_common_dataview::rom::Palette>(
            palette_raw.get(..std::mem::size_of::<tango_gamesupport_common_dataview::rom::Palette>())?,
        );
        Some(tango_gamesupport_common_dataview::rom::apply_palette(
            paletted, &palette,
        ))
    }
}

impl<'a> tango_gamesupport_common_dataview::rom::Chip for Chip<'a> {
    fn name(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_names_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common_dataview::msg::get_entry(&region, id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common_dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn description(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_descriptions_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common_dataview::msg::get_entry(&region, id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common_dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn icon(&self) -> image::RgbaImage {
        // A (patched) ROM whose pointers run out of mappable range
        // renders blank instead of panicking.
        self.try_icon().unwrap_or_else(|| {
            image::RgbaImage::new(
                (2 * tango_gamesupport_common_dataview::rom::TILE_WIDTH) as u32,
                (2 * tango_gamesupport_common_dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }

    fn image(&self) -> image::RgbaImage {
        self.try_image().unwrap_or_else(|| {
            image::RgbaImage::new(
                (8 * tango_gamesupport_common_dataview::rom::TILE_WIDTH) as u32,
                (7 * tango_gamesupport_common_dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }

    fn codes(&self) -> Vec<char> {
        let raw = self.raw();
        raw.codes
            .iter()
            .cloned()
            .filter(|code| *code != 0xff)
            .map(|code| b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[code as usize] as char)
            .collect()
    }

    fn element(&self) -> usize {
        let raw = self.raw();
        raw.element as usize
    }

    fn class(&self) -> tango_gamesupport_common_dataview::rom::ChipClass {
        let raw = self.raw();
        if raw.giga() {
            tango_gamesupport_common_dataview::rom::ChipClass::Giga
        } else if raw.mega() {
            tango_gamesupport_common_dataview::rom::ChipClass::Mega
        } else {
            tango_gamesupport_common_dataview::rom::ChipClass::Standard
        }
    }

    fn dark(&self) -> bool {
        false
    }

    fn mb(&self) -> u8 {
        let raw = self.raw();
        raw.mb
    }

    fn attack_power(&self) -> u32 {
        let raw = self.raw();
        if raw.attack_power < 1000 {
            raw.attack_power as u32
        } else {
            0
        }
    }

    fn library_sort_order(&self) -> Option<usize> {
        let raw = self.raw();
        Some(raw.library_number as usize)
    }
}

struct NavicustPart<'a> {
    id: usize,
    assets: &'a Assets,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawNavicustPart {
    _unk_00: u8,
    is_solid: u8,
    _unk_02: u8,
    color: u8,
    effect_group: u8,
    _unk_06: [u8; 3],
    uncompressed_bitmap_ptr: u32,
    compressed_bitmap_ptr: u32,
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x10);

impl<'a> NavicustPart<'a> {
    fn raw(&'a self) -> RawNavicustPart {
        bytemuck::pod_read_unaligned(
            &self.assets.mapper.get(self.assets.offsets.ncp_data)[self.id * std::mem::size_of::<RawNavicustPart>()..]
                [..std::mem::size_of::<RawNavicustPart>()],
        )
    }
}

impl<'a> tango_gamesupport_common_dataview::rom::NavicustPart for NavicustPart<'a> {
    fn name(&self) -> Option<String> {
        let region = &self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.ncp_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common_dataview::msg::get_entry(region, self.id / 4)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common_dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn description(&self) -> Option<String> {
        let region = &self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.ncp_descriptions_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common_dataview::msg::get_entry(region, self.id / 4)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common_dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn color(&self) -> Option<tango_gamesupport_common_dataview::rom::NavicustPartColor> {
        let raw = self.raw();
        Some(match raw.color {
            1 => tango_gamesupport_common_dataview::rom::NavicustPartColor::White,
            2 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Pink,
            3 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Yellow,
            4 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Red,
            5 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Blue,
            6 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Green,
            7 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Orange,
            8 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Purple,
            9 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Gray,
            _ => {
                return None;
            }
        })
    }

    fn is_solid(&self) -> bool {
        let raw = self.raw();
        raw.is_solid == 0
    }

    fn uncompressed_bitmap(&self) -> tango_gamesupport_common_dataview::rom::NavicustBitmap {
        let raw = self.raw();
        ndarray::Array2::from_shape_vec(
            (5, 5),
            self.assets.mapper.get(raw.uncompressed_bitmap_ptr)[..25]
                .iter()
                .map(|x| *x != 0)
                .collect(),
        )
        .unwrap()
    }

    fn compressed_bitmap(&self) -> Option<tango_gamesupport_common_dataview::rom::NavicustBitmap> {
        let raw = self.raw();
        Some(
            ndarray::Array2::from_shape_vec(
                (5, 5),
                self.assets.mapper.get(raw.compressed_bitmap_ptr)[..25]
                    .iter()
                    .map(|x| *x != 0)
                    .collect(),
            )
            .unwrap(),
        )
    }
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[&str], rom: Vec<u8>, wram: Vec<u8>) -> Self {
        let mapper = tango_gamesupport_common_dataview::rom::MemoryMapper::new(rom, wram);
        let chip_icon_palette = bytemuck::pod_read_unaligned::<tango_gamesupport_common_dataview::rom::Palette>(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.chip_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..std::mem::size_of::<tango_gamesupport_common_dataview::rom::Palette>()],
        );
        let element_icon_palette = bytemuck::pod_read_unaligned::<tango_gamesupport_common_dataview::rom::Palette>(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.element_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..std::mem::size_of::<tango_gamesupport_common_dataview::rom::Palette>()],
        );

        Self {
            offsets,
            msg_parser: msg::parser(charset),
            mapper,
            chip_icon_palette,
            element_icon_palette,
        }
    }
}

#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy, c2rust_bitfields::BitfieldStruct)]
struct RawStyle {
    #[bitfield(name = "element", ty = "u8", bits = "0..=2")]
    #[bitfield(name = "typ", ty = "u8", bits = "3..=7")]
    type_and_element: [u8; 1],
}

/// The style *type* half of a packed style id. BN3's style system is
/// this crate's own model — the only piece the shared traits carry is
/// the equipped style's display name (`Assets::style_name`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StyleType {
    Normal,
    Guts,
    Custom,
    Team,
    Shield,
    Ground,
    Shadow,
    Bug,
}

/// Decode a packed style id's type bits.
pub(crate) fn style_type(id: u8) -> StyleType {
    let raw = bytemuck::cast::<_, RawStyle>(id);
    match raw.typ() {
        0 => StyleType::Normal,
        1 => StyleType::Guts,
        2 => StyleType::Custom,
        3 => StyleType::Team,
        4 => StyleType::Shield,
        5 => StyleType::Ground,
        6 => StyleType::Shadow,
        7 => StyleType::Bug,
        _ => StyleType::Normal,
    }
}

pub(super) fn extra_ncp_color(id: u8) -> Option<tango_gamesupport_common_dataview::rom::NavicustPartColor> {
    let raw = bytemuck::cast::<_, RawStyle>(id);
    Some(match raw.typ() {
        1 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Red,
        2 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Blue,
        3 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Green,
        4 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Blue,
        5 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Green,
        6 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Red,
        7 => tango_gamesupport_common_dataview::rom::NavicustPartColor::Gray,
        _ => {
            return None;
        }
    })
}

impl tango_gamesupport_common_dataview::rom::Assets for Assets {
    fn chip_is_legal(&self, id: usize) -> bool {
        self.offsets.legal_chips.contains(id)
    }

    fn chip<'a>(&'a self, id: usize) -> Option<Box<dyn tango_gamesupport_common_dataview::rom::Chip + 'a>> {
        if id >= self.num_chips() {
            return None;
        }
        Some(Box::new(Chip { id, assets: self }))
    }

    fn num_chips(&self) -> usize {
        super::NUM_CHIPS
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= 5 {
            return None;
        }

        let buf = self.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            self.mapper
                .get(self.offsets.element_icons_pointer)
                .get(..std::mem::size_of::<u32>())?,
        ));
        let buf = buf.get(0x1e0..)?;
        let paletted = tango_gamesupport_common_dataview::rom::read_merged_tiles(
            buf.get(id * tango_gamesupport_common_dataview::rom::TILE_BYTES * 4..)?
                .get(..tango_gamesupport_common_dataview::rom::TILE_BYTES * 2 * 2)?,
            2,
        )
        .ok()?;
        Some(tango_gamesupport_common_dataview::rom::apply_palette(
            paletted,
            &self.element_icon_palette,
        ))
    }

    fn navicust_part(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common_dataview::rom::NavicustPart + '_>> {
        if id >= self.num_navicust_parts() {
            return None;
        }
        Some(Box::new(NavicustPart { id, assets: self }))
    }

    fn num_navicust_parts(&self) -> usize {
        super::NUM_NAVICUST_PARTS
    }

    fn style_name(&self, id: usize) -> Option<String> {
        if id >= super::NUM_STYLES {
            return None;
        }
        let raw = bytemuck::cast::<_, RawStyle>(id as u8);

        let region = &self.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.mapper.get(self.offsets.key_items_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = tango_gamesupport_common_dataview::msg::get_entry(
            region,
            128 + raw.typ() as usize * 5 + raw.element() as usize,
        )?;

        Some(
            self.msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common_dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn navicust_layout(&self) -> Option<tango_gamesupport_common_dataview::rom::NavicustLayout> {
        Some(tango_gamesupport_common_dataview::rom::NavicustLayout {
            command_line: 2,
            has_out_of_bounds: false,
            background: self.offsets.navicust_bg,
        })
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "-", "×", "=", ":", "+", "÷", "※", "*", "!", "?", "%", "&", ",", "⋯", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "@", "♥", "♪", "[MB]", "■", "_", "[circle1]", "[circle2]", "[cross1]", "[cross2]", "[bracket1]", "[bracket2]", "[ModTools1]", "[ModTools2]", "[ModTools3]", "Σ", "Ω", "α", "β", "#", "…", ">", "<", "エ", "[BowneGlobal1]", "[BowneGlobal2]", "[BowneGlobal3]", "[BowneGlobal4]", "[BowneGlobal5]", "[BowneGlobal6]", "[BowneGlobal7]", "[BowneGlobal8]", "[BowneGlobal9]", "[BowneGlobal10]", "[BowneGlobal11]"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "Σ", "Ω", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "ー", "×", "=", ":", "?", "+", "÷", "※", "*", "!", "[?]", "%", "&", "、", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "@", "♥", "♪", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "ヰ", "ヱ", "[MB]", "■", "_", "[circle1]", "[circle2]", "[cross1]", "[cross2]", "[bracket1]", "[bracket2]", "[ModTools1]", "[ModTools2]", "[ModTools3]", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "止", "彩", "起", "父", "博", "士", "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "磁", "真", "人", "入", "出", "山", "口", "光", "電", "気", "話", "広", "王", "名", "前", "学", "校", "渡", "職", "室", "世", "界", "員", "管", "理", "局", "島", "機", "器", "大", "小", "中", "自", "分", "間", "村", "感", "問", "異", "門", "熱", "斗", "要", "常", "道", "行", "街", "屋", "水", "見", "終", "教", "走", "先", "生", "長", "今", "了", "点", "女", "子", "言", "会", "来", "風", "吹", "速", "思", "時", "円", "知", "毎", "年", "火", "朝", "計", "画", "休", "体", "波", "回", "外", "多", "病", "正", "死", "値", "合", "戦", "争", "秋", "原", "町", "天", "用", "金", "男", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "砂", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "岩", "才", "者", "立", "泉", "々", "ヶ", "連", "射", "国", "綾", "耳", "土", "炎", "伊", "集", "院", "各", "科", "省", "祐", "朗", "枚", "路", "川", "花", "兄", "帯", "音", "属", "性", "持", "勝", "赤", "犬", "飼", "荒", "丁", "駒", "地", "所", "明", "切", "急", "木", "無", "高", "駅", "店", "不", "研", "究"];
