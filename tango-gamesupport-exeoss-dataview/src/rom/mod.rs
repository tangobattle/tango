//! The cart's chip assets: stats, names, descriptions, icons and
//! artwork — what the folder view draws from.
//!
//! The remake carries BN1's chip list forward almost untouched. The
//! stat entry keeps the GBA game's field order and grows from 0x1c to
//! 0x20 (a word inserted before the attack power, and the artwork's
//! overlay id appended); ids line up with BN1's for the whole of its
//! list, and OSS adds its own on the end — the Star Force crossover
//! chips at 200/201 and two more Program Advances, for 240 in all. The
//! names and descriptions are BN1's text archives byte for byte, which
//! is how they were found, so BN1's charset decodes them.
//!
//! Nothing here is addressed the way [`super::super`]'s sibling DS game
//! does it. BN5DS reads its chip data straight out of the ARM9 static
//! image; this cart's static image stops well short of any of it, and
//! every asset lives in an overlay — the table and text in overlay 0,
//! the icon bank at the tail of overlay 9, and **one overlay per chip**
//! for the artwork, 186 of them sharing a single load slot. So the
//! offsets below name an overlay as well as an address, and artwork is
//! reached through the id the stat entry carries. See
//! [`nds`](tango_gamesupport_common_dataview::nds) for the decoding
//! that makes an overlay readable at all.
//!
//! The element icons are the cart's own art rather than BN1's, so
//! unlike the chip icons (which are BN1's tiles with the palette
//! indices shuffled) no search of the GBA sheet finds them. They were
//! found by driving the game to its own folder screen, lifting the
//! 14x14 shape it drew off the screenshot, and hunting main RAM for a
//! 2x2-tile cell holding it — which lands in overlay 10, the folder's
//! own module, in BN1's order: null, elec, fire, aqua, wood.

mod msg;

use tango_gamesupport_common_dataview::nds;
use tango_gamesupport_common_dataview::rom::LegalChips;

const LEGAL_CHIPS: LegalChips = LegalChips::from_ranges(&[
    1..=34,
    37..=46,
    49..=52,
    55..=55,
    58..=65,
    67..=74,
    76..=76,
    79..=80,
    82..=88,
    91..=95,
    97..=103,
    105..=119,
    121..=142,
    145..=201,
]);

pub struct Offsets {
    legal_chips: LegalChips,
    /// The overlay holding the chip table and the text archives — the
    /// game's main data overlay.
    chip_data_overlay: u16,
    /// The chip stat table: [`NUM_CHIPS`](super::NUM_CHIPS) entries of
    /// 0x20 bytes.
    chip_data: u32,
    /// The name and description text archives, one entry per chip id.
    /// One archive each, unlike BN5DS's per-0x100-ids arrays: this
    /// game's list fits in the 256 entries a single archive holds.
    chip_names: u32,
    chip_descriptions: u32,
    /// The overlay holding the chip icon bank, which sits at its very
    /// end — the last icon finishes 12 bytes short of the overlay's.
    chip_icon_overlay: u16,
    /// The shared 16-colour icon palette, immediately before the bank.
    chip_icon_palette: u32,
    /// The overlay holding the folder screen — where the element icons
    /// live, sheet and palette both. It is not the icon overlay: the
    /// two load at the same address and are never resident together.
    element_icon_overlay: u16,
    /// Five 16x16 icons back to back, in the element order the chip
    /// table's field indexes, and the palette that colours them (one of
    /// three byte-identical copies on the cart; this is the one in the
    /// same overlay as the sheet).
    element_icons: u32,
    element_icon_palette: u32,
    /// What a stat entry's artwork id is above the overlay it names.
    /// The ids are resource numbers rather than overlay numbers; the
    /// gap between the two is constant across all 240 chips.
    chip_art_overlay_bias: u16,
}

#[rustfmt::skip]
pub static B6XJ_00: Offsets = Offsets {
    legal_chips:           LEGAL_CHIPS,
    chip_data_overlay:     0,
    chip_data:             0x02082e44,
    chip_names:            0x020b017c,
    chip_descriptions:     0x020ae644,
    chip_icon_overlay:     9,
    chip_icon_palette:     0x022ac214,
    element_icon_overlay:  10,
    element_icons:         0x021ae490,
    element_icon_palette:  0x02232698,
    chip_art_overlay_bias: 6,
};

/// How many elements the chip table's field indexes, and how many icons
/// the sheet holds: BN1's five — null, elec, fire, aqua, wood.
const NUM_ELEMENTS: usize = 5;

fn read_palette(data: &[u8]) -> Option<tango_gamesupport_common_dataview::rom::Palette> {
    data.get(..std::mem::size_of::<tango_gamesupport_common_dataview::rom::Palette>())
        .map(bytemuck::pod_read_unaligned)
}

