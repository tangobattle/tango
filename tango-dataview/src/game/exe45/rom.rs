mod msg;

pub struct Offsets {
    chip_data: u32,
    chip_names_pointers: u32,
    chip_descriptions_pointers: u32,
    chip_icon_palette_pointer: u32,
    element_icon_palette_pointer: u32,
    element_icons_pointer: u32,
    navi_names_pointer: u32,
    emblem_icons_pointer: u32,
    emblem_icon_palette_pointer: u32,
}

#[rustfmt::skip]
pub static BR4J_00: Offsets = Offsets {
    chip_data:                      0x0801af0c,
    chip_icon_palette_pointer:      0x080168ec,
    chip_names_pointers:            0x0803cb98,
    chip_descriptions_pointers:     0x0802165c,
    element_icons_pointer:          0x080d4c94,
    element_icon_palette_pointer:   0x080d4ca0,
    navi_names_pointer:             0x0805174c,
    emblem_icons_pointer:          0x08021a50,
    emblem_icon_palette_pointer:   0x080219f4,
};

pub struct Assets {
    offsets: &'static Offsets,
    msg_parser: msg::Parser,
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

struct Navi<'a> {
    id: usize,
    assets: &'a Assets,
}

impl<'a> crate::rom::Navi for Navi<'a> {
    fn name(&self) -> Option<String> {
        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.navi_names_pointer)[..std::mem::size_of::<u32>()],
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

    fn emblem(&self) -> image::RgbaImage {
        crate::rom::apply_palette(
            crate::rom::read_merged_tiles(
                &self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
                    &self.assets.mapper.get(self.assets.offsets.emblem_icons_pointer)
                        [self.id * std::mem::size_of::<u32>()..][..std::mem::size_of::<u32>()],
                ))[..crate::rom::TILE_BYTES * 4],
                2,
            )
            .unwrap(),
            &crate::rom::read_palette(
                &self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
                    &self.assets.mapper.get(self.assets.offsets.emblem_icon_palette_pointer)
                        [self.id * std::mem::size_of::<u32>()..][..std::mem::size_of::<u32>()],
                ))[..32],
            ),
        )
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

    fn regular_chip_is_in_place(&self) -> bool {
        false
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

    fn navi<'a>(&'a self, id: usize) -> Option<Box<dyn crate::rom::Navi + 'a>> {
        if id >= self.num_navis() {
            return None;
        }
        Some(Box::new(Navi { id, assets: self }))
    }

    fn num_navis(&self) -> usize {
        super::NUM_NAVIS
    }
}

#[rustfmt::skip]
pub const CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "熱", "斗", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "-", "×", "=", ":", "%", "?", "+", "÷", "�", "ー", "!", "現", "実", "&", "、", "。", ".", "・", ";", "’", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "_", "[z]", "周", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "研", "究", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "内", "木", "[MB]", "無", "嵐", "[square]", "[circle]", "[cross]", "駅", "客", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "高", "各", "人", "入", "出", "山", "口", "光", "電", "気", "♯", "科", "$", "名", "前", "学", "校", "省", "¥", "室", "世", "界", "約", "朗", "枚", "女", "男", "路", "束", "大", "小", "中", "自", "分", "間", "村", "予", "問", "異", "門", "決", "定", "兄", "帯", "道", "行", "街", "屋", "水", "見", "終", "丁", "週", "先", "生", "長", "今", "了", "点", "緑", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "毎", "年", "火", "改", "計", "画", "休", "体", "波", "回", "外", "地", "病", "正", "造", "値", "合", "戦", "敗", "秋", "原", "町", "所", "用", "金", "習", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "■", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "立", "泉", "々", "ヶ", "連", "射", "国", "綾", "切", "土", "炎", "伊"];
