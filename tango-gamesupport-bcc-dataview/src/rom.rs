//! ROM assets: the chip table and its names.
//!
//! # Text
//!
//! A string is a `u16` array terminated by `0x80NN` (`NN` = how many
//! characters preceded it). A character is the whole halfword indexed
//! into the region's charset ([`EN_CHARSET`] / [`JA_CHARSET`]), not a
//! byte — the lowercase run crosses 0x100, so `v`..`z` are
//! 0x100..0x104. Strings drawn in the game's second font (the chip
//! ranks, dialogue) add 0x600 to every code, so decoding folds that
//! page away before the lookup.
//!
//! # Chips
//!
//! [`Offsets::chip_data`] is a flat 16-byte-per-id table holding
//! [`NUM_CHIPS`] entries — the bound the game's own accessors check
//! (`cmp #0xf8`). Everything else is reached through a `*_pointer`: the
//! address of the ROM word the game's own code loads to find that
//! table, so a patch that relocates the data still reads right. The
//! name/description/artwork pointers are the literals in the accessor
//! cluster next to the chip-data one; the icon pointers are entries in
//! the resource descriptor the loader walks.

use byteorder::ByteOrder as _;

use super::NUM_CHIPS;
use tango_gamesupport_common_dataview::rom::LegalChips;

// The bundled blank starter template has no nonzero pack counts.
const LEGAL_CHIPS: LegalChips = LegalChips::NONE;

