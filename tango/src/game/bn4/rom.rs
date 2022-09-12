use byteorder::ByteOrder;

use crate::rom;

pub mod modcards;

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
}

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
};

#[rustfmt::skip]
pub static B4BJ_00: Offsets = Offsets {
    chip_data:                      0x080196f8,
    chip_names_pointers:            0x0804fa3c,
    chip_descriptions_pointers:     0x0801fce4,
    chip_icon_palette_pointer:      0x080159a0,
    ncp_data:                       0x08045504,
    ncp_names_pointer:              0x0804fa4c,
    ncp_descriptions_pointer:       0x0803e528,
    element_icon_palette_pointer:   0x0810983c,
    element_icons_pointer:          0x08109830,
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
};

const EREADER_COMMAND: u8 = 0xff;

pub struct Assets {
    offsets: &'static Offsets,
    text_parse_options: rom::text::ParseOptions,
    mapper: rom::MemoryMapper,
    modcards: &'static [Modcard4; 133],
    chip_icon_palette: [image::Rgba<u8>; 16],
    element_icon_palette: [image::Rgba<u8>; 16],
}

struct Chip<'a> {
    id: usize,
    assets: &'a Assets,
}

impl<'a> Chip<'a> {
    fn raw_info(&'a self) -> [u8; 0x2c] {
        self.assets.mapper.get(self.assets.offsets.chip_data)[self.id * 0x2c..(self.id + 1) * 0x2c]
            .try_into()
            .unwrap()
    }
}

