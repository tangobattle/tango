use crate::{msg, rom};

const PRINT_VAR_COMMAND: &[u8] = b"\xfa\x03";
const EREADER_DESCRIPTION_COMMAND: &[u8] = b"\xff\x01";

pub struct Offsets {
    chip_data: u32,
    chip_names_pointers: u32,
    chip_descriptions_pointers: u32,
    chip_icon_palette_pointer: u32,
    ncp_data: u32,
    ncp_names_pointer: u32,
    ncp_descriptions_pointer: u32,
    element_icon_palette_pointer: u32,
    element_icons_pointer: u32,
    patch_card_data: u32,
    patch_card_names_pointer: u32,
    patch_card_details_names_pointer: u32,
    navicust_bg: image::Rgba<u8>,
}

const NAVICUST_BG_G: image::Rgba<u8> = image::Rgba([0x08, 0xbd, 0x73, 0xff]);
const NAVICUST_BG_F: image::Rgba<u8> = image::Rgba([0xe7, 0x8c, 0x39, 0xff]);

#[rustfmt::skip]
pub static BR5J_00: Offsets = Offsets {
    chip_data:                          0x080221bc,
    chip_names_pointers:                0x08043274,
    chip_descriptions_pointers:         0x08028164,
    chip_icon_palette_pointer:          0x0801f144,
    ncp_data:                           0x081460cc,
    ncp_names_pointer:                  0x08043284,
    ncp_descriptions_pointer:           0x08139240,
    element_icon_palette_pointer:       0x081226e4,
    element_icons_pointer:              0x081226dc,
    patch_card_data:                    0x08144778,
    patch_card_names_pointer:           0x08130fe0,
    patch_card_details_names_pointer:   0x08130fec,

    navicust_bg: NAVICUST_BG_G,
};

#[rustfmt::skip]
pub static BR6J_00: Offsets = Offsets {
    chip_data:                          0x080221bc,
    chip_names_pointers:                0x080432a4,
    chip_descriptions_pointers:         0x08028164,
    chip_icon_palette_pointer:          0x0801f144,
    ncp_data:                           0x08144300,
    ncp_names_pointer:                  0x080432b4,
    ncp_descriptions_pointer:           0x08137478,
    element_icon_palette_pointer:       0x081213c4,
    element_icons_pointer:              0x081213bc,
    patch_card_data:                    0x081429b0,
    patch_card_names_pointer:           0x0812f218,
    patch_card_details_names_pointer:   0x0812f224,

    navicust_bg: NAVICUST_BG_F,
};

#[rustfmt::skip]
pub static BR5E_00: Offsets = Offsets {
    chip_data:                          0x08021da8,
    chip_names_pointers:                0x08042038,
    chip_descriptions_pointers:         0x08027d50,
    chip_icon_palette_pointer:          0x0801ed20,
    ncp_data:                           0x0813b22c,
    ncp_names_pointer:                  0x08042048,
    ncp_descriptions_pointer:           0x08130878,
    element_icon_palette_pointer:       0x0811a9a4,
    element_icons_pointer:              0x0811a99c,
    patch_card_data:                    0,
    patch_card_names_pointer:           0,
    patch_card_details_names_pointer:   0,

    navicust_bg: NAVICUST_BG_G,
};

#[rustfmt::skip]
pub static BR6E_00: Offsets = Offsets {
    chip_data:                          0x08021da8,
    chip_names_pointers:                0x08042068,
    chip_descriptions_pointers:         0x08027d50,
    chip_icon_palette_pointer:          0x0801ed20,
    ncp_data:                           0x0813944c,
    ncp_names_pointer:                  0x08042078,
    ncp_descriptions_pointer:           0x0812ea9c,
    element_icon_palette_pointer:       0x08119674,
    element_icons_pointer:              0x0811966c,
    patch_card_data:                    0,
    patch_card_names_pointer:           0,
    patch_card_details_names_pointer:   0,

    navicust_bg: NAVICUST_BG_F,
};

