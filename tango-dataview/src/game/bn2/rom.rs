use byteorder::ByteOrder;

use crate::{msg, rom};

pub struct Offsets {
    chip_data: u32,
    chip_names_pointers: u32,
    chip_descriptions_pointers: u32,
    chip_icon_palette_pointer: u32,
    element_icon_palette_pointer: u32,
    element_icons_pointer: u32,
}

#[rustfmt::skip]
pub static AE2E_00: Offsets = Offsets {
    chip_data:                      0x0800e450,
    chip_names_pointers:            0x0800b528,
    chip_descriptions_pointers:     0x08026df4,
    chip_icon_palette_pointer:      0x0800b890,
    element_icons_pointer:          0x08025fe0,
    element_icon_palette_pointer:   0x08005388,
};

#[rustfmt::skip]
pub static AE2J_00_AC: Offsets = Offsets {
    chip_data:                      0x0800e2fc,
    chip_names_pointers:            0x0800b528,
    chip_descriptions_pointers:     0x0800affc,
    chip_icon_palette_pointer:      0x0800b750,
    element_icons_pointer:          0x08025ec0,
    element_icon_palette_pointer:   0x08005384,
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

#[repr(packed)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
struct RawChip {
    codes: [u8; 6],
    element: u8,
    _family: u8,
    _subfamily: u8,
    _rarity: u8,
    mb: u8,
    _unk_0a: u8,
    damage: u16,
    _unk_0e: [u8; 6],
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

impl<'a> rom::Chip for Chip<'a> {
    fn name(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_names_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self
            .assets
            .mapper
            .get(byteorder::LittleEndian::read_u32(&self.assets.mapper.get(pointer)[..4]));
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

        let region = self
            .assets
            .mapper
            .get(byteorder::LittleEndian::read_u32(&self.assets.mapper.get(pointer)[..4]));
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
            rom::read_merged_tiles(&self.assets.mapper.get(raw.image_ptr)[..rom::TILE_BYTES * 8 * 7], 8).unwrap(),
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
        rom::ChipClass::Standard
    }

    fn dark(&self) -> bool {
        false
    }

    fn mb(&self) -> u8 {
        let raw = self.raw();
        raw.mb
    }

    fn damage(&self) -> u32 {
        let raw = self.raw();
        raw.damage as u32
    }

    fn library_sort_order(&self) -> Option<usize> {
        Some(self.id)
    }
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[String], rom: Vec<u8>, wram: Vec<u8>) -> Self {
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
            msg_parser: msg::Parser::builder()
                .with_ignore_unknown(true)
                .add_eof_rule(b"\xe7")
                .add_charset_rules(charset, 0xe5)
                .add_text_rule(b"\xe8", "\n")
                .add_command_rule(b"\xeb", 0)
                .add_command_rule(b"\xec\x00", 1)
                .add_command_rule(b"\xf1\x02", 0)
                .add_command_rule(b"\xf1\x03", 0)
                .build(),
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
        Some(rom::apply_palette(
            rom::read_merged_tiles(&buf[id * rom::TILE_BYTES * 4..][..rom::TILE_BYTES * 4], 2).unwrap(),
            &self.element_icon_palette,
        ))
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "V2", "V3", "-", "×", "=", ":", "?", "+", "÷", "※", "*", "!", "�", "%", "&", ",", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "↑", "→", "↓", "←", "@", "★", "♪", "<", ">", "[bracket1]", "[bracket2]", "■", "$", "#"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "V2", "V3", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "ー", "×", "=", ":", "?", "+", "÷", "※", "*", "!", "[?]", "%", "&", "、", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "↑", "→", "↓", "←", "@", "♥", "♪", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "ヰ", "ヱ", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "止", "彩", "起", "父", "博", "士", "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "磁", "真", "人", "入", "出", "山", "口", "光", "電", "気", "話", "広", "王", "名", "前", "学", "校", "�", "�", "室", "世", "界", "�", "�", "�", "�", "�", "機", "器", "大", "小", "中", "自", "分", "間", "�", "�", "問", "�", "門", "熱", "斗", "要", "�", "道", "行", "街", "屋", "水", "見", "�", "教", "走", "先", "生", "長", "今", "�", "点", "女", "子", "言", "会", "来", "風", "吹", "速", "思", "時", "円", "知", "毎", "年", "火", "朝", "計", "画", "休", "体", "波", "回", "外", "多", "�", "正", "死", "値", "合", "戦", "争", "秋", "原", "町", "天", "用", "金", "男", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "�", "以", "�", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "岩", "才", "者", "立", "�", "々", "ヶ", "連", "射", "国", "�", "耳", "土", "炎", "伊", "集", "院", "各", "科", "省", "祐", "朗", "枚", "�", "川", "花", "兄", "音", "属", "性", "持", "勝", "赤", "丁", "地", "所", "明", "切", "急", "木", "高", "駅", "店", "研", "究"];
