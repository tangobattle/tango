mod msg;
mod patch_cards;

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
    navicust_bg: image::Rgba<u8>,
    patch_cards: &'static [Option<PatchCard4>; super::NUM_PATCH_CARD4S],
}

const NAVICUST_BG_RS: image::Rgba<u8> = image::Rgba([0x8c, 0x10, 0x10, 0xff]);
const NAVICUST_BG_BM: image::Rgba<u8> = image::Rgba([0x52, 0x10, 0xad, 0xff]);

#[rustfmt::skip]
pub static B4WJ_01: Offsets = Offsets {
    chip_data:                      0x0801972c,
    chip_names_pointers:            0x0804fa6c,
    chip_descriptions_pointers:     0x0801fd20,
    chip_icon_palette_pointer:      0x080159d4,
    ncp_data:                       0x08045538,
    ncp_names_pointer:              0x0804fa7c,
    ncp_descriptions_pointer:       0x0803e55c,
    element_icon_palette_pointer:   0x081098ac,
    element_icons_pointer:          0x081098a0,

    navicust_bg: NAVICUST_BG_RS,
    patch_cards: patch_cards::JA_PATCH_CARDS,
};

#[rustfmt::skip]
pub static B4BJ_01: Offsets = Offsets {
    chip_data:                      0x0801972c,
    chip_names_pointers:            0x0804fa78,
    chip_descriptions_pointers:     0x0801fd20,
    chip_icon_palette_pointer:      0x080159d4,
    ncp_data:                       0x08045540,
    ncp_names_pointer:              0x0804fa88,
    ncp_descriptions_pointer:       0x0803e564,
    element_icon_palette_pointer:   0x081098b8,
    element_icons_pointer:          0x081098ac,

    navicust_bg: NAVICUST_BG_BM,
    patch_cards: patch_cards::JA_PATCH_CARDS,
};

#[rustfmt::skip]
pub static B4WE_00: Offsets = Offsets {
    chip_data:                      0x080197ec,
    chip_names_pointers:            0x0804fb74,
    chip_descriptions_pointers:     0x0801fde0,
    chip_icon_palette_pointer:      0x08015a78,
    ncp_data:                       0x0804563c,
    ncp_names_pointer:              0x0804fb84,
    ncp_descriptions_pointer:       0x0803e63c,
    element_icon_palette_pointer:   0x08106bd8,
    element_icons_pointer:          0x081099cc,

    navicust_bg: NAVICUST_BG_RS,
    patch_cards: patch_cards::EN_PATCH_CARDS,
};

#[rustfmt::skip]
pub static B4BE_00: Offsets = Offsets {
    chip_data:                      0x080197ec,
    chip_names_pointers:            0x0804fb80,
    chip_descriptions_pointers:     0x0801fde0,
    chip_icon_palette_pointer:      0x08015a78,
    ncp_data:                       0x08045644,
    ncp_names_pointer:              0x0804fb90,
    ncp_descriptions_pointer:       0x0803e644,
    element_icon_palette_pointer:   0x081099e4,
    element_icons_pointer:          0x081099d8,

    navicust_bg: NAVICUST_BG_BM,
    patch_cards: patch_cards::EN_PATCH_CARDS,
};

pub struct Assets {
    offsets: &'static Offsets,
    msg_parser: msg::Parser,
    mapper: crate::rom::MemoryMapper,
    chip_icon_palette: crate::rom::Palette,
    element_icon_palette: crate::rom::Palette,
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
    mb: u8,
    element: u8,
    class: u8,

    #[bitfield(name = "dark", ty = "bool", bits = "5..=5")]
    effect_flags: [u8; 1],