/// The character sets, indexed by character code — one font per region.
///
/// Read off the running games rather than guessed: a probe ROM repoints
/// a chip's P.DATA description lines at a scratch string, so any code
/// can be put on screen and its glyph read out of the emulator's
/// framebuffer. Sweeping every code that way showed the two fonts are
/// the same table apart from ten slots — the US font puts card suits at
/// 0x0b..0x0f where the JP font has アイウエオ, `,` at 0x85 for JP's 、,
/// and `_[]` at 0x99..0x9b where the JP font has あいう.
///
/// **Codes at 0x110 and above are not a fixed alphabet.** The same code
/// spells different kanji in different messages — the JP script writes
/// 光熱斗 as 0x128,0x129,0x12a in one message and 0x157,0x178,0x179 in
/// another, and those codes' glyphs differ — so the game must swap the
/// kanji block per message. What Tango renders is chip names, ability
/// lines and descriptions; the first two use no kanji at all, and every
/// kanji the descriptions use is in the block below, read from real
/// P.DATA boxes. Dialogue kanji are deliberately left undecoded: no
/// single table can be right for them.
///
/// A few unused symbol slots (0x7f, 0x82, 0x108) stay undecoded too —
/// no text exercises them and their 8×13 shapes are not worth guessing.
#[rustfmt::skip]
pub const EN_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "♠", "♥", "♦", "♣", "★", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヰ", "ヱ", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "ー", "×", "=", ":", "?", "+", "÷", "�", "*", "!", "�", "%", "&", ",", "。", ".", "・", ";", "’", "”", "~", "/", "(", ")", "「", "」", "↑", "→", "↓", "←", "@", "♥", "♪", "_", "[", "]", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "#", "�", "-", "█", "★", "4", "5"];
#[rustfmt::skip]
pub const JA_CHARSET: &[&str] = &[" ", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヰ", "ヱ", "ヲ", "ン", "ガ", "ギ", "グ", "ゲ", "ゴ", "ザ", "ジ", "ズ", "ゼ", "ゾ", "ダ", "ヂ", "ヅ", "デ", "ド", "バ", "ビ", "ブ", "ベ", "ボ", "パ", "ピ", "プ", "ペ", "ポ", "ァ", "ィ", "ゥ", "ェ", "ォ", "ッ", "ャ", "ュ", "ョ", "ヴ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "ー", "×", "=", ":", "?", "+", "÷", "�", "*", "!", "�", "%", "&", "、", "。", ".", "・", ";", "’", "”", "~", "/", "(", ")", "「", "」", "↑", "→", "↓", "←", "@", "♥", "♪", "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん", "が", "ぎ", "ぐ", "げ", "ご", "ざ", "じ", "ず", "ぜ", "ぞ", "だ", "ぢ", "づ", "で", "ど", "ば", "び", "ぶ", "べ", "ぼ", "ぱ", "ぴ", "ぷ", "ぺ", "ぽ", "ぁ", "ぃ", "ぅ", "ぇ", "ぉ", "っ", "ゃ", "ゅ", "ょ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "容", "量", "#", "�", "-", "█", "★", "4", "5", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "列", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "上", "下", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "電", "気", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "中", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "水", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "回", "外", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "数", "�", "�", "攻", "撃", "力", "�", "�", "�", "�", "�", "�", "以", "�", "�", "�", "�", "�", "後", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "�", "炎", "�", "�", "�", "�", "�", "�", "�", "�", "枚", "�", "�", "�", "�", "�", "�", "属", "性", "�", "�", "�", "全", "命", "率", "�", "�", "�", "�", "�", "�", "�", "木", "無"];

/// The page the game's second font sits on: same glyph order, shifted.
const FONT_PAGE: u16 = 0x600;
/// Terminator marker in a string's high byte.
const TERMINATOR: u16 = 0x8000;
/// Bytes per chip stats record.
const CHIP_STATS_SIZE: usize = 0x10;

/// Bytes per artwork record — see [`Offsets::chip_images_pointer`].
const CHIP_GFX_SIZE: usize = 0x14;
/// A chip's artwork: 56 tiles, laid out eight across.
const ART_TILES: usize = 56;
const ART_COLS: usize = 8;
/// Where a [`Palette`](tango_gamesupport_common_dataview::rom::Palette) sits inside an
/// artwork record, and how big one is.
const GFX_PALETTE_FIELD: usize = 0x0c;
const PALETTE_BYTES: usize = 0x20;
/// The elements a chip can carry, in the order the indicator icons are
/// stored: none, fire, aqua, wood, elec. Cross-checked against every
/// chip's own description line in both games.
pub const NUM_ELEMENTS: usize = 5;
/// A list icon: two tiles across, two down, named in reading order by
/// [`Offsets::chip_icon_tilemap_pointer`].
const ICON_SIZE: u32 = 16;
const ICON_COLS: usize = 2;
const ICON_TILES: usize = 4;
/// The tile index lives in the low ten bits of a tilemap entry; the rest
/// would be flip and palette bits, which the icon table never sets.
const TILE_INDEX_MASK: u16 = 0x03ff;

/// How far the icon palettes sit below the icon sheet. Five of them are
/// stored back to back right before the tiles — silver, gold, dark, blue,
/// blank — and the chip list picks between them per row; the first is
/// what it draws a usable chip with. They travel with the sheet, so the
/// palette is taken relative to it rather than pinned on its own.
const ICON_PALETTES_BEFORE_SHEET: u32 = 0xa0;

#[derive(Clone, Copy)]
pub struct Offsets {
    legal_chips: LegalChips,
    /// The chip stats table: `[u16; 8]` per id — max HP at +0x00, price
    /// at +0x02, attack power at +0x04, MB at +0x06, a kind word at
    /// +0x08, the library's display order in the byte at +0x0e and the
    /// artwork variant in the byte at +0x0f. The only table addressed
    /// directly: the game's own accessor loads it as an immediate.
    chip_data: u32,
    /// Points at the pointer table that names each chip id's string.
    chip_names_pointer: u32,
    /// Points at the per-id table of description records; a record is
    /// three string pointers, one per display line of the P.DATA box,
    /// any of which may be null (navis pack the first line into one
    /// string and leave the slot before it null).
    chip_descriptions_pointer: u32,
    /// Points at the tiles the list icons are cut from. Icons are *not*
    /// at a fixed stride in there: identical quarters are shared between
    /// them, so a chip's four tiles are named individually rather than
    /// computed. The icon palettes sit just below (see
    /// [`ICON_PALETTES_BEFORE_SHEET`]).
    chip_icons_pointer: u32,
    /// Points at the tilemap: four entries per chip id, in reading
    /// order, indexing the icon sheet.
    chip_icon_tilemap_pointer: u32,
    /// Points at the artwork table, indexed by a chip's *library order*
    /// rather than its id — chips that share a library slot (Bubbler/
    /// Bub-V/BubCross, say) share one set of tiles and differ only by
    /// palette, which is what the variant byte selects.
    ///
    /// Each record is five words: `[tiles, size, dest, palette, size]`,
    /// where the two offsets are relative to the *field's own address*
    /// (the game does `(&record.palette) + record.palette`, so they
    /// can't be resolved without knowing where the record lives).
    chip_images_pointer: u32,
    /// Points at the element indicators the chip list draws in each row
    /// — [`NUM_ELEMENTS`] icons of 2×2 tiles, in element order (none,
    /// fire, aqua, wood, elec). Its pointer sits in the same resource
    /// descriptor as the chip icons', one entry along.
    element_icons_pointer: u32,
    /// Points at the sixteen colors those indicators are drawn with.
    element_icon_palette_pointer: u32,
}

#[rustfmt::skip]
pub static A89E_00: Offsets = Offsets {
    legal_chips:                 LEGAL_CHIPS,
    chip_data:                  0x0822740c,
    chip_names_pointer:         0x080246bc,
    chip_descriptions_pointer:  0x080246f4,
    chip_icons_pointer:         0x081720bc,
    chip_icon_tilemap_pointer:  0x08170edc,
    chip_images_pointer:        0x080234b8,
    element_icons_pointer:         0x081720c4,
    element_icon_palette_pointer:  0x08009234,
};

#[rustfmt::skip]
pub static A89J_00: Offsets = Offsets {
    legal_chips:                 LEGAL_CHIPS,
    chip_data:                  0x082a1ae0,
    chip_names_pointer:         0x08024420,
    chip_descriptions_pointer:  0x08024458,
    chip_icons_pointer:         0x081cb3b8,
    chip_icon_tilemap_pointer:  0x081ca764,
    chip_images_pointer:        0x0802321c,
    element_icons_pointer:         0x081cb3c0,
    element_icon_palette_pointer:  0x08009014,
};

pub struct Assets {
    offsets: &'static Offsets,
    charset: Vec<String>,
    mapper: tango_gamesupport_common_dataview::rom::MemoryMapper,
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[&str], rom: Vec<u8>, wram: Vec<u8>) -> Self {
        Self {
            offsets,
            charset: charset.iter().map(|s| s.to_string()).collect(),
            mapper: tango_gamesupport_common_dataview::rom::MemoryMapper::new(rom, wram),
        }
    }

    /// The address stored at `pointer` — the ROM word the game's own
    /// code loads to reach a table. Going through these keeps the
    /// reads right if a patch relocates the data.
    fn deref(&self, pointer: u32) -> u32 {
        byteorder::LittleEndian::read_u32(&self.mapper.get(pointer)[..4])
    }

    /// The sixteen-color palette at `addr`.
    fn palette(&self, addr: u32) -> tango_gamesupport_common_dataview::rom::Palette {
        let raw = self.mapper.get(addr);
        std::array::from_fn(|i| {
            tango_gamesupport_common_dataview::rom::Bgr555::new(
                raw.get(i * 2..i * 2 + 2)
                    .and_then(|b| b.try_into().ok())
                    .unwrap_or_default(),
            )
        })
    }

    /// The string at `addr`, decoded until its terminator.
    fn string(&self, addr: u32) -> Option<String> {
        let region = self.mapper.get(addr);
        let mut out = String::new();
        for chunk in region.chunks_exact(2) {
            let v = byteorder::LittleEndian::read_u16(chunk);
            if v & 0xff00 == TERMINATOR {
                return Some(out);
            }
            let code = v.checked_sub(FONT_PAGE).unwrap_or(v) as usize;
            out.push_str(self.charset.get(code).map(|s| s.as_str())?);
        }
        None
    }

    /// Like [`Self::string`], but a code past the table's end decodes
    /// to a replacement char instead of sinking the whole string —
    /// description text is worth showing even with an unpinned glyph
    /// in it.
    fn lenient_string(&self, addr: u32) -> String {
        let region = self.mapper.get(addr);
        let mut out = String::new();
        for chunk in region.chunks_exact(2) {
            let v = byteorder::LittleEndian::read_u16(chunk);
            if v & 0xff00 == TERMINATOR {
                break;
            }
            let code = v.checked_sub(FONT_PAGE).unwrap_or(v) as usize;
            out.push_str(self.charset.get(code).map(|s| s.as_str()).unwrap_or("\u{fffd}"));
        }
        out
    }
}


