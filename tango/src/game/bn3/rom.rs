use byteorder::ByteOrder;

use crate::rom;

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
}

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
};

pub struct Assets {
    element_icons: [image::RgbaImage; 5],
    chips: [rom::Chip; 374],
    navicust_parts: [rom::NavicustPart; 204],
    styles: [rom::Style; 40],
}

impl Assets {
    pub fn new(offsets: &Offsets, charset: &[&str], rom: &[u8], wram: &[u8]) -> Self {
        let text_parse_options = rom::text::ParseOptions {
            charset,
            extension_ops: 0xe5..=0xe6,
            eof_op: 0xe7,
            newline_op: 0xe8,
            commands: std::collections::HashMap::new(),
        };

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
                    let buf = &buf[0x1e0..];
                    (0..5)
                        .map(|i| {
                            rom::apply_palette(
                                rom::read_merged_tiles(&buf[i * rom::TILE_BYTES * 4..(i + 1) * rom::TILE_BYTES * 4], 2)
                                    .unwrap(),
                                &palette,
                            )
                        })
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap()
                }
            },
            chips: (0..374)
                .map(|i| {
                    let buf = &mapper.get(offsets.chip_data)[i * 0x20..(i + 1) * 0x20];
                    let flags = buf[0x13];
                    rom::Chip {
                        name: {
                            let pointer = offsets.chip_names_pointers + ((i / 0x100) * 4) as u32;
                            let i = i % 0x100;

                            if let Ok(parts) = rom::text::parse_entry(
                                &mapper.get(byteorder::LittleEndian::read_u32(&mapper.get(pointer)[..4])),
                                i,
                                &text_parse_options,
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
                        },
                        description: {
                            let pointer = offsets.chip_descriptions_pointers + ((i / 0x100) * 4) as u32;
                            let i = i % 0x100;

                            if let Ok(parts) = rom::text::parse_entry(
                                &mapper.get(byteorder::LittleEndian::read_u32(&mapper.get(pointer)[..4])),
                                i,
                                &text_parse_options,
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
                        },
                        icon: rom::apply_palette(
                            rom::read_merged_tiles(
                                &mapper.get(byteorder::LittleEndian::read_u32(&buf[0x14..0x14 + 4]))
                                    [..rom::TILE_BYTES * 4],
                                2,
                            )
                            .unwrap(),
                            &chip_icon_palette,
                        ),
                        codes: buf[0x00..0x06].iter().cloned().filter(|code| *code != 0xff).collect(),
                        element: buf[0x06] as usize,
                        class: if flags & 0x02 != 0 {
                            rom::ChipClass::Giga
                        } else if flags & 0x01 != 0 {
                            rom::ChipClass::Mega
                        } else {
                            rom::ChipClass::Standard
                        },
                        dark: false,
                        mb: buf[0x0a],
                        damage: {
                            let damage = byteorder::LittleEndian::read_u16(&buf[0x0c..0x0c + 2]) as u32;
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
            navicust_parts: (0..204)
                .map(|i| {
                    let buf = &mapper.get(offsets.ncp_data)[i * 0x10..(i + 1) * 0x10];
                    rom::NavicustPart {
                        name: {
                            if let Ok(parts) = rom::text::parse_entry(
                                &mapper.get(byteorder::LittleEndian::read_u32(
                                    &mapper.get(offsets.ncp_names_pointer)[..4],
                                )),
                                i / 4,
                                &text_parse_options,
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
                        },
                        description: if let Ok(parts) = rom::text::parse_entry(
                            &mapper.get(byteorder::LittleEndian::read_u32(
                                &mapper.get(offsets.ncp_descriptions_pointer)[..4],
                            )),
                            i / 4,
                            &text_parse_options,
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
                        },
                        color: [
                            None,
                            Some(rom::NavicustPartColor::White),
                            Some(rom::NavicustPartColor::Pink),
                            Some(rom::NavicustPartColor::Yellow),
                            Some(rom::NavicustPartColor::Red),
                            Some(rom::NavicustPartColor::Blue),
                            Some(rom::NavicustPartColor::Green),
                            Some(rom::NavicustPartColor::Orange),
                            Some(rom::NavicustPartColor::Purple),
                            Some(rom::NavicustPartColor::Gray),
                        ][buf[0x03] as usize]
                            .clone(),
                        is_solid: buf[0x01] == 0,
                        compressed_bitmap: image::ImageBuffer::from_vec(
                            5,
                            5,
                            mapper.get(byteorder::LittleEndian::read_u32(&buf[0x08..0x0c]))[..49].to_vec(),
                        )
                        .unwrap(),
                        uncompressed_bitmap: image::ImageBuffer::from_vec(
                            5,
                            5,
                            mapper.get(byteorder::LittleEndian::read_u32(&buf[0x0c..0x10]))[..49].to_vec(),
                        )
                        .unwrap(),
                    }
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            styles: (0..40)
                .map(|id| {
                    let typ = id >> 3;
                    let element = id & 0x7;

                    rom::Style {
                        name: {
                            if let Ok(parts) = rom::text::parse_entry(
                                &mapper.get(byteorder::LittleEndian::read_u32(
                                    &mapper.get(offsets.key_items_names_pointer)[..4],
                                )),
                                128 + typ * 5 + element,
                                &text_parse_options,
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
                        },
                        extra_ncp_color: [
                            None,
                            Some(rom::NavicustPartColor::Red),
                            Some(rom::NavicustPartColor::Blue),
                            Some(rom::NavicustPartColor::Green),
                            Some(rom::NavicustPartColor::Blue),
                            Some(rom::NavicustPartColor::Green),
                            Some(rom::NavicustPartColor::Red),
                            Some(rom::NavicustPartColor::Gray),
                        ][typ as usize]
                            .clone(),
                    }
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        }
    }
}

impl rom::Assets for Assets {
    fn chip(&self, id: usize) -> Option<&rom::Chip> {
        self.chips.get(id)
    }

    fn num_chips(&self) -> usize {
        self.chips.len()
    }

    fn element_icon(&self, id: usize) -> Option<&image::RgbaImage> {
        self.element_icons.get(id)
    }

    fn navicust_part(&self, id: usize, variant: usize) -> Option<&rom::NavicustPart> {
        self.navicust_parts.get(id * 4 + variant)
    }

    fn num_navicust_parts(&self) -> (usize, usize) {
        (self.navicust_parts.len() / 4, 4)
    }

    fn style(&self, id: usize) -> Option<&rom::Style> {
        self.styles.get(id)
    }

    fn num_styles(&self) -> usize {
        self.styles.len()
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "-", "×", "=", ":", "+", "÷", "※", "*", "!", "?", "%", "&", ",", "⋯", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "@", "♥", "♪", "[MB]", "■", "_", "[circle1]", "[circle2]", "[cross1]", "[cross2]", "[bracket1]", "[bracket2]", "[ModTools1]", "[ModTools2]", "[ModTools3]", "Σ", "Ω", "α", "β", "#", "…", ">", "<", "エ", "[BowneGlobal1]", "[BowneGlobal2]", "[BowneGlobal3]", "[BowneGlobal4]", "[BowneGlobal5]", "[BowneGlobal6]", "[BowneGlobal7]", "[BowneGlobal8]", "[BowneGlobal9]", "[BowneGlobal10]", "[BowneGlobal11]"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "Σ", "Ω", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "ー", "×", "=", ":", "?", "+", "÷", "※", "*", "!", "[?]", "%", "&", "、", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "@", "♥", "♪", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "ヰ", "ヱ", "[MB]", "■", "_", "[circle1]", "[circle2]", "[cross1]", "[cross2]", "[bracket1]", "[bracket2]", "[ModTools1]", "[ModTools2]", "[ModTools3]", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "止", "彩", "起", "父", "博", "士", "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "磁", "真", "人", "入", "出", "山", "口", "光", "電", "気", "話", "広", "王", "名", "前", "学", "校", "渡", "職", "室", "世", "界", "員", "管", "理", "局", "島", "機", "器", "大", "小", "中", "自", "分", "間", "村", "感", "問", "異", "門", "熱", "斗", "要", "常", "道", "行", "街", "屋", "水", "見", "終", "教", "走", "先", "生", "長", "今", "了", "点", "女", "子", "言", "会", "来", "風", "吹", "速", "思", "時", "円", "知", "毎", "年", "火", "朝", "計", "画", "休", "体", "波", "回", "外", "多", "病", "正", "死", "値", "合", "戦", "争", "秋", "原", "町", "天", "用", "金", "男", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "砂", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "岩", "才", "者", "立", "泉", "々", "ヶ", "連", "射", "国", "綾", "耳", "土", "炎", "伊", "集", "院", "各", "科", "省", "祐", "朗", "枚", "路", "川", "花", "兄", "帯", "音", "属", "性", "持", "勝", "赤", "犬", "飼", "荒", "丁", "駒", "地", "所", "明", "切", "急", "木", "無", "高", "駅", "店", "不", "研", "究"];
