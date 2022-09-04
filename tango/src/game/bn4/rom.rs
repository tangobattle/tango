use byteorder::ByteOrder;

use crate::rom;

pub mod modcards;

pub struct Offsets {
    chip_data: u32,
    chip_names_pointers: u32,
    chip_icon_palette_pointer: u32,
    ncp_data: u32,
    ncp_names_pointer: u32,
    element_icon_palette_pointer: u32,
    element_icons_pointer: u32,
}

#[rustfmt::skip]
pub static B4WJ_01: Offsets = Offsets {
    chip_data:                      0x0801972c,
    chip_names_pointers:            0x0804fa6c,
    chip_icon_palette_pointer:      0x080159d4,
    ncp_data:                       0x08045538,
    ncp_names_pointer:              0x0804fa7c,
    element_icon_palette_pointer:   0x081098ac,
    element_icons_pointer:          0x081098a0,
};

#[rustfmt::skip]
pub static B4BJ_00: Offsets = Offsets {
    chip_data:                      0x080196f8,
    chip_names_pointers:            0x0804fa3c,
    chip_icon_palette_pointer:      0x080159a0,
    ncp_data:                       0x08045504,
    ncp_names_pointer:              0x0804fa4c,
    element_icon_palette_pointer:   0x0810983c,
    element_icons_pointer:          0x08109830,
};

#[rustfmt::skip]
pub static B4WE_00: Offsets = Offsets {
    chip_data:                      0x080197ec,
    chip_names_pointers:            0x0804fb74,
    chip_icon_palette_pointer:      0x08015a78,
    ncp_data:                       0x0804563c,
    ncp_names_pointer:              0x0804fb84,
    element_icon_palette_pointer:   0x08106bd8,
    element_icons_pointer:          0x081099cc,
};

#[rustfmt::skip]
pub static B4BE_00: Offsets = Offsets {
    chip_data:                      0x080197ec,
    chip_names_pointers:            0x0804fb80,
    chip_icon_palette_pointer:      0x08015a78,
    ncp_data:                       0x08045644,
    ncp_names_pointer:              0x0804fb90,
    element_icon_palette_pointer:   0x081099e4,
    element_icons_pointer:          0x081099d8,
};

const NEWLINE_COMMAND: u8 = 0xe8;
const EREADER_COMMAND: u8 = 0xff;

lazy_static! {
    pub static ref TEXT_PARSE_OPTIONS: rom::text::ParseOptions =
        rom::text::ParseOptions::new(0xe4, 0xe5)
            .with_command(NEWLINE_COMMAND, 0)
            .with_command(EREADER_COMMAND, 2);
}

pub struct Assets {
    element_icons: [image::RgbaImage; 13],
    chips: [rom::Chip; 389],
    navicust_parts: [rom::NavicustPart; 188],
    modcards: &'static [rom::Modcard4; 133],
}