pub struct Assets {
    offsets: &'static Offsets,
    msg_parser: msg::Parser,
    /// The cart itself, kept so a chip's artwork overlay can be decoded
    /// when it is asked for rather than all 186 up front.
    cart: nds::Cart,
    data: Option<nds::Overlay>,
    icons: Option<nds::Overlay>,
    elements: Option<nds::Overlay>,
    chip_icon_palette: tango_gamesupport_common_dataview::rom::Palette,
    element_icon_palette: tango_gamesupport_common_dataview::rom::Palette,
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[&str], rom: Vec<u8>) -> Self {
        let cart = nds::Cart::new(rom);
        let data = cart.overlay(offsets.chip_data_overlay);
        let icons = cart.overlay(offsets.chip_icon_overlay);
        let elements = cart.overlay(offsets.element_icon_overlay);
        let palette = |overlay: &Option<nds::Overlay>, addr| {
            overlay
                .as_ref()
                .and_then(|o| read_palette(o.get(addr)))
                .unwrap_or([Default::default(); 16])
        };
        let chip_icon_palette = palette(&icons, offsets.chip_icon_palette);
        let element_icon_palette = palette(&elements, offsets.element_icon_palette);
        Self {
            offsets,
            msg_parser: msg::parser(charset),
            cart,
            data,
            icons,
            elements,
            chip_icon_palette,
            element_icon_palette,
        }
    }

    /// The main data overlay, or an empty slice for a ROM whose
    /// overlays don't decode — every reader below treats short data as
    /// missing.
    fn data(&self, addr: u32) -> &[u8] {
        self.data.as_ref().map(|o| o.get(addr)).unwrap_or(&[])
    }
}

/// A code byte as the letter the game prints. The remake adds a
/// wildcard the GBA game has none of, and puts it at 27 rather than the
/// 26 the later GBA games use — 39 chips gained it as their last code,
/// and Barrier has it as its only one.
fn code_char(code: u8) -> Option<char> {
    match code {
        0..=25 => Some((b'A' + code) as char),
        27 => Some('*'),
        _ => None,
    }
}

struct Chip<'a> {
    id: usize,
    assets: &'a Assets,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
#[allow(dead_code)]
struct RawChip {
    codes: [u8; 5],
    element: u8,
    family: u8,
    _subfamily: u8,
    rarity: u8,
    library_number: u8,
    _unk_0a: u8,
    _unk_0b: u8,
    /// The word the DS entry inserts ahead of the attack power. It
    /// sorts the chips into a handful of coarse kinds (the cannons read
    /// 1, the swords 2, the bombs 4) and nothing here reads it.
    _unk_0c: u16,
    attack_power: u16,
    _unk_10: u16,
    /// Which overlay holds this chip's artwork, biased by
    /// [`Offsets::chip_art_overlay_bias`]. It replaces nothing in the
    /// GBA entry — the art *pointers* below survive, but they are the
    /// slot every artwork overlay loads into and so read the same for
    /// every chip; this is the part that differs.
    art_overlay: u16,
    icon_ptr: u32,
    image_ptr: u32,
    palette_ptr: u32,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x20);
const _: () = assert!(std::mem::offset_of!(RawChip, attack_power) == 0x0e);
const _: () = assert!(std::mem::offset_of!(RawChip, art_overlay) == 0x12);
const _: () = assert!(std::mem::offset_of!(RawChip, icon_ptr) == 0x14);

impl<'a> Chip<'a> {
    fn raw(&self) -> Option<RawChip> {
        Some(bytemuck::pod_read_unaligned(
            self.assets
                .data(self.assets.offsets.chip_data)
                .get(self.id * std::mem::size_of::<RawChip>()..)?
                .get(..std::mem::size_of::<RawChip>())?,
        ))
    }

    /// Decode this chip's entry out of the archive at `archive`,
    /// keeping only its text.
    fn text(&self, archive: u32) -> Option<String> {
        let region = self.assets.data(archive);
        // `get_entry` indexes the offset table rather than probing it,
        // so a region too short to hold this id's pair of offsets is
        // ours to reject.
        if region.len() < (self.id + 2) * std::mem::size_of::<u16>() {
            return None;
        }
        let entry = tango_gamesupport_common_dataview::msg::get_entry(region, self.id)?;

        Some(
            self.assets
                .msg_parser
                .parse(entry)
                .ok()?
                .into_iter()
                .flat_map(|part| {
                    match part {
                        tango_gamesupport_common_dataview::msg::Chunk::Text(s) => s,
                        _ => "".to_string(),
                    }
                    .chars()
                    .collect::<Vec<_>>()
                })
                .collect::<String>(),
        )
    }