impl tango_gamesupport_common_dataview::rom::Assets for Assets {
    fn chip_is_legal(&self, id: usize) -> bool {
        self.offsets.legal_chips.contains(id)
    }

    fn chip(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common_dataview::rom::Chip + '_>> {
        Some(Box::new(self.chip_info(id)?))
    }

    fn num_chips(&self) -> usize {
        NUM_CHIPS
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= NUM_ELEMENTS {
            return None;
        }
        let icons = self.mapper.get(self.deref(self.offsets.element_icons_pointer));
        let tiles = icons
            .get(id * tango_gamesupport_common_dataview::rom::TILE_BYTES * ICON_TILES..)?
            .get(..tango_gamesupport_common_dataview::rom::TILE_BYTES * ICON_TILES)?;
        let paletted = tango_gamesupport_common_dataview::rom::read_merged_tiles(tiles, ICON_COLS).ok()?;
        Some(tango_gamesupport_common_dataview::rom::apply_palette(
            paletted,
            &self.palette(self.deref(self.offsets.element_icon_palette_pointer)),
        ))
    }
}

impl Assets {
    /// The concrete chip record. BCC's own UI reads chips through this
    /// — the game's model (chip HP, the deck-capacity MB stat) is
    /// richer than the shared trait, which stays implemented only for
    /// the shared plumbing (icon/artwork baking, the popover).
    pub fn chip_info(&self, id: usize) -> Option<Chip<'_>> {
        (id < NUM_CHIPS).then_some(Chip { assets: self, id })
    }
}

