mod msg;

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

const NAVICUST_BG_TOB: image::Rgba<u8> = image::Rgba([0x21, 0x8c, 0xa5, 0xff]);
const NAVICUST_BG_TOC: image::Rgba<u8> = image::Rgba([0x5a, 0x5a, 0x4a, 0xff]);

#[rustfmt::skip]
pub static BRBJ_00: Offsets = Offsets {
    chip_data:                          0x0801e1d0,
    chip_names_pointers:                0x08040a68,
    chip_descriptions_pointers:         0x08023afc,
    chip_icon_palette_pointer:          0x0804992c,
    ncp_data:                           0x0813d0cc,
    ncp_names_pointer:                  0x08040a78,
    ncp_descriptions_pointer:           0x08132b28,
    element_icon_palette_pointer:       0x08122ffc,
    element_icons_pointer:              0x08122ff4,
    patch_card_data:                    0x0813842c,
    patch_card_names_pointer:           0x081373c4,
    patch_card_details_names_pointer:   0x081373d0,

    navicust_bg: NAVICUST_BG_TOB,
};

#[rustfmt::skip]
pub static BRKJ_00: Offsets = Offsets {
    chip_data:                          0x0801e1cc,
    chip_names_pointers:                0x08040a70,
    chip_descriptions_pointers:         0x08023af8,
    chip_icon_palette_pointer:          0x08049934,
    ncp_data:                           0x0813d1b4,
    ncp_names_pointer:                  0x08040a80,
    ncp_descriptions_pointer:           0x08132c10,
    element_icon_palette_pointer:       0x081230e4,
    element_icons_pointer:              0x081230dc,
    patch_card_data:                    0x08138514,
    patch_card_names_pointer:           0x081374ac,
    patch_card_details_names_pointer:   0x081374b8,

    navicust_bg: NAVICUST_BG_TOC,
};

#[rustfmt::skip]
pub static BRBE_00: Offsets = Offsets {
    chip_data:                          0x0801e214,
    chip_names_pointers:                0x08040b84,
    chip_descriptions_pointers:         0x08023b40,
    chip_icon_palette_pointer:          0x0804a0f0,
    ncp_data:                           0x0813d540,
    ncp_names_pointer:                  0x08040b94,
    ncp_descriptions_pointer:           0x08132f70,
    element_icon_palette_pointer:       0x081233e0,
    element_icons_pointer:              0x081233d8,
    patch_card_data:                    0x08138874,
    patch_card_names_pointer:           0x0813780c,
    patch_card_details_names_pointer:   0x08137818,

    navicust_bg: NAVICUST_BG_TOB,
};

#[rustfmt::skip]
pub static BRKE_00: Offsets = Offsets {
    chip_data:                          0x0801e210,
    chip_names_pointers:                0x08040b8c,
    chip_descriptions_pointers:         0x08023b3c,
    chip_icon_palette_pointer:          0x0804a0f8,
    ncp_data:                           0x0813d628,
    ncp_names_pointer:                  0x08040b9c,
    ncp_descriptions_pointer:           0x08133058,
    element_icon_palette_pointer:       0x081234c8,
    element_icons_pointer:              0x081234c0,
    patch_card_data:                    0x0813895c,
    patch_card_names_pointer:           0x081378f4,
    patch_card_details_names_pointer:   0x08137900,

    navicust_bg: NAVICUST_BG_TOC,
};

pub struct Assets {
    offsets: &'static Offsets,
    msg_parser: crate::msg::Parser,
    mapper: crate::rom::MemoryMapper,
    chip_icon_palette: [image::Rgba<u8>; 16],
    element_icon_palette: [image::Rgba<u8>; 16],
}

struct Chip<'a> {
    id: usize,
    assets: &'a Assets,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy, c2rust_bitfields::BitfieldStruct)]
struct RawChip {
    codes: [u8; 4],
    _attack_element: u8,
    _rarity: u8,
    element: u8,
    class: u8,
    mb: u8,

    #[bitfield(name = "dark", ty = "bool", bits = "5..=5")]
    effect_flags: [u8; 1],

