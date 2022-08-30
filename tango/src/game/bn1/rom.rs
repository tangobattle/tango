use byteorder::ByteOrder;

use crate::rom;

pub struct Offsets {
    chip_data: usize,
    chip_names_pointer: usize,
    chip_icon_palette_pointer: usize,
    element_icon_palette_pointer: usize,
    element_icons_pointer: usize,
}

#[rustfmt::skip]
pub static AREE_00: Offsets = Offsets {
    chip_data:                      0x08007d70,
    chip_names_pointer:             0x080145f4,
    element_icons_pointer:          0x0801a688,
    element_icon_palette_pointer:   0x08005a1c,
    chip_icon_palette_pointer:      0x08015ebc,
};

#[rustfmt::skip]
pub static AREJ_00: Offsets = Offsets {
    chip_data:                      0x08007d3c,
    chip_names_pointer:             0x08014578,
    element_icons_pointer:          0x0801a5a4,
    element_icon_palette_pointer:   0x08005a0c,
    chip_icon_palette_pointer:      0x08015e40,
};

lazy_static! {
    pub static ref TEXT_PARSE_OPTIONS: rom::text::ParseOptions =
        rom::text::ParseOptions::new(0xe5, 0xe7).with_command(0xe8, 0);
}

pub struct Assets {
    element_icons: [image::RgbaImage; 5],
    chips: [rom::Chip; 240],
}

impl Assets {
    pub fn new(
        offsets: &Offsets,
        charset: &'static [&'static str],
        rom: &[u8],
        _wram: &[u8],
    ) -> Self {
        let chip_icon_palette = {
            let pointer = offsets.chip_icon_palette_pointer & !0x08000000;
            let offset = (byteorder::LittleEndian::read_u32(&rom[pointer..pointer + 4])
                & !0x08000000) as usize;
            rom::read_palette(&rom[offset..offset + 32])
        };

        Self {
            element_icons: {
                let palette = {
                    let pointer = offsets.element_icon_palette_pointer & !0x08000000;
                    let offset = (byteorder::LittleEndian::read_u32(&rom[pointer..pointer + 4])
                        & !0x08000000) as usize;
                    rom::read_palette(&rom[offset..offset + 32])
                };
                {
                    let pointer = offsets.element_icons_pointer & !0x08000000;
                    let offset = (byteorder::LittleEndian::read_u32(&rom[pointer..pointer + 4])
                        & !0x08000000) as usize;
                    (0..5)
                        .map(|i| {
                            rom::apply_palette(
                                rom::read_merged_tiles(
                                    &rom[offset + (i * rom::TILE_BYTES * 4)
                                        ..offset + ((i + 1) * rom::TILE_BYTES * 4)],
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
            chips: (0..240)
                .map(|i| {
                    let offset = (offsets.chip_data & !0x08000000) + i * 0x1c;
                    let buf = &rom[offset..offset + 0x1c];
                    rom::Chip {
                        name: {
                            let pointer = offsets.chip_names_pointer & !0x08000000;
                            let offset =
                                (byteorder::LittleEndian::read_u32(&rom[pointer..pointer + 4])
                                    & !0x08000000) as usize;

                            if let Ok(parts) =
                                rom::text::parse_entry(&rom[offset..], i, &TEXT_PARSE_OPTIONS)
                            {
                                parts
                                    .into_iter()
                                    .flat_map(|part| {
                                        match part {
                                            rom::text::Part::Literal(c) => charset[c],
                                            _ => "",
                                        }
                                        .chars()
                                    })
                                    .collect::<String>()
                            } else {
                                "???".to_string()
                            }
                        },
                        icon: {
                            let offset = (byteorder::LittleEndian::read_u32(&buf[0x10..0x10 + 4])
                                & !0x08000000) as usize;
                            rom::apply_palette(
                                rom::read_merged_tiles(
                                    &rom[offset..offset + rom::TILE_BYTES * 4],
                                    2,
                                )
                                .unwrap(),
                                &chip_icon_palette,
                            )
                        },
                        codes: buf[0x00..0x05].iter().cloned().collect(),
                        element: buf[0x05] as usize,
                        class: rom::ChipClass::Standard,
                        dark: false,
                        mb: 0,
                        damage: byteorder::LittleEndian::read_u16(&buf[0x0c..0x0c + 2]) as u32,
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

    fn element_icon(&self, id: usize) -> Option<&image::RgbaImage> {
        self.element_icons.get(id)
    }
}

#[rustfmt::skip]
pub const EN_CHARSET: &'static [&'static str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "[Lv.]", "[11]", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "-", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "[.]", "[×]", "[=]", "[:]", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "!", "‼", "?", "“", "„", "#", "♭", "$", "%", "&", "'", "(", ")", "~", "^", "\"", "∧", "∨", "<", ">", ",", "。", ".", "・", "/", "\\\\", "_", "「", "」", "\\[", "\\]", "[bracket1]", "[bracket2]", "⊂", "⊃", "∩", "[raindrop]", "↑", "→", "↓", "←", "∀", "α", "β", "@", "★", "♥", "♪", "℃", "♂", "♀", "＿", "｜", "￣", ":", ";", "…", "¥", "+", "×", "÷", "=", "※", "*", "○", "●", "◎", "□", "■", "◇", "◆", "△", "▲", "▽", "▼", "▶", "◀", "☛", "¼", "[infinity1]", "[infinity2]"];

#[rustfmt::skip]
pub const JA_CHARSET: &'static [&'static str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヰ", "ヱ", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "ー", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "!", "‼", "?", "“", "„", "#", "♭", "$", "%", "&", "'", "(", ")", "~", "^", "\"", "∧", "∨", "<", ">", "、", "。", ".", "・", "/", "\\\\", "_", "「", "」", "\\[", "\\]", "[bracket1]", "[bracket2]", "⊂", "⊃", "∩", "[raindrop]", "↑", "→", "↓", "←", "∀", "α", "β", "@", "★", "♥", "♪", "℃", "♂", "♀", "＿", "｜", "￣", ":", ";", "…", "¥", "+", "×", "÷", "=", "※", "*", "○", "●", "◎", "□", "■", "◇", "◆", "△", "▲", "▽", "▼", "▶", "◀", "☛", "止", "彩", "起", "父", "博", "士", "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "百", "千", "万", "億", "上", "下", "左", "右", "手", "足", "日", "目", "月", "顔", "頭", "人", "入", "出", "山", "口", "光", "電", "気", "話", "広", "雨", "名", "前", "学", "校", "保", "健", "室", "世", "界", "体", "育", "館", "信", "号", "機", "器", "大", "小", "中", "自", "分", "間", "開", "閉", "問", "聞", "門", "熱", "斗", "要", "住", "道", "行", "街", "屋", "水", "見", "家", "教", "走", "先", "生", "長", "今", "事", "点", "女", "子", "言", "会", "来", "¼", "[infinity1]", "[infinity2]", "思", "時", "円", "知", "毎", "年", "火", "朝", "計", "画", "休", "曜", "帰", "回", "外", "多", "考", "正", "死", "値", "合", "戦", "争", "秋", "原", "町", "天", "用", "金", "男", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "度", "以", "楽", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "岩", "才", "者", "立", "官", "庁", "ヶ", "連", "射", "国", "局", "耳", "土", "炎", "伊", "集", "院", "各", "科", "省", "祐", "朗", "枚", "永", "川", "花", "兄", "茶"];
