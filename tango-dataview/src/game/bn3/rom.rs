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
    codes: [u8; 6],
    element: u8,
    _family: u8,
    _subfamily: u8,
    _rarity: u8,
    mb: u8,
    _unk_0a: u8,
    attack_power: u16,
    library_number: u16,
    _unk_0e: [u8; 3],

    #[bitfield(name = "giga", ty = "bool", bits = "1..=1")]
    #[bitfield(name = "mega", ty = "bool", bits = "0..=0")]
    flags: [u8; 1],

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

impl<'a> crate::rom::Chip for Chip<'a> {
    fn name(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_names_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = crate::msg::get_entry(&region, id)?;

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

    fn description(&self) -> Option<String> {
        let pointer = self.assets.offsets.chip_descriptions_pointers + ((self.id / 0x100) * 4) as u32;
        let id = self.id % 0x100;

        let region = self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = crate::msg::get_entry(&region, id)?;

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

    fn icon(&self) -> image::RgbaImage {
        let raw = self.raw();
        crate::rom::apply_palette(
            crate::rom::read_merged_tiles(
                &self.assets.mapper.get(raw.icon_ptr)[..crate::rom::TILE_BYTES * 2 * 2],
                2,
            )
            .unwrap(),
            &self.assets.chip_icon_palette,
        )
    }

    fn image(&self) -> image::RgbaImage {
        let raw = self.raw();
        crate::rom::apply_palette(
            crate::rom::read_merged_tiles(
                &self.assets.mapper.get(raw.image_ptr)[..crate::rom::TILE_BYTES * 8 * 7],
                8,
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
        if raw.giga() {
            crate::rom::ChipClass::Giga
        } else if raw.mega() {
            crate::rom::ChipClass::Mega
        } else {
            crate::rom::ChipClass::Standard
        }
    }

    fn dark(&self) -> bool {
        false
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
        Some(raw.library_number as usize)
    }
}

struct NavicustPart<'a> {
    id: usize,
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
        bytemuck::pod_read_unaligned(
            &self.assets.mapper.get(self.assets.offsets.ncp_data)[self.id * std::mem::size_of::<RawNavicustPart>()..]
                [..std::mem::size_of::<RawNavicustPart>()],
        )
    }
}

impl<'a> crate::rom::NavicustPart for NavicustPart<'a> {
    fn name(&self) -> Option<String> {
        let region = &self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.ncp_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = crate::msg::get_entry(region, self.id / 4)?;

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

    fn description(&self) -> Option<String> {
        let region = &self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.ncp_descriptions_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = crate::msg::get_entry(region, self.id / 4)?;

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
            7 => crate::rom::NavicustPartColor::Orange,
            8 => crate::rom::NavicustPartColor::Purple,
            9 => crate::rom::NavicustPartColor::Gray,
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
    pub fn new(offsets: &'static Offsets, charset: &[&str], rom: Vec<u8>, wram: Vec<u8>) -> Self {
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

struct Style<'a> {
    id: usize,
    assets: &'a Assets,
}

#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy, c2rust_bitfields::BitfieldStruct)]
struct RawStyle {
    #[bitfield(name = "element", ty = "u8", bits = "0..=2")]
    #[bitfield(name = "typ", ty = "u8", bits = "3..=7")]
    type_and_element: [u8; 1],
}

impl<'a> crate::rom::Style for Style<'a> {
    fn name(&self) -> Option<String> {
        let raw = bytemuck::cast::<_, RawStyle>(self.id as u8);

        let region = &self.assets.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.assets.mapper.get(self.assets.offsets.key_items_names_pointer)[..std::mem::size_of::<u32>()],
        ));
        let entry = crate::msg::get_entry(region, 128 + raw.typ() as usize * 5 + raw.element() as usize)?;

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

    fn extra_ncp_color(&self) -> Option<crate::rom::NavicustPartColor> {
        let raw = bytemuck::cast::<_, RawStyle>(self.id as u8);
        Some(match raw.typ() {
            1 => crate::rom::NavicustPartColor::Red,
            2 => crate::rom::NavicustPartColor::Blue,
            3 => crate::rom::NavicustPartColor::Green,
            4 => crate::rom::NavicustPartColor::Blue,
            5 => crate::rom::NavicustPartColor::Green,
            6 => crate::rom::NavicustPartColor::Red,
            7 => crate::rom::NavicustPartColor::Gray,
            _ => {
                return None;
            }
        })
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
        if id >= 5 {
            return None;
        }

        let buf = self.mapper.get(bytemuck::pod_read_unaligned::<u32>(
            &self.mapper.get(self.offsets.element_icons_pointer)[..std::mem::size_of::<u32>()],
        ));
        let buf = &buf[0x1e0..];
        Some(crate::rom::apply_palette(
            crate::rom::read_merged_tiles(
                &buf[id * crate::rom::TILE_BYTES * 4..][..crate::rom::TILE_BYTES * 2 * 2],
                2,
            )
            .unwrap(),
            &self.element_icon_palette,
        ))
    }

    fn navicust_part(&self, id: usize) -> Option<Box<dyn crate::rom::NavicustPart + '_>> {
        if id >= self.num_navicust_parts() {
            return None;
        }
        Some(Box::new(NavicustPart { id, assets: self }))
    }

    fn num_navicust_parts(&self) -> usize {
        super::NUM_NAVICUST_PARTS
    }

    fn style<'a>(&'a self, id: usize) -> Option<Box<dyn crate::rom::Style + 'a>> {
        if id >= self.num_styles() {
            return None;
        }
        Some(Box::new(Style { id, assets: self }))
    }

    fn num_styles(&self) -> usize {
        super::NUM_STYLES
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
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "-", "×", "=", ":", "+", "÷", "※", "*", "!", "?", "%", "&", ",", "⋯", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "@", "♥", "♪", "[MB]", "■", "_", "[circle1]", "[circle2]", "[cross1]", "[cross2]", "[bracket1]", "[bracket2]", "[ModTools1]", "[ModTools2]", "[ModTools3]", "Σ", "Ω", "α", "β", "#", "…", ">", "<", "エ", "[BowneGlobal1]", "[BowneGlobal2]", "[BowneGlobal3]", "[BowneGlobal4]", "[BowneGlobal5]", "[BowneGlobal6]", "[BowneGlobal7]", "[BowneGlobal8]", "[BowneGlobal9]", "[BowneGlobal10]", "[BowneGlobal11]"];

#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "Σ", "Ω", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "ー", "×", "=", ":", "?", "+", "÷", "※", "*", "!", "[?]", "%", "&", "、", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "@", "♥", "♪", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "ヰ", "ヱ", "[MB]", "■", "_", "[circle1]", "[circle2]", "[cross1]", "[cross2]", "[bracket1]", "[bracket2]", "[ModTools1]", "[ModTools2]", "[ModTools3]", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "止", "彩", "起", "父", "博", "士", "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "磁", "真", "人", "入", "出", "山", "口", "光", "電", "気", "話", "広", "王", "名", "前", "学", "校", "渡", "職", "室", "世", "界", "員", "管", "理", "局", "島", "機", "器", "大", "小", "中", "自", "分", "間", "村", "感", "問", "異", "門", "熱", "斗", "要", "常", "道", "行", "街", "屋", "水", "見", "終", "教", "走", "先", "生", "長", "今", "了", "点", "女", "子", "言", "会", "来", "風", "吹", "速", "思", "時", "円", "知", "毎", "年", "火", "朝", "計", "画", "休", "体", "波", "回", "外", "多", "病", "正", "死", "値", "合", "戦", "争", "秋", "原", "町", "天", "用", "金", "男", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "砂", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "岩", "才", "者", "立", "泉", "々", "ヶ", "連", "射", "国", "綾", "耳", "土", "炎", "伊", "集", "院", "各", "科", "省", "祐", "朗", "枚", "路", "川", "花", "兄", "帯", "音", "属", "性", "持", "勝", "赤", "犬", "飼", "荒", "丁", "駒", "地", "所", "明", "切", "急", "木", "無", "高", "駅", "店", "不", "研", "究"];
