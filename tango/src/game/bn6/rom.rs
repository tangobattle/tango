use byteorder::ByteOrder;

use crate::rom;

pub struct Offsets {
    chip_data: u32,
    chip_names_pointers: u32,
    chip_icon_palette_pointer: u32,
    ncp_data: u32,
    ncp_names_pointer: u32,
    element_icon_palette_pointer: u32,
    element_icons_pointer: u32,
    modcard_data: u32,
    modcard_names_pointer: u32,
    modcard_details_names_pointer: u32,
}

#[rustfmt::skip]
pub static BR5J_00: Offsets = Offsets {
    chip_data:                      0x080221bc,
    chip_names_pointers:            0x08043274,
    chip_icon_palette_pointer:      0x0801f144,
    ncp_data:                       0x081460cc,
    ncp_names_pointer:              0x08043284,
    element_icon_palette_pointer:   0x081226e4,
    element_icons_pointer:          0x081226dc,
    modcard_data:                   0x08144778,
    modcard_names_pointer:          0x08130fe0,
    modcard_details_names_pointer:  0x08130fec,
};

#[rustfmt::skip]
pub static BR6J_00: Offsets = Offsets {
    chip_data:                      0x080221bc,
    chip_names_pointers:            0x080432a4,
    chip_icon_palette_pointer:      0x0801f144,
    ncp_data:                       0x08144300,
    ncp_names_pointer:              0x080432b4,
    element_icon_palette_pointer:   0x081213c4,
    element_icons_pointer:          0x081213bc,
    modcard_data:                   0x081429b0,
    modcard_names_pointer:          0x0812f218,
    modcard_details_names_pointer:  0x0812f224,
};

#[rustfmt::skip]
pub static BR5E_00: Offsets = Offsets {
    chip_data:                      0x08021da8,
    chip_names_pointers:            0x08042038,
    chip_icon_palette_pointer:      0x0801ed20,
    ncp_data:                       0x0813b22c,
    ncp_names_pointer:              0x08042048,
    element_icon_palette_pointer:   0x0811a9a4,
    element_icons_pointer:          0x0811a99c,
    modcard_data:                   0,
    modcard_names_pointer:          0,
    modcard_details_names_pointer:  0,
};

#[rustfmt::skip]
pub static BR6E_00: Offsets = Offsets {
    chip_data:                      0x08021da8,
    chip_names_pointers:            0x08042068,
    chip_icon_palette_pointer:      0x0801ed20,
    ncp_data:                       0x0813944c,
    ncp_names_pointer:              0x08042078,
    element_icon_palette_pointer:   0x08119674,
    element_icons_pointer:          0x0811966c,
    modcard_data:                   0,
    modcard_names_pointer:          0,
    modcard_details_names_pointer:  0,
};

const NEWLINE_TEXT_COMMAND: u8 = 0xe9;
const PRINT_VAR_COMMAND: u8 = 0xfa;

lazy_static! {
    pub static ref TEXT_PARSE_OPTIONS: rom::text::ParseOptions =
        rom::text::ParseOptions::new(0xe4, 0xe6)
            .with_command(NEWLINE_TEXT_COMMAND, 0)
            .with_command(PRINT_VAR_COMMAND, 3);
}

pub struct Assets {
    element_icons: [image::RgbaImage; 11],
    chips: [rom::Chip; 411],
    navicust_parts: [rom::NavicustPart; 188],
    modcards56: Option<[rom::Modcard56; 118]>,
}

