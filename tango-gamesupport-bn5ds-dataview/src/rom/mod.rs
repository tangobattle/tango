//! The DS cart's chip assets: stats, names, descriptions, icons and
//! element icons — what the folder view draws from.
//!
//! The port keeps the GBA game's data shapes almost wholesale. The chip
//! table's first 0x20 bytes are byte-identical to BN5's (found by
//! searching the cart for the GBA entries' leading bytes); the entry
//! shrinks from 0x2c to 0x28 by replacing the GBA's three art pointers
//! with one RAM icon pointer plus a pair of indexes into the artwork
//! banks — two cart files holding every chip's tiles and palettes
//! back-to-back, in 0x10-byte units. Names and descriptions are GBA
//! text archives, decoded by BN5's own charsets.
//!
//! Addresses come in two kinds, told apart by value: `0x02xxxxxx` is
//! main-RAM inside the static ARM9 image (mapped through the cart
//! header's load parameters), anything below is a plain file offset
//! into the cart image — the element art lives in data files, found by
//! searching for the GBA sheets' bytes, and nothing that useful points
//! at them from the static binary.

mod msg;

pub struct Offsets {
    /// The chip stat table (RAM): [`NUM_CHIPS`](super::NUM_CHIPS)
    /// entries of 0x28 bytes.
    chip_data: u32,
    /// Array of text-archive pointers (RAM), one per 0x100 chip ids —
    /// the game's own literal pool holds names and descriptions as four
    /// consecutive words, so the description array is this plus 8.
    chip_names_pointers: u32,
    chip_descriptions_pointers: u32,
    /// Pointers (RAM) to the two archives that name whoever is standing
    /// on the field: the navi roster, whose first entry is the player's
    /// own MegaMan, and the enemy list, which runs the viruses and ends
    /// on the two crosses. The game keeps them as consecutive words.
    navi_names_pointer: u32,
    enemy_names_pointer: u32,
    /// The shared 16-color icon palette (RAM), byte-identical to the
    /// GBA game's — which is how it was found, so it is addressed as
    /// data rather than through a pointer chain.
    chip_icon_palette: u32,
    /// The element icon sheet and its palette (file offsets): 4bpp
    /// tiles, one 16x16 icon per element, same bytes as the GBA sheet.
    element_icons: u32,
    element_icon_palette: u32,
    /// The chip artwork banks (file offsets): every chip's 56x48 art
    /// tiles in one cart file and their palettes in another, each
    /// indexed by [`RawChip`]'s pair in 0x10-byte units. The bytes are
    /// the GBA game's own art — which is how the banks were found.
    chip_art: u32,
    chip_art_palettes: u32,
}

#[rustfmt::skip]
pub static A5TE_00: Offsets = Offsets {
    chip_data:                  0x0203e9e8,
    chip_names_pointers:        0x020cecf8,
    chip_descriptions_pointers: 0x020ced00,
    navi_names_pointer:         0x020057b4,
    enemy_names_pointer:        0x020057b0,
    chip_icon_palette:          0x020fbf88,
    element_icons:              0x0088_6200,
    element_icon_palette:       0x0088_6a00,
    chip_art:                   0x00b7_f400,
    chip_art_palettes:          0x00b7_cc00,
};

#[rustfmt::skip]
pub static A5TJ_00: Offsets = Offsets {
    chip_data:                  0x0203e7c0,
    chip_names_pointers:        0x020cd734,
    chip_descriptions_pointers: 0x020cd73c,
    navi_names_pointer:         0x02005764,
    enemy_names_pointer:        0x02005760,
    chip_icon_palette:          0x020fa8ac,
    element_icons:              0x0099_7a00,
    element_icon_palette:       0x0099_8200,
    chip_art:                   0x00ce_6200,
    chip_art_palettes:          0x00ce_3a00,
};

/// Resolves the two address kinds against the cart image: `0x02xxxxxx`
/// through the ARM9 static image the header describes, lower values as
/// file offsets. Anything unmappable (a pointer into heap, a truncated
/// image) yields an empty slice, and the readers treat short data as
/// missing rather than panicking.
struct Mapper {
    rom: Vec<u8>,
    arm9_rom_offset: usize,
    arm9_ram_addr: u32,
    arm9_len: usize,
}