impl Assets {
    pub fn new(
        offsets: &Offsets,
        modcards: &'static [rom::Modcard4; 133],
        charset: &[&str],
        rom: &[u8],
        wram: &[u8],
    ) -> Self {
        let mapper = rom::MemoryMapper::new(rom, wram);

        let chip_icon_palette = rom::read_palette(
            &mapper.get(byteorder::LittleEndian::read_u32(
                &mapper.get(offsets.chip_icon_palette_pointer)[..4],
            ))[..32],
        );

        Self {
            element_icons: {
                let palette = rom::read_palette(
                    &mapper.get(byteorder::LittleEndian::read_u32(
                        &mapper.get(offsets.element_icon_palette_pointer)[..4],
                    ))[..32],
                );
                {
                    let buf = mapper.get(byteorder::LittleEndian::read_u32(
                        &mapper.get(offsets.element_icons_pointer)[..4],
                    ));
                    (0..13)
                        .map(|i| {
                            rom::apply_palette(
                                rom::read_merged_tiles(
                                    &buf[i * rom::TILE_BYTES * 4..(i + 1) * rom::TILE_BYTES * 4],
                                    2,
                                )
                                .unwrap(),
                                &palette,
                            )
                        })
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap()
                }
            },
            chips: (0..389)
                .map(|i| {
                    let buf = &mapper.get(offsets.chip_data)[i * 0x2c..(i + 1) * 0x2c];
                    let flags = buf[0x09];
                    rom::Chip {
                        name: {
                            let (i, pointer) = if i < 0x100 {
                                (i, offsets.chip_names_pointers)
                            } else {
                                (i - 0x100, offsets.chip_names_pointers + 4)
                            };
                            if let Ok(parts) = rom::text::parse_entry(
                                &mapper.get(byteorder::LittleEndian::read_u32(
                                    &mapper.get(pointer)[..4],
                                )),
                                i,
                                &TEXT_PARSE_OPTIONS,
                            ) {
                                parts
                                    .into_iter()
                                    .flat_map(|part| {
                                        match part {
                                            rom::text::Part::Literal(c) => {
                                                charset.get(c).unwrap_or(&"�").to_string()
                                            }
                                            rom::text::Part::Command {
                                                op: EREADER_COMMAND,
                                                params,
                                            } => {
                                                if let Ok(parts) = rom::text::parse_entry(
                                                    &mapper
                                                        .get(0x02001770 + params[1] as u32 * 0x10),
                                                    0,
                                                    &TEXT_PARSE_OPTIONS,
                                                ) {
                                                    parts
                                                        .into_iter()
                                                        .flat_map(|part| {
                                                            match part {
                                                                rom::text::Part::Literal(c) => {
                                                                    charset
                                                                        .get(c)
                                                                        .unwrap_or(&"�")
                                                                        .to_string()
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
                                            _ => "".to_string(),
                                        }
                                        .chars()
                                        .collect::<Vec<_>>()
                                    })
                                    .collect::<String>()
                            } else {
                                "???".to_string()
                            }
                        },
                        icon: rom::apply_palette(
                            rom::read_merged_tiles(
                                &mapper
                                    .get(byteorder::LittleEndian::read_u32(&buf[0x20..0x20 + 4]))
                                    [..rom::TILE_BYTES * 4],
                                2,
                            )
                            .unwrap(),
                            &chip_icon_palette,
                        ),
                        codes: buf[0x00..0x04].iter().cloned().collect(),
                        element: buf[0x07] as usize,
                        class: [
                            rom::ChipClass::Standard,
                            rom::ChipClass::Mega,
                            rom::ChipClass::Giga,
                            rom::ChipClass::None,
                            rom::ChipClass::ProgramAdvance,
                        ][buf[0x08] as usize],
                        dark: (flags & 0x20) != 0,
                        mb: buf[0x06],
                        damage: {
                            let damage =
                                byteorder::LittleEndian::read_u16(&buf[0x1a..0x1a + 2]) as u32;
                            if damage < 1000 {
                                damage
                            } else {
                                0
                            }
                        },
                    }
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            navicust_parts: (0..188)
                .map(|i| {
                    let buf = &mapper.get(offsets.ncp_data)[i * 0x10..(i + 1) * 0x10];
                    rom::NavicustPart {
                        name: {
                            if let Ok(parts) = rom::text::parse_entry(
                                &mapper.get(byteorder::LittleEndian::read_u32(
                                    &mapper.get(offsets.ncp_names_pointer)[..4],
                                )),
                                i / 4,
                                &TEXT_PARSE_OPTIONS,
                            ) {
                                parts
                                    .into_iter()
                                    .flat_map(|part| {
                                        match part {
                                            rom::text::Part::Literal(c) => {
                                                charset.get(c).unwrap_or(&"�")
                                            }
                                            _ => "",
                                        }
                                        .chars()
                                    })
                                    .collect::<String>()
                            } else {
                                "???".to_string()
                            }
                        },
                        color: [
                            None,
                            Some(rom::NavicustPartColor::White),
                            Some(rom::NavicustPartColor::Pink),
                            Some(rom::NavicustPartColor::Yellow),
                            Some(rom::NavicustPartColor::Red),
                            Some(rom::NavicustPartColor::Blue),
                            Some(rom::NavicustPartColor::Green),
                        ][buf[0x03] as usize]
                            .clone(),
                        is_solid: buf[0x01] == 0,
                        compressed_bitmap: image::ImageBuffer::from_vec(
                            5,
                            5,
                            mapper.get(byteorder::LittleEndian::read_u32(&buf[0x08..0x0c]))[..49]
                                .to_vec(),
                        )
                        .unwrap(),
                        uncompressed_bitmap: image::ImageBuffer::from_vec(
                            5,
                            5,
                            mapper.get(byteorder::LittleEndian::read_u32(&buf[0x0c..0x10]))[..49]
                                .to_vec(),
                        )
                        .unwrap(),
                    }
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            modcards,
        }
    }
}

impl rom::Assets for Assets {
    fn chip(&self, id: usize) -> Option<&rom::Chip> {
        self.chips.get(id)
    }

    fn element_icon(&self, id: usize) -> Option<&image::RgbaImage> {
        self.element_icons.get(id)
    }

    fn navicust_part(&self, id: usize, variant: usize) -> Option<&rom::NavicustPart> {
        self.navicust_parts.get(id * 4 + variant)
    }

    fn modcard4(&self, id: usize) -> Option<&rom::Modcard4> {
        self.modcards.get(id)
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "-", "×", "=", ":", "%", "?", "+", "÷", "※", "ー", "!", "&", ",", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "α", "β", "Ω", "[V5]", "_", "[MB]", "[z]", "[square]", "[circle]", "[cross]", "■", "⋯", "…", "#", "[bracket1]", "[bracket2]", ">", "<", "★", "♥", "♦", "♣", "♠", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "[?]"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "熱", "斗", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "-", "×", "=", ":", "%", "?", "+", "空", "港", "ー", "!", "現", "実", "&", "、", "。", ".", "・", ";", "’", "\"", "~", "/", "(", ")", "「", "」", "[V2]", "[V3]", "[V4]", "[V5]", "_", "[z]", "周", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "研", "究", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "全", "木", "[MB]", "無", "嵐", "[square]", "[circle]", "[cross]", "駅", "匠", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "転", "各", "人", "入", "出", "山", "口", "光", "電", "気", "助", "科", "戸", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "舟", "朗", "枚", "野", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "村", "花", "問", "異", "門", "城", "王", "兄", "帯", "道", "行", "街", "屋", "水", "見", "終", "丁", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "毎", "年", "火", "改", "計", "画", "休", "体", "波", "回", "外", "地", "病", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "所", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "味", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "立", "泉", "々", "ヶ", "連", "射", "国", "綾", "切", "土", "炎", "伊", "■"];
