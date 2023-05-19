use byteorder::ByteOrder;

use crate::{rom, text};

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
    key_items_names_pointer: u32,
    navicust_bg: image::Rgba<u8>,
}

const NAVICUST_BG_W: image::Rgba<u8> = image::Rgba([0x4a, 0x63, 0x7b, 0xff]);
const NAVICUST_BG_B: image::Rgba<u8> = image::Rgba([0x5a, 0x5a, 0x5a, 0xff]);

#[rustfmt::skip]
pub static A6BJ_01: Offsets = Offsets {
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
    text_parse_options: text::ParseOptions,
    mapper: rom::MemoryMapper,
    chip_icon_palette: [image::Rgba<u8>; 16],
    element_icon_palette: [image::Rgba<u8>; 16],
}

struct Chip<'a> {
    id: usize,
    assets: &'a Assets,
}

impl<'a> Chip<'a> {
    fn raw_info(&'a self) -> [u8; 0x20] {
        self.assets.mapper.get(self.assets.offsets.chip_data)[self.id * 0x20..(self.id + 1) * 0x20]
            .try_into()
            .unwrap()
    }
}

impl<'a> rom::Chip for Chip<'a> {
    fn name(&self) -> String {
        let pointer = self.assets.offsets.chip_names_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        if let Ok(parts) = text::parse_entry(
            &self
                .assets
                .mapper
                .get(byteorder::LittleEndian::read_u32(&self.assets.mapper.get(pointer)[..4])),
            id,
            &self.assets.text_parse_options,
        ) {
            parts
                .into_iter()
                .flat_map(|part| {
                    match part {
                        text::Part::String(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>()
        } else {
            "???".to_string()
        }
    }

    fn description(&self) -> String {
        let pointer = self.assets.offsets.chip_descriptions_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        if let Ok(parts) = text::parse_entry(
            &self
                .assets
                .mapper
                .get(byteorder::LittleEndian::read_u32(&self.assets.mapper.get(pointer)[..4])),
            id,
            &self.assets.text_parse_options,
        ) {
            parts
                .into_iter()
                .flat_map(|part| {
                    match part {
                        text::Part::String(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>()
        } else {
            "???".to_string()
        }
    }

    fn icon(&self) -> image::RgbaImage {
        let raw = self.raw_info();
        rom::apply_palette(
            rom::read_merged_tiles(
                &self
                    .assets
                    .mapper
                    .get(byteorder::LittleEndian::read_u32(&raw[0x14..0x14 + 4]))[..rom::TILE_BYTES * 4],
                2,
            )
            .unwrap(),
            &self.assets.chip_icon_palette,
        )
    }

    fn image(&self) -> image::RgbaImage {
        let raw = self.raw_info();
        rom::apply_palette(
            rom::read_merged_tiles(
                &self
                    .assets
                    .mapper
                    .get(byteorder::LittleEndian::read_u32(&raw[0x18..0x18 + 4]))[..rom::TILE_BYTES * 8 * 7],
                8,
            )
            .unwrap(),
            &rom::read_palette(
                &self
                    .assets
                    .mapper
                    .get(byteorder::LittleEndian::read_u32(&raw[0x1c..0x1c + 4]))[..32],
            ),
        )
    }

    fn codes(&self) -> Vec<char> {
        let raw = self.raw_info();
        raw[0x00..0x06]
            .iter()
            .cloned()
            .filter(|code| *code != 0xff)
            .map(|code| b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[code as usize] as char)
            .collect()
    }

    fn element(&self) -> usize {
        let raw = self.raw_info();
        raw[0x06] as usize
    }

    fn class(&self) -> rom::ChipClass {
        let raw = self.raw_info();
        let flags = raw[0x13];
        if flags & 0x02 != 0 {
            rom::ChipClass::Giga
        } else if flags & 0x01 != 0 {
            rom::ChipClass::Mega
        } else {
            rom::ChipClass::Standard
        }
    }

    fn dark(&self) -> bool {
        false
    }

    fn mb(&self) -> u8 {
        let raw = self.raw_info();
        raw[0x0a]
    }

    fn damage(&self) -> u32 {
        let raw = self.raw_info();
        let damage = byteorder::LittleEndian::read_u16(&raw[0x0c..0x0c + 2]) as u32;
        if damage < 1000 {
            damage
        } else {
            0
        }
    }

    fn library_sort_order(&self) -> Option<usize> {
        let raw = self.raw_info();
        Some(byteorder::LittleEndian::read_u16(&raw[0xe..0xe + 2]) as usize)
    }
}

struct NavicustPart<'a> {
    id: usize,
    variant: usize,
    assets: &'a Assets,
}

impl<'a> NavicustPart<'a> {
    fn raw_info(&'a self) -> [u8; 0x10] {
        let i = self.id * 4 + self.variant;
        self.assets.mapper.get(self.assets.offsets.ncp_data)[i * 0x10..(i + 1) * 0x10]
            .try_into()
            .unwrap()
    }
}

impl<'a> rom::NavicustPart for NavicustPart<'a> {
    fn name(&self) -> String {
        if let Ok(parts) = text::parse_entry(
            &self.assets.mapper.get(byteorder::LittleEndian::read_u32(
                &self.assets.mapper.get(self.assets.offsets.ncp_names_pointer)[..4],
            )),
            self.id,
            &self.assets.text_parse_options,
        ) {
            parts
                .into_iter()
                .flat_map(|part| {
                    match &part {
                        text::Part::String(s) => s,
                        _ => "",
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>()
        } else {
            "???".to_string()
        }
    }

    fn description(&self) -> String {
        if let Ok(parts) = text::parse_entry(
            &self.assets.mapper.get(byteorder::LittleEndian::read_u32(
                &self.assets.mapper.get(self.assets.offsets.ncp_descriptions_pointer)[..4],
            )),
            self.id,
            &self.assets.text_parse_options,
        ) {
            parts
                .into_iter()
                .flat_map(|part| {
                    match part {
                        text::Part::String(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>()
        } else {
            "???".to_string()
        }
    }

    fn color(&self) -> Option<rom::NavicustPartColor> {
        let raw = self.raw_info();
        Some(match raw[0x03] {
            1 => rom::NavicustPartColor::White,
            2 => rom::NavicustPartColor::Pink,
            3 => rom::NavicustPartColor::Yellow,
            4 => rom::NavicustPartColor::Red,
            5 => rom::NavicustPartColor::Blue,
            6 => rom::NavicustPartColor::Green,
            7 => rom::NavicustPartColor::Orange,
            8 => rom::NavicustPartColor::Purple,
            9 => rom::NavicustPartColor::Gray,
            _ => {
                return None;
            }
        })
    }

    fn is_solid(&self) -> bool {
        let raw = self.raw_info();
        raw[0x01] == 0
    }

    fn uncompressed_bitmap(&self) -> rom::NavicustBitmap {
        let raw = self.raw_info();
        ndarray::Array2::from_shape_vec(
            (5, 5),
            self.assets
                .mapper
                .get(byteorder::LittleEndian::read_u32(&raw[0x08..0x0c]))[..25]
                .iter()
                .map(|x| *x != 0)
                .collect(),
        )
        .unwrap()
    }

    fn compressed_bitmap(&self) -> rom::NavicustBitmap {
        let raw = self.raw_info();
        ndarray::Array2::from_shape_vec(
            (5, 5),
            self.assets
                .mapper
                .get(byteorder::LittleEndian::read_u32(&raw[0x0c..0x10]))[..25]
                .iter()
                .map(|x| *x != 0)
                .collect(),
        )
        .unwrap()
    }
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: Vec<String>, rom: Vec<u8>, wram: Vec<u8>) -> Self {
        let mapper = rom::MemoryMapper::new(rom, wram);
        let chip_icon_palette = rom::read_palette(
            &mapper.get(byteorder::LittleEndian::read_u32(
                &mapper.get(offsets.chip_icon_palette_pointer)[..4],
            ))[..32],
        );
        let element_icon_palette = rom::read_palette(
            &mapper.get(byteorder::LittleEndian::read_u32(
                &mapper.get(offsets.element_icon_palette_pointer)[..4],
            ))[..32],
        );

        Self {
            offsets,
            text_parse_options: text::ParseOptions {
                charset,
                extension_ops: 0xe5..=0xe6,
                eof_op: 0xe7,
                newline_op: 0xe8,
                commands: std::collections::HashMap::from([(0xea, 3), (0xeb, 0), (0xec, 2), (0xee, 3), (0xf1, 1)]),
            },
            mapper,
            chip_icon_palette,
            element_icon_palette,
        }
    }
}

struct Style<'a> {
    id: usize,
    assets: &'a Assets,
}

impl<'a> rom::Style for Style<'a> {
    fn name(&self) -> String {
        let typ = self.id >> 3;
        let element = self.id & 0x7;

        if let Ok(parts) = text::parse_entry(
            &self.assets.mapper.get(byteorder::LittleEndian::read_u32(
                &self.assets.mapper.get(self.assets.offsets.key_items_names_pointer)[..4],
            )),
            128 + typ * 5 + element,
            &self.assets.text_parse_options,
        ) {
            parts
                .into_iter()
                .flat_map(|part| {
                    match &part {
                        text::Part::String(s) => s,
                        _ => "",
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>()
        } else {
            "???".to_string()
        }
    }

    fn extra_ncp_color(&self) -> Option<rom::NavicustPartColor> {
        Some(match self.id >> 3 {
            1 => rom::NavicustPartColor::Red,
            2 => rom::NavicustPartColor::Blue,
            3 => rom::NavicustPartColor::Green,
            4 => rom::NavicustPartColor::Blue,
            5 => rom::NavicustPartColor::Green,
            6 => rom::NavicustPartColor::Red,
            7 => rom::NavicustPartColor::Gray,
            _ => {
                return None;
            }
        })
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

    fn regular_chip_is_in_place(&self) -> bool {
        true
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= 5 {
            return None;
        }

        let buf = self.mapper.get(byteorder::LittleEndian::read_u32(
            &self.mapper.get(self.offsets.element_icons_pointer)[..4],
        ));
        let buf = &buf[0x1e0..];
        Some(rom::apply_palette(
            rom::read_merged_tiles(&buf[id * rom::TILE_BYTES * 4..(id + 1) * rom::TILE_BYTES * 4], 2).unwrap(),
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

    fn style<'a>(&'a self, id: usize) -> Option<Box<dyn rom::Style + 'a>> {
        if id >= self.num_styles() {
            return None;
        }
        Some(Box::new(Style { id, assets: self }))
    }

    fn num_styles(&self) -> usize {
        super::NUM_STYLES
    }

    fn navicust_layout(&self) -> Option<rom::NavicustLayout> {
        Some(rom::NavicustLayout {
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