    /// This chip's list icon, out of the bank at the end of the icon
    /// overlay. `None` when the pointer runs out of it — the trait
    /// method renders those blank instead of panicking.
    fn try_icon(&self) -> Option<image::RgbaImage> {
        let raw = self.raw()?;
        let tiles = self.assets.icons.as_ref()?.get(raw.icon_ptr);
        let paletted = tango_gamesupport_common_dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common_dataview::rom::TILE_BYTES * 2 * 2)?,
            2,
        )
        .ok()?;
        Some(tango_gamesupport_common_dataview::rom::apply_palette(
            paletted,
            &self.assets.chip_icon_palette,
        ))
    }

    /// This chip's artwork, out of the overlay its entry names — tiles
    /// and palette both, at the addresses the entry's own (otherwise
    /// chip-invariant) pointers give. Chips that share artwork share
    /// the overlay, exactly where the GBA versions shared an image
    /// pointer.
    fn try_image(&self) -> Option<image::RgbaImage> {
        let raw = self.raw()?;
        // Copied out first: a packed field can't be borrowed, which is
        // what a method call on it would do.
        let art_overlay = raw.art_overlay;
        let overlay = self
            .assets
            .cart
            .overlay(art_overlay.checked_sub(self.assets.offsets.chip_art_overlay_bias)?)?;
        let paletted = tango_gamesupport_common_dataview::rom::read_merged_tiles(
            overlay
                .get(raw.image_ptr)
                .get(..tango_gamesupport_common_dataview::rom::TILE_BYTES * 8 * 7)?,
            8,
        )
        .ok()?;
        let palette = read_palette(overlay.get(raw.palette_ptr))?;
        Some(tango_gamesupport_common_dataview::rom::apply_palette(
            paletted, &palette,
        ))
    }
}