impl Assets {
    pub fn new(offsets: &Offsets, charset: &[&str], rom: &[u8], wram: &[u8]) -> Self {
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
                    (0..11)
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
            chips: (0..411)
                .map(|i| {
                    let buf = &mapper.get(offsets.chip_data)[i * 0x2c..(i + 1) * 0x2c];
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
                        codes: buf[0x00..0x04]
                            .iter()
                            .cloned()
                            .filter(|code| *code != 0xff)
                            .collect(),
                        element: buf[0x06] as usize,
                        class: [
                            rom::ChipClass::Standard,
                            rom::ChipClass::Mega,
                            rom::ChipClass::Giga,
                            rom::ChipClass::None,
                            rom::ChipClass::ProgramAdvance,
                        ][buf[0x07] as usize],
                        dark: false,
                        mb: buf[0x08],
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
                            Some(rom::NavicustPartColor::Yellow),
                            Some(rom::NavicustPartColor::Pink),
                            Some(rom::NavicustPartColor::Red),
                            Some(rom::NavicustPartColor::Blue),
                            Some(rom::NavicustPartColor::Green),
                        ][buf[0x03] as usize]
                            .clone(),
                        is_solid: buf[0x01] == 0,
                        compressed_bitmap: image::ImageBuffer::from_vec(
                            7,
                            7,
                            mapper.get(byteorder::LittleEndian::read_u32(&buf[0x08..0x0c]))[..49]
                                .to_vec(),
                        )
                        .unwrap(),
                        uncompressed_bitmap: image::ImageBuffer::from_vec(
                            7,
                            7,
                            mapper.get(byteorder::LittleEndian::read_u32(&buf[0x0c..0x10]))[..49]
                                .to_vec(),
                        )
                        .unwrap(),
                    }
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            modcards56: if offsets.modcard_data != 0 {
                Some(
                    [rom::Modcard56 {
                        name: "".to_string(),
                        mb: 0,
                        effects: vec![],
                    }]
                    .into_iter()
                    .chain((1..118).map(|i| {
                        let buf = mapper.get(offsets.modcard_data);
                        let buf = &buf[byteorder::LittleEndian::read_u16(&buf[i * 2..(i + 1) * 2])
                            as usize
                            ..byteorder::LittleEndian::read_u16(&buf[(i + 1) * 2..(i + 2) * 2])
                                as usize];
                        rom::Modcard56 {
                            name: {
                                if let Ok(parts) = rom::text::parse_entry(
                                    &mapper.get(byteorder::LittleEndian::read_u32(
                                        &mapper.get(offsets.modcard_names_pointer)[..4],
                                    )),
                                    i,
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
                            mb: buf[1],
                            effects: buf[3..]
                                .chunks(3)
                                .map(|chunk| {
                                    let id = chunk[0];
                                    let parameter = chunk[1];
                                    rom::Modcard56Effect {
                                        id,
                                        name: {
                                            if let Ok(parts) = rom::text::parse_entry(
                                                &mapper.get(byteorder::LittleEndian::read_u32(
                                                    &mapper
                                                        .get(offsets.modcard_details_names_pointer)
                                                        [..4],
                                                )),
                                                id as usize,
                                                &TEXT_PARSE_OPTIONS,
                                            ) {
                                                parts
                                                    .into_iter()
                                                    .flat_map(|part| {
                                                        match part {
                                                            rom::text::Part::Literal(c) => charset
                                                                .get(c)
                                                                .unwrap_or(&"�")
                                                                .to_string(),
                                                            rom::text::Part::Command {
                                                                op: PRINT_VAR_COMMAND,
                                                                params,
                                                            } => {
                                                                if params[2] == 1 {
                                                                    let mut parameter =
                                                                        parameter as u32;
                                                                    if id == 0x00 || id == 0x02 {
                                                                        parameter = parameter * 10;
                                                                    }
                                                                    format!("{}", parameter)
                                                                } else {
                                                                    "".to_string()
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
                                        parameter,
                                        is_debuff: chunk[2] == 1,
                                        is_ability: id > 0x15,
                                    }
                                })
                                .collect::<Vec<_>>(),
                        }
                    }))
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap(),
                )
            } else {
                None
            },
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

    fn modcard56(&self, id: usize) -> Option<&rom::Modcard56> {
        self.modcards56
            .as_ref()
            .and_then(|modcards56| modcards56.get(id))
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "RV", "BX", "EX", "SP", "FZ", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "-", "×", "=", ":", "%", "?", "+", "█", "[bat]", "ー", "!", "&", ",", "゜", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "�", "_", "ƶ", "[L]", "[B]", "[R]", "[A]", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "[END]", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "容", "量", "全", "木", "[MB]", "無", "現", "実", "[circle]", "×", "緑", "道", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "来", "日", "目", "月", "獣", "各", "人", "入", "出", "山", "口", "光", "電", "気", "綾", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "高", "朗", "枚", "野", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", "究", "門", "城", "王", "兄", "化", "葉", "行", "街", "屋", "水", "見", "終", "新", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "代", "年", "火", "改", "計", "画", "職", "体", "波", "回", "外", "地", "員", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "晴", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "教", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "向", "犬", "々", "ヶ", "連", "射", "舟", "戸", "切", "土", "炎", "伊", "夫", "鉄", "国", "男", "天", "老", "師", "堀", "杉", "士", "悟", "森", "霧", "麻", "剛", "垣", "★", "[bracket1]", "[bracket2]", "[.]"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "-", "×", "=", ":", "%", "?", "+", "■", "[bat]", "ー", "!", "RV", "BX", "&", "、", "゜", ".", "・", ";", "’", "\"", "~", "/", "(", ")", "「", "」", "EX", "SP", "FZ", "�", "_", "ƶ", "周", "え", "お", "う", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "[END]", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "全", "木", "[MB]", "無", "現", "実", "[circle]", "×", "緑", "道", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "来", "日", "目", "月", "獣", "各", "人", "入", "出", "山", "口", "光", "電", "気", "綾", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "高", "朗", "枚", "野", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", "究", "門", "城", "王", "兄", "化", "葉", "行", "街", "屋", "水", "見", "終", "新", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "代", "年", "火", "改", "計", "画", "職", "体", "波", "回", "外", "地", "員", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "晴", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "教", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "向", "犬", "々", "ヶ", "連", "射", "舟", "戸", "切", "土", "炎", "伊", "夫", "鉄", "国", "男", "天", "老", "師", "堀", "杉", "士", "悟", "森", "霧", "麻", "剛", "垣"];