impl Mapper {
    fn new(rom: Vec<u8>) -> Self {
        let word = |offset: usize| {
            rom.get(offset..offset + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .unwrap_or(0)
        };
        let arm9_rom_offset = word(0x20) as usize;
        let arm9_ram_addr = word(0x28);
        let arm9_len = word(0x2c) as usize;
        Self {
            rom,
            arm9_rom_offset,
            arm9_ram_addr,
            arm9_len,
        }
    }

    fn get(&self, start: u32) -> &[u8] {
        let range = if start >= self.arm9_ram_addr && start < self.arm9_ram_addr + self.arm9_len as u32 {
            self.arm9_rom_offset + (start - self.arm9_ram_addr) as usize..self.arm9_rom_offset + self.arm9_len
        } else if (start as usize) < self.rom.len() && start < self.arm9_ram_addr {
            start as usize..self.rom.len()
        } else {
            return &[];
        };
        self.rom.get(range).unwrap_or(&[])
    }

    fn read_u32(&self, addr: u32) -> Option<u32> {
        Some(u32::from_le_bytes(self.get(addr).get(..4)?.try_into().unwrap()))
    }
}

fn read_palette(mapper: &Mapper, addr: u32) -> tango_gamesupport_common::dataview::rom::Palette {
    mapper
        .get(addr)
        .get(..std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>())
        .map(bytemuck::pod_read_unaligned)
        .unwrap_or([Default::default(); 16])
}

pub struct Assets {
    offsets: &'static Offsets,
    msg_parser: msg::Parser,
    mapper: Mapper,
    chip_icon_palette: tango_gamesupport_common::dataview::rom::Palette,
    element_icon_palette: tango_gamesupport_common::dataview::rom::Palette,
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[&str], rom: Vec<u8>) -> Self {
        let mapper = Mapper::new(rom);
        let chip_icon_palette = read_palette(&mapper, offsets.chip_icon_palette);
        let element_icon_palette = read_palette(&mapper, offsets.element_icon_palette);
        Self {
            offsets,
            msg_parser: msg::parser(charset),
            mapper,
            chip_icon_palette,
            element_icon_palette,
        }
    }