pub struct Chip<'a> {
    assets: &'a Assets,
    id: usize,
}

impl Chip<'_> {
    /// This chip's stats record.
    fn stats(&self) -> [u8; CHIP_STATS_SIZE] {
        let region = self.assets.mapper.get(self.assets.offsets.chip_data);
        region[self.id * CHIP_STATS_SIZE..][..CHIP_STATS_SIZE]
            .try_into()
            .unwrap()
    }

    fn stat_u16(&self, at: usize) -> u16 {
        byteorder::LittleEndian::read_u16(&self.stats()[at..][..2])
    }

    /// This chip's list icon, gathered tile by tile out of the sheet the
    /// way the game's own list builds its background.
    fn try_icon(&self) -> Option<image::RgbaImage> {
        let entries = self
            .assets
            .mapper
            .get(self.assets.deref(self.assets.offsets.chip_icon_tilemap_pointer) + (self.id * ICON_TILES * 2) as u32);
        let sheet_at = self.assets.deref(self.assets.offsets.chip_icons_pointer);
        let sheet = self.assets.mapper.get(sheet_at);
        let mut tiles = Vec::with_capacity(tango_gamesupport_common_dataview::rom::TILE_BYTES * ICON_TILES);
        for entry in entries.get(..ICON_TILES * 2)?.chunks_exact(2) {
            let at = (byteorder::LittleEndian::read_u16(entry) & TILE_INDEX_MASK) as usize
                * tango_gamesupport_common_dataview::rom::TILE_BYTES;
            tiles.extend_from_slice(sheet.get(at..at + tango_gamesupport_common_dataview::rom::TILE_BYTES)?);
        }
        let paletted = tango_gamesupport_common_dataview::rom::read_merged_tiles(&tiles, ICON_COLS).ok()?;
        Some(tango_gamesupport_common_dataview::rom::apply_palette(
            paletted,
            &self.assets.palette(sheet_at - ICON_PALETTES_BEFORE_SHEET),
        ))
    }

    /// This chip's artwork, as the game's own card view draws it: 64×56
    /// pixels from the artwork record its library order names, colored
    /// by the palette its variant byte selects.
    fn try_image(&self) -> Option<image::RgbaImage> {
        let stats = self.stats();
        let record =
            self.assets.deref(self.assets.offsets.chip_images_pointer) + (stats[0x0e] as usize * CHIP_GFX_SIZE) as u32;
        let fields = self.assets.mapper.get(record);
        let tiles_at = record.wrapping_add(byteorder::LittleEndian::read_u32(fields.get(..4)?));
        // The palette offset is relative to its own field, and the
        // variant picks one of the palettes stored back to back there.
        let palette_at = record
            .wrapping_add(GFX_PALETTE_FIELD as u32)
            .wrapping_add(byteorder::LittleEndian::read_u32(
                fields.get(GFX_PALETTE_FIELD..GFX_PALETTE_FIELD + 4)?,
            ))
            .wrapping_add(stats[0x0f] as u32 * PALETTE_BYTES as u32);

        let palette = self.assets.palette(palette_at);
        let tiles = self.assets.mapper.get(tiles_at);
        let paletted = tango_gamesupport_common_dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common_dataview::rom::TILE_BYTES * ART_TILES)?,
            ART_COLS,
        )
        .ok()?;
        Some(tango_gamesupport_common_dataview::rom::apply_palette(
            paletted, &palette,
        ))
    }
}