pub struct Assets {
    offsets: &'static Offsets,
    msg_parser: msg::Parser,
    mapper: rom::MemoryMapper,
    chip_icon_palette: [image::Rgba<u8>; 16],
    element_icon_palette: [image::Rgba<u8>; 16],
}

struct Chip<'a> {
    id: usize,
    assets: &'a Assets,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawChip {
    codes: [u8; 4],
    _attack_element: u8,
    _rarity: u8,
    element: u8,
    class: u8,
    mb: u8,
    effect_flags: [u8; 1],
    _counter_settings: u8,
    _attack_family: u8,
    _attack_subfamily: u8,
    _dark_soul_usage_behavior: u8,
    _unk_0e: u8,
    _lock_on: u8,
    _attack_params: [u8; 4],
    _delay: u8,
    _library_number: u8,
    _library_flags: [u8; 1],
    _lock_on_type: u8,
    _alphabet_sort: u16,
    attack_power: u16,
    library_sort_order: u16,
    _battle_chip_gate_usage: u8,
    _dark_chip_id: u8,
    icon_ptr: u32,
    image_ptr: u32,
    palette_ptr: u32,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x2c);

impl<'a> Chip<'a> {
    fn raw(&'a self) -> RawChip {
        bytemuck::pod_read_unaligned(
            &self.assets.mapper.get(self.assets.offsets.chip_data)[self.id * std::mem::size_of::<RawChip>()..]
                [..std::mem::size_of::<RawChip>()],
        )
    }
}