    _counter_settings: u8,
    _attack_family: u8,
    _attack_subfamily: u8,
    _dark_soul_usage_behavior: u8,
    _unk_0e: u8,
    _lock_on: u8,
    _attack_params: [u8; 4],
    _delay: u8,
    _karma: u8,
    _library_number: u8,
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

impl<'a> crate::rom::Chip for Chip<'a> {
    fn name(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_names_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = crate::msg::get_entry(&region, id)?;

        self.assets
            .msg_parser
            .parse(entry)
            .ok()?
            .into_iter()
            .map(|part| {
                Some(match part {
                    crate::msg::Chunk::Text(s) => s,
                    crate::msg::Chunk::Command { op, params } if op == msg::EREADER_NAME_COMMAND => {
                        let cmd = bytemuck::pod_read_unaligned::<msg::EreaderNameCommand>(&params);
                        if let Ok(parts) = self.assets.msg_parser.parse(&self.assets.mapper.get(
                            (super::save::EREADER_NAME_OFFSET + cmd.index as usize * super::save::EREADER_NAME_SIZE)
                                as u32
                                | 0x02000000,
                        )) {
                            parts
                                .into_iter()
                                .flat_map(|part| {
                                    match part {
                                        crate::msg::Chunk::Text(s) => s,
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

    fn description(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_descriptions_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = crate::msg::get_entry(&region, id)?;

        self.assets
            .msg_parser
            .parse(entry)
            .ok()?
            .into_iter()
            .map(|part| {
                Some(match part {
                    crate::msg::Chunk::Text(s) => s,
                    crate::msg::Chunk::Command { op, params } if op == msg::EREADER_DESCRIPTION_COMMAND => {
                        let cmd = bytemuck::pod_read_unaligned::<msg::EreaderDescriptionCommand>(&params);
                        if let Ok(parts) = self.assets.msg_parser.parse(&self.assets.mapper.get(
                            (super::save::EREADER_DESCRIPTION_OFFSET
                                + cmd.index as usize * super::save::EREADER_DESCRIPTION_SIZE)
                                as u32
                                | 0x02000000,
                        )) {
                            parts
                                .into_iter()
                                .flat_map(|part| {
                                    match part {
                                        crate::msg::Chunk::Text(s) => s,
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
        crate::rom::apply_palette(
            crate::rom::read_merged_tiles(&self.assets.mapper.get(raw.icon_ptr)[..crate::rom::TILE_BYTES * 4], 2)
                .unwrap(),
            &self.assets.chip_icon_palette,
        )
    }

    fn image(&self) -> image::RgbaImage {
        let raw = self.raw();
        crate::rom::apply_palette(
            crate::rom::read_merged_tiles(
                &self.assets.mapper.get(raw.image_ptr)[..crate::rom::TILE_BYTES * 7 * 6],
                7,
            )
            .unwrap(),
            &crate::rom::read_palette(&self.assets.mapper.get(raw.palette_ptr)[..32]),
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

    fn class(&self) -> crate::rom::ChipClass {
        let raw = self.raw();
        match raw.class {
            0 => crate::rom::ChipClass::Standard,
            1 => crate::rom::ChipClass::Mega,
            2 => crate::rom::ChipClass::Giga,
            4 => crate::rom::ChipClass::ProgramAdvance,
            _ => crate::rom::ChipClass::None,
        }
    }

    fn dark(&self) -> bool {
        let raw = self.raw();
        raw.dark()
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

impl<'a> crate::rom::NavicustPart for NavicustPart<'a> {
    fn name(&self) -> Option<String> {
        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.ncp_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = crate::msg::get_entry(&region, self.id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match &part {
                        crate::msg::Chunk::Text(s) => s,
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
        let entry = crate::msg::get_entry(&region, self.id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        crate::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    fn color(&self) -> Option<crate::rom::NavicustPartColor> {
        let raw: RawNavicustPart = self.raw();
        Some(match raw.color {
            1 => crate::rom::NavicustPartColor::White,
            2 => crate::rom::NavicustPartColor::Yellow,
            3 => crate::rom::NavicustPartColor::Pink,
            4 => crate::rom::NavicustPartColor::Red,
            5 => crate::rom::NavicustPartColor::Blue,
            6 => crate::rom::NavicustPartColor::Green,
            _ => {
                return None;
            }
        })
    }

    fn is_solid(&self) -> bool {
        let raw = self.raw();
        raw.is_solid == 0
    }

    fn uncompressed_bitmap(&self) -> crate::rom::NavicustBitmap {
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

    fn compressed_bitmap(&self) -> crate::rom::NavicustBitmap {
        let raw = self.raw();
        ndarray::Array2::from_shape_vec(
            (5, 5),
            self.assets.mapper.get(raw.compressed_bitmap_ptr)[..25]
                .iter()
                .map(|x| *x != 0)
                .collect(),
        )
        .unwrap()
    }
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[String], rom: Vec<u8>, wram: Vec<u8>) -> Self {
        let mapper = crate::rom::MemoryMapper::new(rom, wram);

        let chip_icon_palette = crate::rom::read_palette(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.chip_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..32],
        );

        let element_icon_palette = crate::rom::read_palette(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.element_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..32],
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

impl<'a> crate::rom::PatchCard56 for PatchCard56<'a> {
    fn name(&self) -> Option<String> {
        if self.id == 0 {
            return Some("".to_string());
        }

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.patch_card_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = crate::msg::get_entry(&region, self.id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        crate::msg::Chunk::Text(s) => s,
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
                    crate::msg::get_entry(&region, effect.id as usize)
                        .and_then(|entry| self.assets.msg_parser.parse(entry).ok())
                        .and_then(|chunks| {
                            chunks
                                .into_iter()
                                .map(|chunk| match chunk {
                                    crate::msg::Chunk::Text(s) => {
                                        Some(crate::rom::PatchCard56EffectTemplatePart::String(s))
                                    }
                                    crate::msg::Chunk::Command { op, params } if op == msg::PRINT_VAR_COMMAND => {
                                        let cmd = bytemuck::pod_read_unaligned::<msg::PrintVarCommand>(&params);
                                        Some(crate::rom::PatchCard56EffectTemplatePart::PrintVar(cmd.buffer as usize))
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

impl crate::rom::Assets for Assets {
    fn chip<'a>(&'a self, id: usize) -> Option<Box<dyn crate::rom::Chip + 'a>> {
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

    fn regular_chip_is_in_place(&self) -> bool {
        true
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= 13 {
            return None;
        }

        let buf = self.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.mapper.get(self.offsets.element_icons_pointer)[..std::mem::size_of::<u32>()],
        ));
        Some(crate::rom::apply_palette(
            crate::rom::read_merged_tiles(&buf[id * crate::rom::TILE_BYTES * 4..][..crate::rom::TILE_BYTES * 4], 2)
                .unwrap(),
            &self.element_icon_palette,
        ))
    }

    fn navicust_part<'a>(&'a self, id: usize, variant: usize) -> Option<Box<dyn crate::rom::NavicustPart + 'a>> {
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

    fn patch_card56<'a>(&'a self, id: usize) -> Option<Box<dyn crate::rom::PatchCard56 + 'a>> {
        if id >= self.num_patch_card56s() {
            return None;
        }
        Some(Box::new(PatchCard56 { id, assets: self }))
    }

    fn num_patch_card56s(&self) -> usize {
        super::NUM_PATCH_CARD56S
    }

    fn navicust_layout(&self) -> Option<crate::rom::NavicustLayout> {
        Some(crate::rom::NavicustLayout {
            command_line: 2,
            has_out_of_bounds: false,
            background: self.offsets.navicust_bg,
        })
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "-", "×", "=", ":", "%", "?", "+", "█", "[bat]", "ー", "!", "SP", "DS", "&", ",", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "α", "β", "Ω", "■", "_", "[z]", "周", "え", "お", "う", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "究", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "Ω", "←", "↓", "木", "[MB]", "無", "現", "実", "[circle]", "[cross]", "#", "⋯", "不", "止", "彩", "\\[", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "\\]", "<", "人", "入", "出", "山", "口", "光", "電", "気", "助", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "燃", "朗", "枚", "島", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", ">", "$", "城", "王", "兄", "化", "行", "街", "屋", "水", "見", "終", "丁", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "年", "火", "改", "計", "画", "体", "波", "回", "外", "地", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "所", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "立", "々", "ヶ", "連", "射", "綾", "切", "土", "炎", "伊"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "-", "×", "=", ":", "%", "?", "+", "■", "[bat]", "ー", "!", "SP", "DS", "&", "、", "゜", ".", "・", ";", "’", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "_", "[z]", "周", "え", "お", "う", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "究", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "全", "木", "[MB]", "無", "現", "実", "[circle]", "[cross]", "緑", "尺", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "玉", "各", "人", "入", "出", "山", "口", "光", "電", "気", "助", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "燃", "朗", "枚", "島", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", "異", "門", "城", "王", "兄", "化", "行", "街", "屋", "水", "見", "終", "丁", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "毎", "年", "火", "改", "計", "画", "休", "体", "波", "回", "外", "地", "病", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "所", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "立", "泉", "々", "ヶ", "連", "射", "国", "綾", "切", "土", "炎", "伊"];