    /// Decode entry `index` of the text archive `pointer` points at,
    /// keeping only its text. `None` when either address runs out of
    /// mappable range, or the entry does not decode.
    fn text(&self, pointer: u32, index: usize) -> Option<String> {
        let archive = self.mapper.read_u32(pointer)?;
        let entry = tango_gamesupport_common::dataview::msg::get_entry(self.mapper.get(archive), index)?;

        Some(
            self.msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common::dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    /// What the cart calls the MegaMan a save with this cross brings —
    /// `MegaMan`/`SCMegaMn`/`BCMegaMn`, `ロックマン`/`SCロックマン`/`FCロックマン`.
    ///
    /// Plain MegaMan is the navi roster's first entry, the name every
    /// other game in the series shows for the player's own navi. The
    /// crosses are not on that roster — the only place the cart writes
    /// them down is where it names what stands on the field, so they
    /// come off the end of the enemy list.
    pub fn cross_name(&self, cross: crate::save::Cross) -> Option<String> {
        let (pointer, index) = match cross {
            crate::save::Cross::None => (self.offsets.navi_names_pointer, MEGAMAN_NAVI_INDEX),
            crate::save::Cross::BassProto | crate::save::Cross::BassColonel => {
                (self.offsets.enemy_names_pointer, BASS_CROSS_ENEMY_INDEX)
            }
            crate::save::Cross::Sol => (self.offsets.enemy_names_pointer, SOL_CROSS_ENEMY_INDEX),
        };
        self.text(pointer, index).filter(|name| !name.is_empty())
    }

    /// What the cart calls the navi leading `team`, the way its own file
    /// select names a file: `ProtoMan`/`Colonel`, ブルース/カーネル. Off the
    /// same roster as plain MegaMan — the two are the roster's first
    /// entries after him, each team's leader.
    pub fn leader_name(&self, team: u8) -> Option<String> {
        let index = if team == 0 {
            PROTOMAN_NAVI_INDEX
        } else {
            COLONEL_NAVI_INDEX
        };
        self.text(self.offsets.navi_names_pointer, index)
            .filter(|name| !name.is_empty())
    }
}

/// Where [`Assets::cross_name`] and [`Assets::leader_name`] read each
/// name. The two crosses are the last two entries of the enemy list,
/// after the viruses and the game's own modified MegaMan; both BassCross
/// values share one name, as the pair is a difference of team rather
/// than of who shows up.
const MEGAMAN_NAVI_INDEX: usize = 0;
const PROTOMAN_NAVI_INDEX: usize = 1;
const COLONEL_NAVI_INDEX: usize = 37;
const BASS_CROSS_ENEMY_INDEX: usize = 234;
const SOL_CROSS_ENEMY_INDEX: usize = 235;

struct Chip<'a> {
    id: usize,
    assets: &'a Assets,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy, c2rust_bitfields::BitfieldStruct)]
#[allow(dead_code)]
struct RawChip {
    codes: [u8; 4],
    _attack_element: u8,
    _rarity: u8,
    element: u8,
    class: u8,
    mb: u8,

    #[bitfield(name = "dark", ty = "bool", bits = "5..=5")]
    effect_flags: [u8; 1],

    _counter_settings: u8,
    _attack_family: u8,
    _attack_subfamily: u8,
    _dark_soul_usage_behavior: u8,
    /// 0x0e..0x18: the GBA entry's lock-on/attack-params/delay/karma
    /// stretch, shuffled slightly on DS and unread here.
    _unk_0e: [u8; 10],
    _alphabet_sort: u16,
    attack_power: u16,
    library_sort_order: u16,
    _unk_1e: [u8; 2],
    icon_ptr: u32,
    /// Replaces the GBA's image+palette pointers: offsets into the
    /// [`Offsets::chip_art`] and [`Offsets::chip_art_palettes`] banks,
    /// in 0x10-byte units. Chips sharing artwork share `art_tiles` (the
    /// Cannon family), exactly where the GBA versions shared their
    /// image pointer.
    art_tiles: u16,
    art_palette: u16,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x28);
const _: () = assert!(std::mem::offset_of!(RawChip, attack_power) == 0x1a);
const _: () = assert!(std::mem::offset_of!(RawChip, icon_ptr) == 0x20);
const _: () = assert!(std::mem::offset_of!(RawChip, art_tiles) == 0x24);

impl<'a> Chip<'a> {
    fn raw(&'a self) -> RawChip {
        bytemuck::pod_read_unaligned(
            &self.assets.mapper.get(self.assets.offsets.chip_data)[self.id * std::mem::size_of::<RawChip>()..]
                [..std::mem::size_of::<RawChip>()],
        )
    }

    /// Decode the msg entry for this chip out of the per-0x100-ids
    /// archive array at `pointers`, keeping only its text.
    fn text(&self, pointers: u32) -> Option<String> {
        self.assets
            .text(pointers + ((self.id / 0x100) * 4) as u32, self.id % 0x100)
    }

    /// This chip's artwork, from the art banks. `None` when an index
    /// runs out of the cart image — the trait method renders those
    /// blank instead of panicking.
    fn try_image(&self) -> Option<image::RgbaImage> {
        let raw = self.raw();
        let tiles = self.assets.mapper.get(self.assets.offsets.chip_art + raw.art_tiles as u32 * 0x10);
        let paletted = tango_gamesupport_common::dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common::dataview::rom::TILE_BYTES * 7 * 6)?,
            7,
        )
        .ok()?;
        let palette = self
            .assets
            .mapper
            .get(self.assets.offsets.chip_art_palettes + raw.art_palette as u32 * 0x10)
            .get(..std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>())
            .map(bytemuck::pod_read_unaligned::<tango_gamesupport_common::dataview::rom::Palette>)?;
        Some(tango_gamesupport_common::dataview::rom::apply_palette(
            paletted, &palette,
        ))
    }

    /// This chip's list icon. `None` when the pointer or sheet runs out
    /// of mappable range — the trait method renders those blank instead
    /// of panicking.
    fn try_icon(&self) -> Option<image::RgbaImage> {
        let raw = self.raw();
        let tiles = self.assets.mapper.get(raw.icon_ptr);
        let paletted = tango_gamesupport_common::dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common::dataview::rom::TILE_BYTES * 2 * 2)?,
            2,
        )
        .ok()?;
        Some(tango_gamesupport_common::dataview::rom::apply_palette(
            paletted,
            &self.assets.chip_icon_palette,
        ))
    }
}

impl<'a> tango_gamesupport_common::dataview::rom::Chip for Chip<'a> {
    fn name(&self) -> Option<String> {
        self.text(self.assets.offsets.chip_names_pointers)
    }