impl<'a> rom::Chip for Chip<'a> {
    fn name(&self) -> String {
        let pointer = self.assets.offsets.chip_names_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        if let Ok(parts) = rom::text::parse_entry(
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
                        rom::text::Part::String(s) => s,
                        rom::text::Part::Command {
                            op: EREADER_COMMAND,
                            params,
                        } => {
                            if let Ok(parts) = rom::text::parse(
                                &self.assets.mapper.get(0x02001772 + params[1] as u32 * 0x10),
                                &self.assets.text_parse_options,
                            ) {
                                parts
                                    .into_iter()
                                    .flat_map(|part| {
                                        match part {
                                            rom::text::Part::String(s) => s,
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

        if let Ok(parts) = rom::text::parse_entry(
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
                        rom::text::Part::String(s) => s,
                        rom::text::Part::Command {
                            op: EREADER_COMMAND,
                            params,
                        } => {
                            if let Ok(parts) = rom::text::parse(
                                &self.assets.mapper.get(0x02000522 + params[1] as u32 * 0x5c),
                                &self.assets.text_parse_options,
                            ) {
                                parts
                                    .into_iter()
                                    .flat_map(|part| {
                                        match part {
                                            rom::text::Part::String(s) => s,
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
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>()
                .replace("-\n", "-")
                .replace("\n", " ")
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
                    .get(byteorder::LittleEndian::read_u32(&raw[0x20..0x20 + 4]))[..rom::TILE_BYTES * 4],
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
                    .get(byteorder::LittleEndian::read_u32(&raw[0x24..0x24 + 4]))[..rom::TILE_BYTES * 7 * 6],
                7,
            )
            .unwrap(),
            &rom::read_palette(
                &self
                    .assets
                    .mapper
                    .get(byteorder::LittleEndian::read_u32(&raw[0x28..0x28 + 4]))[..32],
            ),
        )
    }

    fn codes(&self) -> Vec<u8> {
        let raw = self.raw_info();
        raw[0x00..0x04].iter().cloned().collect()
    }

    fn element(&self) -> usize {
        let raw = self.raw_info();
        raw[0x07] as usize
    }

    fn class(&self) -> rom::ChipClass {
        let raw = self.raw_info();
        [
            rom::ChipClass::Standard,
            rom::ChipClass::Mega,
            rom::ChipClass::Giga,
            rom::ChipClass::None,
            rom::ChipClass::ProgramAdvance,
        ][raw[0x08] as usize]
    }

    fn dark(&self) -> bool {
        let raw = self.raw_info();
        let flags = raw[0x09];
        (flags & 0x20) != 0
    }

    fn mb(&self) -> u8 {
        let raw = self.raw_info();
        raw[0x06]
    }

    fn damage(&self) -> u32 {
        let raw = self.raw_info();
        let damage = byteorder::LittleEndian::read_u16(&raw[0x1a..0x1a + 2]) as u32;
        if damage < 1000 {
            damage
        } else {
            0
        }
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
        if let Ok(parts) = rom::text::parse_entry(
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
                        rom::text::Part::String(s) => s,
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
        if let Ok(parts) = rom::text::parse_entry(
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
                        rom::text::Part::String(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>()
                .replace("-\n", "-")
                .replace("\n", " ")
        } else {
            "???".to_string()
        }
    }

    fn color(&self) -> Option<rom::NavicustPartColor> {
        let raw = self.raw_info();
        [
            None,
            Some(rom::NavicustPartColor::White),
            Some(rom::NavicustPartColor::Pink),
            Some(rom::NavicustPartColor::Yellow),
            Some(rom::NavicustPartColor::Red),
            Some(rom::NavicustPartColor::Blue),
            Some(rom::NavicustPartColor::Green),
        ][raw[0x03] as usize]
            .clone()
    }

    fn is_solid(&self) -> bool {
        let raw = self.raw_info();
        raw[0x01] == 0
    }

    fn compressed_bitmap(&self) -> rom::NavicustBitmap {
        let raw = self.raw_info();
        image::ImageBuffer::from_vec(
            5,
            5,
            self.assets
                .mapper
                .get(byteorder::LittleEndian::read_u32(&raw[0x08..0x0c]))[..25]
                .to_vec(),
        )
        .unwrap()
    }

    fn uncompressed_bitmap(&self) -> rom::NavicustBitmap {
        let raw = self.raw_info();
        image::ImageBuffer::from_vec(
            5,
            5,
            self.assets
                .mapper
                .get(byteorder::LittleEndian::read_u32(&raw[0x0c..0x10]))[..25]
                .to_vec(),
        )
        .unwrap()
    }
}

pub struct Modcard4 {
    name: &'static str,
    slot: u8,
    effect: &'static str,
    bug: Option<&'static str>,
}

impl rom::Modcard4 for &Modcard4 {
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn slot(&self) -> u8 {
        self.slot
    }

    fn effect(&self) -> String {
        self.effect.to_string()
    }

    fn bug(&self) -> Option<String> {
        self.bug.map(|s| s.to_string())
    }
}

impl Assets {
    pub fn new(
        offsets: &'static Offsets,
        modcards: &'static [Modcard4; 133],
        charset: Vec<String>,
        rom: Vec<u8>,
        wram: Vec<u8>,
    ) -> Self {
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
            text_parse_options: rom::text::ParseOptions {
                charset,
                extension_ops: 0xe4..=0xe4,
                eof_op: 0xe5,
                newline_op: 0xe8,
                commands: std::collections::HashMap::from([
                    (EREADER_COMMAND, 2),
                    (0xe6, 1),
                    (0xe7, 1),
                    (0xed, 3),
                    (0xf0, 2),
                ]),
            },
            modcards,
            mapper,
            chip_icon_palette,
            element_icon_palette,
        }
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
        389
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= 13 {
            return None;
        }

        let buf = self.mapper.get(byteorder::LittleEndian::read_u32(
            &self.mapper.get(self.offsets.element_icons_pointer)[..4],
        ));
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
        (47, 4)
    }

    fn modcard4<'a>(&'a self, id: usize) -> Option<Box<dyn rom::Modcard4 + 'a>> {
        self.modcards.get(id).map(|m| Box::new(m) as Box<dyn rom::Modcard4>)
    }

    fn num_modcard4s(&self) -> usize {
        self.modcards.len()
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "-", "×", "=", ":", "%", "?", "+", "÷", "※", "ー", "!", "&", ",", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "α", "β", "Ω", "V5", "_", "[MB]", "[z]", "[square]", "[circle]", "[cross]", "■", "⋯", "…", "#", "[bracket1]", "[bracket2]", ">", "<", "★", "♥", "♦", "♣", "♠", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "[?]"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "熱", "斗", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "-", "×", "=", ":", "%", "?", "+", "空", "港", "ー", "!", "現", "実", "&", "、", "。", ".", "・", ";", "’", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "_", "[z]", "周", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "研", "究", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "全", "木", "[MB]", "無", "嵐", "[square]", "[circle]", "[cross]", "駅", "匠", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "転", "各", "人", "入", "出", "山", "口", "光", "電", "気", "助", "科", "戸", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "舟", "朗", "枚", "野", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "村", "花", "問", "異", "門", "城", "王", "兄", "帯", "道", "行", "街", "屋", "水", "見", "終", "丁", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "毎", "年", "火", "改", "計", "画", "休", "体", "波", "回", "外", "地", "病", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "所", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "味", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "立", "泉", "々", "ヶ", "連", "射", "国", "綾", "切", "土", "炎", "伊", "■"];