// The concrete chip API — what BCC's own UI reads. The shared trait
// below delegates here.
impl Chip<'_> {
    pub fn name(&self) -> Option<String> {
        let table = self
            .assets
            .mapper
            .get(self.assets.deref(self.assets.offsets.chip_names_pointer));
        let ptr = byteorder::LittleEndian::read_u32(table.get(self.id * 4..)?.get(..4)?);
        self.assets.string(ptr)
    }

    pub fn description(&self) -> Option<String> {
        // The card screen's three description strings — each one its
        // own display line, exactly as the P.DATA box draws them
        // ("None" / "AccC Norm60" / "Navichip Attack" for Cannon;
        // navis pack the first line into one string and leave the
        // slot before it null).
        let table = self
            .assets
            .mapper
            .get(self.assets.deref(self.assets.offsets.chip_descriptions_pointer));
        let record = byteorder::LittleEndian::read_u32(table.get(self.id * 4..)?.get(..4)?);
        if record == 0 {
            return None;
        }
        let record = self.assets.mapper.get(record);
        let mut lines = Vec::new();
        for k in 0..3usize {
            let p = byteorder::LittleEndian::read_u32(record.get(k * 4..)?.get(..4)?);
            let Some(s) = (p != 0).then(|| self.assets.lenient_string(p)) else {
                continue;
            };
            // The strings carry right-alignment padding; collapse it.
            let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
            if !s.is_empty() {
                lines.push(s);
            }
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// The chip's HP stat — its contribution to the deck's HP total.
    pub fn hp(&self) -> u16 {
        self.stat_u16(0x00)
    }

    /// The chip's attack power (the card screen's AP).
    pub fn attack_power(&self) -> u16 {
        self.stat_u16(0x04)
    }

    /// The chip's MB cost — and, for a navi chip, the deck's base MB
    /// capacity. A `u16` because the game's is: HubStyl costs 350, past
    /// anything a byte holds.
    pub fn mb(&self) -> u16 {
        self.stat_u16(0x06)
    }

    /// The chip's element: the kind word's top nibble, the same way the
    /// game's own list code takes it to pick the indicator it draws.
    /// 0 none, 1 fire, 2 aqua, 3 wood, 4 elec — cross-checked against
    /// every chip's description line in both games.
    pub fn element(&self) -> usize {
        (self.stat_u16(0x08) >> 12) as usize
    }
}

impl tango_gamesupport_common_dataview::rom::Chip for Chip<'_> {
    fn name(&self) -> Option<String> {
        Chip::name(self)
    }

    fn description(&self) -> Option<String> {
        Chip::description(self)
    }

    fn icon(&self) -> image::RgbaImage {
        self.try_icon()
            .unwrap_or_else(|| image::RgbaImage::new(ICON_SIZE, ICON_SIZE))
    }

    fn image(&self) -> image::RgbaImage {
        self.try_image().unwrap_or_else(|| {
            image::RgbaImage::new(
                (ART_COLS * tango_gamesupport_common_dataview::rom::TILE_WIDTH) as u32,
                (ART_TILES / ART_COLS * tango_gamesupport_common_dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }

    fn codes(&self) -> Vec<char> {
        // BCC chips carry no code letter; one variant each.
        vec!['*']
    }

    fn element(&self) -> usize {
        Chip::element(self)
    }

    fn class(&self) -> tango_gamesupport_common_dataview::rom::ChipClass {
        tango_gamesupport_common_dataview::rom::ChipClass::Standard
    }

    fn dark(&self) -> bool {
        false
    }

    fn mb(&self) -> u8 {
        // The shared chip model tops out at a byte; BCC's own UI reads
        // the real width through the concrete record.
        Chip::mb(self).min(u8::MAX as u16) as u8
    }

    fn attack_power(&self) -> u32 {
        Chip::attack_power(self) as u32
    }

    fn library_sort_order(&self) -> Option<usize> {
        Some(self.stats()[0x0e] as usize)
    }
}