    _counter_settings: u8,
    _attack_family: u8,
    _attack_subfamily: u8,
    _dark_soul_usage_behavior: u8,
    _unk_0e: [u8; 2],
    _attack_params: [u8; 4],
    _delay: u8,
    _karma: u8,
    _library_flags: u8,
    _unk_17: u8,
    _alphabet_sort: u16,
    attack_power: u16,
    library_sort_order: u16,
    _battle_chip_gate_usage: u8,
    _unused: u8,
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
                    crate::msg::Chunk::Command(command) => match command {
                        msg::Command::EreaderNameCommand(cmd) => {
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
                    },
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
                    crate::msg::Chunk::Command(command) => match command {
                        msg::Command::EreaderDescriptionCommand(cmd) => {
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
                    },
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
            &bytemuck::pod_read_unaligned::<crate::rom::Palette>(
                &self.assets.mapper.get(raw.palette_ptr)[..std::mem::size_of::<crate::rom::Palette>()],
            ),
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
        let raw = self.raw();
        Some(match raw.color {
            1 => crate::rom::NavicustPartColor::White,
            2 => crate::rom::NavicustPartColor::Pink,
            3 => crate::rom::NavicustPartColor::Yellow,
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

pub struct PatchCard4 {
    name: &'static str,
    slot: u8,
    effect: &'static str,
    bug: Option<&'static str>,
}

impl crate::rom::PatchCard4 for &PatchCard4 {
    fn name(&self) -> Option<String> {
        Some(self.name.to_string())
    }

    fn slot(&self) -> u8 {
        self.slot
    }

    fn effect(&self) -> Option<String> {
        Some(self.effect.to_string())
    }

    fn bug(&self) -> Option<String> {
        self.bug.map(|s| s.to_string())
    }
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[String], rom: Vec<u8>, wram: Vec<u8>) -> Self {
        let mapper = crate::rom::MemoryMapper::new(rom, wram);

        let chip_icon_palette = bytemuck::pod_read_unaligned::<crate::rom::Palette>(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.chip_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..std::mem::size_of::<crate::rom::Palette>()],
        );

        let element_icon_palette = bytemuck::pod_read_unaligned::<crate::rom::Palette>(
            &mapper.get(bytemuck::pod_read_unaligned::<u32>(
                &mapper.get(offsets.element_icon_palette_pointer)[..std::mem::size_of::<u32>()],
            ))[..std::mem::size_of::<crate::rom::Palette>()],
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

    fn patch_card4<'a>(&'a self, id: usize) -> Option<Box<dyn crate::rom::PatchCard4 + 'a>> {
        self.offsets
            .patch_cards
            .get(id)
            .map(|m| m.as_ref().map(|m| Box::new(m) as Box<dyn crate::rom::PatchCard4>))
            .flatten()
    }

    fn num_patch_card4s(&self) -> usize {
        super::NUM_PATCH_CARD4S
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
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "-", "×", "=", ":", "%", "?", "+", "÷", "※", "ー", "!", "&", ",", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "α", "β", "Ω", "V5", "_", "[MB]", "[z]", "[square]", "[circle]", "[cross]", "■", "⋯", "…", "#", "[bracket1]", "[bracket2]", ">", "<", "★", "♥", "♦", "♣", "♠", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "[?]"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "熱", "斗", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "-", "×", "=", ":", "%", "?", "+", "空", "港", "ー", "!", "現", "実", "&", "、", "。", ".", "・", ";", "’", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "_", "[z]", "周", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "研", "究", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "全", "木", "[MB]", "無", "嵐", "[square]", "[circle]", "[cross]", "駅", "匠", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "転", "各", "人", "入", "出", "山", "口", "光", "電", "気", "助", "科", "戸", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "舟", "朗", "枚", "野", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "村", "花", "問", "異", "門", "城", "王", "兄", "帯", "道", "行", "街", "屋", "水", "見", "終", "丁", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "毎", "年", "火", "改", "計", "画", "休", "体", "波", "回", "外", "地", "病", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "所", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "味", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "立", "泉", "々", "ヶ", "連", "射", "国", "綾", "切", "土", "炎", "伊", "■"];