impl tango_gamesupport_common_dataview::rom::Chip for Chip<'_> {
    fn name(&self) -> Option<String> {
        self.text(self.assets.offsets.chip_names)
    }

    fn description(&self) -> Option<String> {
        self.text(self.assets.offsets.chip_descriptions)
    }

    fn icon(&self) -> image::RgbaImage {
        self.try_icon().unwrap_or_else(|| {
            image::RgbaImage::new(
                (2 * tango_gamesupport_common_dataview::rom::TILE_WIDTH) as u32,
                (2 * tango_gamesupport_common_dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }

    fn image(&self) -> image::RgbaImage {
        self.try_image().unwrap_or_else(|| {
            image::RgbaImage::new(
                (8 * tango_gamesupport_common_dataview::rom::TILE_WIDTH) as u32,
                (7 * tango_gamesupport_common_dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }

    fn codes(&self) -> Vec<char> {
        self.raw()
            .into_iter()
            .flat_map(|raw| raw.codes)
            .filter_map(code_char)
            .collect()
    }

    fn element(&self) -> usize {
        self.raw().map(|raw| raw.element as usize).unwrap_or(0)
    }

    fn class(&self) -> tango_gamesupport_common_dataview::rom::ChipClass {
        let Some(raw) = self.raw() else {
            return tango_gamesupport_common_dataview::rom::ChipClass::None;
        };
        if raw.family == 0x40 {
            // Family 0x40 is the Navi-chip family (PharoMan … Bass, plus
            // the two the crossover adds), ids 148-201.
            tango_gamesupport_common_dataview::rom::ChipClass::Navi
        } else if self.id != 0 && raw.library_number == 0xff && raw.rarity != 0xff {
            // Program-advance result chips (Z-Cannon … 2xHero, ids
            // 202-239): not registered in the library, but real chips
            // (rarity != 0xff, unlike the blank slots) and not the
            // Buster (id 0).
            tango_gamesupport_common_dataview::rom::ChipClass::ProgramAdvance
        } else {
            tango_gamesupport_common_dataview::rom::ChipClass::Standard
        }
    }

    fn dark(&self) -> bool {
        false
    }

    fn mb(&self) -> u8 {
        0
    }

    fn attack_power(&self) -> u32 {
        self.raw().map(|raw| raw.attack_power as u32).unwrap_or(0)
    }

    fn library_sort_order(&self) -> Option<usize> {
        Some(self.id)
    }
}

impl tango_gamesupport_common_dataview::rom::Assets for Assets {
    fn chip_is_legal(&self, id: usize) -> bool {
        self.offsets.legal_chips.contains(id)
    }

    fn chip(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common_dataview::rom::Chip + '_>> {
        if id >= self.num_chips() {
            return None;
        }
        Some(Box::new(Chip { id, assets: self }))
    }

    fn num_chips(&self) -> usize {
        super::NUM_CHIPS
    }

    /// BN1's chips carry no MB cost, and the remake's don't either —
    /// the stat entry has no field for one.
    fn chips_have_mb(&self) -> bool {
        false
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= NUM_ELEMENTS {
            return None;
        }
        let sheet = self.elements.as_ref()?.get(self.offsets.element_icons);
        let paletted = tango_gamesupport_common_dataview::rom::read_merged_tiles(
            sheet
                .get(id * tango_gamesupport_common_dataview::rom::TILE_BYTES * 4..)?
                .get(..tango_gamesupport_common_dataview::rom::TILE_BYTES * 2 * 2)?,
            2,
        )
        .ok()?;
        Some(tango_gamesupport_common_dataview::rom::apply_palette(
            paletted,
            &self.element_icon_palette,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_read_as_the_letters_the_game_prints() {
        assert_eq!(code_char(0), Some('A'));
        assert_eq!(code_char(25), Some('Z'));
        assert_eq!(code_char(27), Some('*'));
        // 26 is unused by this game, and 0xff is the empty code slot.
        assert_eq!(code_char(26), None);
        assert_eq!(code_char(0xff), None);
    }

    #[test]
    fn a_rom_that_decodes_to_nothing_still_answers() {
        use tango_gamesupport_common_dataview::rom::Assets as _;
        let assets = Assets::new(&B6XJ_00, JA_CHARSET, vec![0; 0x1000]);
        assert_eq!(assets.num_chips(), super::super::NUM_CHIPS);
        let chip = assets.chip(1).unwrap();
        assert_eq!(chip.name(), None);
        assert_eq!(chip.codes(), Vec::<char>::new());
        assert_eq!(chip.attack_power(), 0);
        // Blank rather than a panic, at the sizes the folder lays out.
        assert_eq!(chip.icon().dimensions(), (16, 16));
        assert_eq!(chip.image().dimensions(), (64, 56));
        assert!(assets.element_icon(0).is_none());
    }

    #[test]
    fn there_are_only_five_elements() {
        use tango_gamesupport_common_dataview::rom::Assets as _;
        let assets = Assets::new(&B6XJ_00, JA_CHARSET, vec![0; 0x1000]);
        assert!(assets.element_icon(NUM_ELEMENTS).is_none());
    }
}

// The remake renders text with the GBA game's own encoding; this is
// BN1's Japanese charset verbatim (the chip name and description
// archives on the cart are BN1's bytes, and decode with it). There is
// no English charset here: the cart shipped in Japan only.
#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヰ", "ヱ", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "ー", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "!", "‼", "?", "“", "„", "#", "♭", "$", "%", "&", "'", "(", ")", "~", "^", "\"", "∧", "∨", "<", ">", "、", "。", ".", "・", "/", "\\\\", "_", "「", "」", "\\[", "\\]", "[bracket1]", "[bracket2]", "⊂", "⊃", "∩", "[raindrop]", "↑", "→", "↓", "←", "∀", "α", "β", "@", "★", "♥", "♪", "℃", "♂", "♀", "＿", "｜", "￣", ":", ";", "…", "¥", "+", "×", "÷", "=", "※", "*", "○", "●", "◎", "□", "■", "◇", "◆", "△", "▲", "▽", "▼", "▶", "◀", "☛", "止", "彩", "起", "父", "博", "士", "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "百", "千", "万", "億", "上", "下", "左", "右", "手", "足", "日", "目", "月", "顔", "頭", "人", "入", "出", "山", "口", "光", "電", "気", "話", "広", "雨", "名", "前", "学", "校", "保", "健", "室", "世", "界", "体", "育", "館", "信", "号", "機", "器", "大", "小", "中", "自", "分", "間", "開", "閉", "問", "聞", "門", "熱", "斗", "要", "住", "道", "行", "街", "屋", "水", "見", "家", "教", "走", "先", "生", "長", "今", "事", "点", "女", "子", "言", "会", "来", "¼", "[infinity1]", "[infinity2]", "思", "時", "円", "知", "毎", "年", "火", "朝", "計", "画", "休", "曜", "帰", "回", "外", "多", "考", "正", "死", "値", "合", "戦", "争", "秋", "原", "町", "天", "用", "金", "男", "作", "数", "方", "社", "攻", "撃", "力", "同", "武", "何", "発", "少", "度", "以", "楽", "早", "暮", "面", "組", "後", "文", "字", "本", "階", "岩", "才", "者", "立", "官", "庁", "ヶ", "連", "射", "国", "局", "耳", "土", "炎", "伊", "集", "院", "各", "科", "省", "祐", "朗", "枚", "永", "川", "花", "兄", "茶", "音", "属", "性", "持", "勝", "赤", "充", "池", "停", "丁", "舎", "地", "所", "明", "切", "急", "木", "無", "高", "駅", "店", "闘", "絵", "球", "研", "究", "香"];