impl<'a> rom::Chip for Chip<'a> {
    fn name(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_names_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = msg::get_entry(&region, id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        msg::Chunk::Text(s) => s,
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
        let entry = msg::get_entry(&region, id)?;

        self.assets
            .msg_parser
            .parse(entry)
            .ok()?
            .into_iter()
            .map(|part| {
                Some(match part {
                    msg::Chunk::Text(s) => s,
                    msg::Chunk::Command { op, params } if op == EREADER_DESCRIPTION_COMMAND => {
                        if let Ok(parts) = self.assets.msg_parser.parse(&self.assets.mapper.get(
                            (super::save::EREADER_DESCRIPTION_OFFSET
                                + params[0] as usize * super::save::EREADER_DESCRIPTION_SIZE)
                                as u32
                                | 0x02000000,
                        )) {
                            parts
                                .into_iter()
                                .flat_map(|part| {
                                    match part {
                                        msg::Chunk::Text(s) => s,
                                        _ => "".to_string(),
                                    }
                                    .chars()
                                    .collect::<Vec<_>>()
                                })
                                .collect::<String>()
                        } else {
                            return None;
                        }
                    }
                    _ => "".to_string(),
                })
            })
            .collect::<Option<String>>()
    }

    fn icon(&self) -> image::RgbaImage {
        let raw = self.raw();
        rom::apply_palette(
            rom::read_merged_tiles(&self.assets.mapper.get(raw.icon_ptr)[..rom::TILE_BYTES * 4], 2).unwrap(),
            &self.assets.chip_icon_palette,
        )
    }

    fn image(&self) -> image::RgbaImage {
        let raw = self.raw();
        rom::apply_palette(
            rom::read_merged_tiles(&self.assets.mapper.get(raw.image_ptr)[..rom::TILE_BYTES * 7 * 6], 7).unwrap(),
            &rom::read_palette(&self.assets.mapper.get(raw.palette_ptr)[..32]),
        )
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

    fn class(&self) -> rom::ChipClass {
        let raw = self.raw();
        match raw.class {
            0 => rom::ChipClass::Standard,
            1 => rom::ChipClass::Mega,
            2 => rom::ChipClass::Giga,
            4 => rom::ChipClass::ProgramAdvance,
            _ => rom::ChipClass::None,
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
        Some(raw.library_sort_order as usize)
    }
}

struct NavicustPart<'a> {
    id: usize,
    variant: usize,
    assets: &'a Assets,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawNavicustPart {
    _unk_00: u8,
    is_solid: u8,
    _unk_02: u8,
    color: u8,
    _unk_05: [u8; 4],
    uncompressed_bitmap_ptr: u32,
    compressed_bitmap_ptr: u32,
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x10);

impl<'a> NavicustPart<'a> {
    fn raw(&'a self) -> RawNavicustPart {
        let i = self.id * 4 + self.variant;
        bytemuck::pod_read_unaligned(
            &self.assets.mapper.get(self.assets.offsets.ncp_data)[i * std::mem::size_of::<RawNavicustPart>()..]
                [..std::mem::size_of::<RawNavicustPart>()],
        )
    }
}

impl<'a> rom::NavicustPart for NavicustPart<'a> {
    fn name(&self) -> Option<String> {
        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.ncp_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = msg::get_entry(&region, self.id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match &part {
                        msg::Chunk::Text(s) => s,
                        _ => "",
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn description(&self) -> Option<String> {
        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.ncp_descriptions_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = msg::get_entry(&region, self.id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn color(&self) -> Option<rom::NavicustPartColor> {
        let raw = self.raw();
        Some(match raw.color {
            1 => rom::NavicustPartColor::White,
            2 => rom::NavicustPartColor::Yellow,
            3 => rom::NavicustPartColor::Pink,
            4 => rom::NavicustPartColor::Red,
            5 => rom::NavicustPartColor::Blue,
            6 => rom::NavicustPartColor::Green,
            _ => {
                return None;
            }
        })
    }

    fn is_solid(&self) -> bool {
        let raw = self.raw();
        raw.is_solid == 0
    }

    fn uncompressed_bitmap(&self) -> rom::NavicustBitmap {
        let raw = self.raw();
        ndarray::Array2::from_shape_vec(
            (7, 7),
            self.assets.mapper.get(raw.uncompressed_bitmap_ptr)[..49]
                .iter()
                .map(|x| *x != 0)
                .collect(),
        )
        .unwrap()
    }

    fn compressed_bitmap(&self) -> rom::NavicustBitmap {
        let raw = self.raw();
        ndarray::Array2::from_shape_vec(
            (7, 7),
            self.assets.mapper.get(raw.compressed_bitmap_ptr)[..49]
                .iter()
                .map(|x| *x != 0)
                .collect(),
        )
        .unwrap()
    }
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[String], rom: Vec<u8>, wram: Vec<u8>) -> Self {
        let mapper = rom::MemoryMapper::new(rom, wram);

        let chip_icon_palette = rom::read_palette(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.chip_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..32],
        );

        let element_icon_palette = rom::read_palette(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.element_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..32],
        );

        Self {
            offsets,
            msg_parser: msg::Parser::builder()
                .with_ignore_unknown(true)
                .add_eof_rule(b"\xe6")
                .add_charset_rules(charset, 0xe4)
                .add_text_rule(b"\xe9", "\n")
                .add_command_rule(PRINT_VAR_COMMAND, 2)
                .add_command_rule(EREADER_DESCRIPTION_COMMAND, 1)
                .add_command_rule(b"\xe7", 1)
                .add_command_rule(b"\xe8\x01", 0)
                .add_command_rule(b"\xe8\x02", 0)
                .add_command_rule(b"\xe8\x03", 0)
                .add_command_rule(b"\xe8\x04", 2)
                .add_command_rule(b"\xe8\x05", 2)
                .add_command_rule(b"\xe8\x06", 2)
                .add_command_rule(b"\xee\x00", 2)
                .add_command_rule(b"\xf1\x00", 1)
                .build(),
            mapper,
            chip_icon_palette,
            element_icon_palette,
        }
    }
}

struct PatchCard56<'a> {
    id: usize,
    assets: &'a Assets,
}

impl<'a> PatchCard56<'a> {
    pub fn raw_header(&self) -> RawPatchCard56Header {
        let buf = self.assets.mapper.get(self.assets.offsets.patch_card_data);
        let [offset, next_offset] =
            bytemuck::pod_read_unaligned::<[u16; 2]>(&buf[self.id * 2..][..std::mem::size_of::<u32>()]);
        let buf = &buf[offset as usize..next_offset as usize];

        bytemuck::pod_read_unaligned(&buf[0..][..std::mem::size_of::<RawPatchCard56Header>()])
    }

    pub fn raw_effects(&self) -> Vec<RawPatchCard56Effect> {
        let buf = self.assets.mapper.get(self.assets.offsets.patch_card_data);
        let [offset, next_offset] =
            bytemuck::pod_read_unaligned::<[u16; 2]>(&buf[self.id * 2..][..std::mem::size_of::<u32>()]);
        let buf = &buf[offset as usize..next_offset as usize];

        buf[std::mem::size_of::<RawPatchCard56Header>()..]
            .chunks(std::mem::size_of::<RawPatchCard56Effect>())
            .into_iter()
            .map(|chunk| bytemuck::pod_read_unaligned(chunk))
            .collect()
    }
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawPatchCard56Header {
    _unk_00: u8,
    mb: u8,
    _unused: u8,
}
const _: () = assert!(std::mem::size_of::<RawPatchCard56Header>() == 0x3);

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawPatchCard56Effect {
    id: u8,
    parameter: u8,
    is_debuff: u8,
}
const _: () = assert!(std::mem::size_of::<RawPatchCard56Effect>() == 0x3);

impl<'a> rom::PatchCard56 for PatchCard56<'a> {
    fn name(&self) -> Option<String> {
        if self.id == 0 {
            return Some("".to_string());
        }

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.patch_card_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = msg::get_entry(&region, self.id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn mb(&self) -> u8 {
        if self.id == 0 {
            return 0;
        }

        let header = self.raw_header();
        header.mb
    }

    fn effects(&self) -> Vec<crate::rom::PatchCard56Effect> {
        if self.id == 0 {
            return vec![];
        }

        let effects = self.raw_effects();
        effects
            .into_iter()
            .map(|effect| crate::rom::PatchCard56Effect {
                id: effect.id as usize,
                name: {
                    let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
                        &self
                            .assets
                            .mapper
                            .get(self.assets.offsets.patch_card_details_names_pointer)[..std::mem::size_of::<u32>()],
                    ));
                    msg::get_entry(&region, effect.id as usize)
                        .and_then(|entry| self.assets.msg_parser.parse(entry).ok())
                        .and_then(|chunks| {
                            chunks
                                .into_iter()
                                .map(|chunk| match chunk {
                                    msg::Chunk::Text(s) => Some(crate::rom::PatchCard56EffectTemplatePart::String(s)),
                                    msg::Chunk::Command { op, params } if op == PRINT_VAR_COMMAND => {
                                        Some(crate::rom::PatchCard56EffectTemplatePart::PrintVar(params[1] as usize))
                                    }
                                    _ => None,
                                })
                                .collect::<Option<Vec<_>>>()
                        })
                        .map(|parts| {
                            parts
                                .into_iter()
                                .flat_map(|p| {
                                    match p {
                                        crate::rom::PatchCard56EffectTemplatePart::String(s) => s,
                                        crate::rom::PatchCard56EffectTemplatePart::PrintVar(v) => {
                                            if v == 1 {
                                                let mut parameter = effect.parameter as u32;
                                                if effect.id == 0x00 || effect.id == 0x02 {
                                                    parameter = parameter * 10;
                                                }
                                                format!("{}", parameter)
                                            } else {
                                                "".to_string()
                                            }
                                        }
                                    }
                                    .chars()
                                    .collect::<Vec<_>>()
                                })
                                .collect()
                        })
                },
                parameter: effect.parameter,
                is_debuff: effect.is_debuff == 1,
                is_ability: effect.id > 0x15,
            })
            .collect::<Vec<_>>()
    }
}

impl rom::Assets for Assets {
    fn chip<'a>(&'a self, id: usize) -> Option<Box<dyn rom::Chip + 'a>> {
        if id >= self.num_chips() {
            return None;
        }
        Some(Box::new(Chip { id, assets: self }))
    }

    fn num_chips(&self) -> usize {
        super::NUM_CHIPS
    }

    fn can_set_regular_chip(&self) -> bool {
        true
    }

    fn can_set_tag_chips(&self) -> bool {
        true
    }

    fn regular_chip_is_in_place(&self) -> bool {
        true
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= 11 {
            return None;
        }

        let buf = self.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.mapper.get(self.offsets.element_icons_pointer)[..std::mem::size_of::<u32>()],
        ));
        Some(rom::apply_palette(
            rom::read_merged_tiles(&buf[id * rom::TILE_BYTES * 4..][..rom::TILE_BYTES * 4], 2).unwrap(),
            &self.element_icon_palette,
        ))
    }