    fn description(&self) -> Option<String> {
        self.text(self.assets.offsets.chip_descriptions_pointers)
    }

    fn icon(&self) -> image::RgbaImage {
        self.try_icon().unwrap_or_else(|| {
            image::RgbaImage::new(
                (2 * tango_gamesupport_common::dataview::rom::TILE_WIDTH) as u32,
                (2 * tango_gamesupport_common::dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }

    fn image(&self) -> image::RgbaImage {
        self.try_image().unwrap_or_else(|| {
            image::RgbaImage::new(
                (7 * tango_gamesupport_common::dataview::rom::TILE_WIDTH) as u32,
                (6 * tango_gamesupport_common::dataview::rom::TILE_HEIGHT) as u32,
            )
        })
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

    fn class(&self) -> tango_gamesupport_common::dataview::rom::ChipClass {
        let raw = self.raw();
        match raw.class {
            0 => tango_gamesupport_common::dataview::rom::ChipClass::Standard,
            1 => tango_gamesupport_common::dataview::rom::ChipClass::Mega,
            2 => tango_gamesupport_common::dataview::rom::ChipClass::Giga,
            4 => tango_gamesupport_common::dataview::rom::ChipClass::ProgramAdvance,
            _ => tango_gamesupport_common::dataview::rom::ChipClass::None,
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

impl tango_gamesupport_common::dataview::rom::Assets for Assets {
    fn chip(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common::dataview::rom::Chip + '_>> {
        if id >= self.num_chips() {
            return None;
        }
        Some(Box::new(Chip { id, assets: self }))
    }

    fn num_chips(&self) -> usize {
        super::NUM_CHIPS
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= 13 {
            return None;
        }

        let buf = self.mapper.get(self.offsets.element_icons);
        let paletted = tango_gamesupport_common::dataview::rom::read_merged_tiles(
            buf.get(id * tango_gamesupport_common::dataview::rom::TILE_BYTES * 4..)?
                .get(..tango_gamesupport_common::dataview::rom::TILE_BYTES * 2 * 2)?,
            2,
        )
        .ok()?;
        Some(tango_gamesupport_common::dataview::rom::apply_palette(
            paletted,
            &self.element_icon_palette,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal cart image: header load params plus an ARM9 segment.
    fn cart(arm9: &[u8]) -> Vec<u8> {
        let mut rom = vec![0u8; 0x4000 + arm9.len()];
        rom[0x20..0x24].copy_from_slice(&0x4000u32.to_le_bytes());
        rom[0x28..0x2c].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        rom[0x2c..0x30].copy_from_slice(&(arm9.len() as u32).to_le_bytes());
        rom[0x4000..].copy_from_slice(arm9);
        rom
    }

    #[test]
    fn maps_ram_through_the_arm9_image() {
        let mapper = Mapper::new(cart(&[1, 2, 3, 4]));
        assert_eq!(mapper.get(0x0200_0001), &[2, 3, 4]);
        // Past the static image is heap, not cart data.
        assert_eq!(mapper.get(0x0200_0004), &[] as &[u8]);
    }

    #[test]
    fn maps_low_addresses_as_file_offsets() {
        let mapper = Mapper::new(cart(&[9, 8, 7]));
        assert_eq!(mapper.get(0x4001), &[8, 7]);
        assert_eq!(mapper.get(0x0900_0000), &[] as &[u8]);
    }
}

// The DS build renders text with the GBA games's own encoding; the US
// charset is BN5's verbatim (chip names on the cart decode with it).
#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "-", "×", "=", ":", "%", "?", "+", "█", "[bat]", "ー", "!", "SP", "DS", "&", ",", "。", ".", "・", ";", "'", "\"", "~", "/", "(", ")", "「", "」", "α", "β", "Ω", "■", "_", "[z]", "周", "え", "お", "う", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "究", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "Ω", "←", "↓", "木", "[MB]", "無", "現", "実", "[circle]", "[cross]", "#", "⋯", "不", "止", "彩", "\\[", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "\\]", "<", "人", "入", "出", "山", "口", "光", "電", "気", "助", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "燃", "朗", "枚", "島", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", ">", "$", "城", "王", "兄", "化", "行", "街", "屋", "水", "見", "終", "丁", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "年", "火", "改", "計", "画", "体", "波", "回", "外", "地", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "所", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "立", "々", "ヶ", "連", "射", "綾", "切", "土", "炎", "伊"];

// The JP cart's is BN5's with two nine-entry runs traded: A–I sit where
// the GBA game keeps its small hiragana (0xe4, above the escape
// threshold and so two bytes a character), and the small hiragana where
// it keeps A–I (0x5e). JP text is full of ゃゅょっ and nearly free of
// letters, so the trade shortens it — at the price of two bytes apiece
// for the F, C and S that spell FC/SCロックマン. Derived by lining this
// cart's chip text up against the GBA cart's, entry for entry: 10124
// characters agree on the trade and none contradict it.
//
// The two � in the kanji run are BN5's, and carry over from it: slots
// whose glyph nothing here can name, because the JP script never uses
// either. They are not padding — leaving them out is what made every
// kanji past 化 decode one place late and every kanji past 少 two.
#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ウ", "ア", "イ", "オ", "エ", "ケ", "コ", "カ", "ク", "キ", "セ", "サ", "ソ", "シ", "ス", "テ", "ト", "ツ", "タ", "チ", "ネ", "ノ", "ヌ", "ナ", "ニ", "ヒ", "ヘ", "ホ", "ハ", "フ", "ミ", "マ", "メ", "ム", "モ", "ヤ", "ヨ", "ユ", "ロ", "ル", "リ", "レ", "ラ", "ン", "熱", "斗", "ワ", "ヲ", "ギ", "ガ", "ゲ", "ゴ", "グ", "ゾ", "ジ", "ゼ", "ズ", "ザ", "デ", "ド", "ヅ", "ダ", "ヂ", "ベ", "ビ", "ボ", "バ", "ブ", "ピ", "パ", "ペ", "プ", "ポ", "ゥ", "ァ", "ィ", "ォ", "ェ", "ュ", "ヴ", "ッ", "ョ", "ャ", "ぅ", "ぁ", "ぃ", "ぉ", "ぇ", "ゅ", "ょ", "っ", "ゃ", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "*", "-", "×", "=", ":", "%", "?", "+", "■", "[bat]", "ー", "!", "SP", "DS", "&", "、", "゜", ".", "・", ";", "’", "\"", "~", "/", "(", ")", "「", "」", "V2", "V3", "V4", "V5", "_", "[z]", "周", "え", "お", "う", "あ", "い", "け", "く", "き", "こ", "か", "せ", "そ", "す", "さ", "し", "つ", "と", "て", "た", "ち", "ね", "の", "な", "ぬ", "に", "へ", "ふ", "ほ", "は", "ひ", "め", "む", "み", "も", "ま", "ゆ", "よ", "や", "る", "ら", "り", "ろ", "れ", "究", "ん", "を", "わ", "研", "げ", "ぐ", "ご", "が", "ぎ", "ぜ", "ず", "じ", "ぞ", "ざ", "で", "ど", "づ", "だ", "ぢ", "べ", "ば", "び", "ぼ", "ぶ", "ぽ", "ぷ", "ぴ", "ぺ", "ぱ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "全", "木", "[MB]", "無", "現", "実", "[circle]", "[cross]", "緑", "尺", "不", "止", "彩", "起", "父", "集", "院", "一", "二", "三", "四", "五", "六", "七", "八", "陽", "十", "百", "千", "万", "脳", "上", "下", "左", "右", "手", "足", "日", "目", "月", "玉", "各", "人", "入", "出", "山", "口", "光", "電", "気", "助", "科", "次", "名", "前", "学", "校", "省", "祐", "室", "世", "界", "燃", "朗", "枚", "島", "悪", "路", "闇", "大", "小", "中", "自", "分", "間", "系", "花", "問", "異", "門", "城", "王", "兄", "化", "�", "行", "街", "屋", "水", "見", "終", "丁", "桜", "先", "生", "長", "今", "了", "点", "井", "子", "言", "太", "属", "風", "会", "性", "持", "時", "勝", "赤", "毎", "年", "火", "改", "計", "画", "休", "体", "波", "回", "外", "地", "病", "正", "造", "値", "合", "戦", "川", "秋", "原", "町", "所", "用", "金", "郎", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "�", "以", "白", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "明", "才", "者", "立", "泉", "々", "ヶ", "連", "射", "国", "綾", "切", "土", "炎", "伊"];