    fn navicust_part<'a>(&'a self, id: usize, variant: usize) -> Option<Box<dyn rom::NavicustPart + 'a>> {
        let (max_id, max_variant) = self.num_navicust_parts();
        if id >= max_id || variant >= max_variant {
            return None;
        }
        Some(Box::new(NavicustPart {
            id,
            variant,
            assets: self,
        }))
    }

    fn num_navicust_parts(&self) -> (usize, usize) {
        super::NUM_NAVICUST_PARTS
    }

    fn patch_card56<'a>(&'a self, id: usize) -> Option<Box<dyn rom::PatchCard56 + 'a>> {
        if id >= self.num_patch_card56s() {
            return None;
        }
        Some(Box::new(PatchCard56 { id, assets: self }))
    }

    fn num_patch_card56s(&self) -> usize {
        super::NUM_PATCH_CARD56S
    }

    fn navicust_layout(&self) -> Option<rom::NavicustLayout> {
        Some(rom::NavicustLayout {
            command_line: 3,
            has_out_of_bounds: true,
            background: self.offsets.navicust_bg,
        })
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "RV", "BX", "EX", "SP", "FZ", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "-", "×", "=", ":", "%", "?", "+", "█", "[bat]", "ー", "!", "&", ",", "゜", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "�", "_", "ƶ", "[L]", "[B]", "[R]", "[A]", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "[END]", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "容", "量", "全", "木", "[MB]", "無", "現", "実", "[circle]", "×", "緑", "道", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "来", "日", "目", "月", "獣", "各", "人", "入", "出", "山", "口", "光", "電", "気", "綾", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "高", "朗", "枚", "野", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", "究", "門", "城", "王", "兄", "化", "葉", "行", "街", "屋", "水", "見", "終", "新", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "代", "年", "火", "改", "計", "画", "職", "体", "波", "回", "外", "地", "員", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "晴", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "教", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "向", "犬", "々", "ヶ", "連", "射", "舟", "戸", "切", "土", "炎", "伊", "夫", "鉄", "国", "男", "天", "老", "師", "堀", "杉", "士", "悟", "森", "霧", "麻", "剛", "垣", "★", "[bracket1]", "[bracket2]", "[.]"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "-", "×", "=", ":", "%", "?", "+", "■", "[bat]", "ー", "!", "RV", "BX", "&", "、", "゜", ".", "・", ";", "’", "\"", "~", "/", "(", ")", "「", "」", "EX", "SP", "FZ", "�", "_", "ƶ", "周", "え", "お", "う", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "[END]", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "全", "木", "[MB]", "無", "現", "実", "[circle]", "×", "緑", "道", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "来", "日", "目", "月", "獣", "各", "人", "入", "出", "山", "口", "光", "電", "気", "綾", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "高", "朗", "枚", "野", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", "究", "門", "城", "王", "兄", "化", "葉", "行", "街", "屋", "水", "見", "終", "新", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "代", "年", "火", "改", "計", "画", "職", "体", "波", "回", "外", "地", "員", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "晴", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "教", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "向", "犬", "々", "ヶ", "連", "射", "舟", "戸", "切", "土", "炎", "伊", "夫", "鉄", "国", "男", "天", "老", "師", "堀", "杉", "士", "悟", "森", "霧", "麻", "剛", "垣"];
