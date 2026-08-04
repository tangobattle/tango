//! The DS cart's assets: the chips' stats, names, descriptions, icons
//! and element icons, and the NaviCust programs' table, names and
//! descriptions — what the folder and NaviCust views draw from.
//!
//! The port keeps the GBA game's data shapes almost wholesale. The chip
//! table's first 0x20 bytes are byte-identical to BN5's (found by
//! searching the cart for the GBA entries' leading bytes); the entry
//! shrinks from 0x2c to 0x28 by replacing the GBA's three art pointers
//! with one RAM icon pointer plus a pair of indexes into the artwork
//! banks — two cart files holding every chip's tiles and palettes
//! back-to-back, in 0x10-byte units. Names and descriptions are GBA
//! text archives, decoded by BN5's own charsets. The NaviCust program
//! table is the GBA game's outright, all 192 entries of it, with
//! thirteen more appended for the port's own programs (the Navi Change
//! pair, Spport, RUN!).
//!
//! Addresses come in three kinds. `0x02xxxxxx` is main-RAM, mapped
//! through the cart header's load parameters into the ARM9 static
//! image — or, for the party program tables, through the overlay table
//! into the overlay named alongside them. Everything else is a cart
//! *file*, named by the path the file name table spells and read
//! through the allocation table — the element art lives in data files,
//! found by searching for the GBA sheets' bytes, and nothing that
//! useful points at them from the static binary. One of those files,
//! the NaviCust descriptions, is LZ77-compressed; everything else is
//! read where it lies.
//!
//! Nothing is addressed by its position in the image. The data files
//! used to be — they were found there, after all — until the undub
//! patch showed why that can't hold: it rebuilds the whole image to
//! swap the sound archive, and every other file comes out
//! byte-identical but somewhere else. A rebuild rewrites the tables
//! the game itself reads through, so resolving the way the game does
//! (see [`nds`]) survives any repack; a raw offset reads whatever
//! happens to live there now.

mod msg;
pub mod navicust;

use tango_gamesupport_common::dataview::nds;

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
    /// The element icon sheet and its palette (cart files): 4bpp
    /// tiles, one 16x16 icon per element, same bytes as the GBA sheet.
    element_icons: &'static str,
    element_icon_palette: &'static str,
    /// The chip artwork banks (cart files): every chip's 56x48 art
    /// tiles in one cart file and their palettes in another, each
    /// indexed by [`RawChip`]'s pair in 0x10-byte units. The bytes are
    /// the GBA game's own art — which is how the banks were found.
    chip_art: &'static str,
    chip_art_palettes: &'static str,
    /// The NaviCust program table (RAM):
    /// [`NUM_NAVICUST_PARTS`](super::NUM_NAVICUST_PARTS) entries of
    /// 0x10 bytes, the GBA game's own — every entry's first eight bytes
    /// are byte-identical to BN5's, which is how it was found, and the
    /// two bitmap pointers behind them are the same 5x5 masks at DS
    /// addresses.
    ncp_data: u32,
    /// The NaviCust program names, as a text archive (RAM). Not reached
    /// through a pointer — nothing in the static image names it — so it
    /// is addressed as data, the way the element art is. Found by
    /// matching the GBA archive's own encoded entries, which the cart
    /// carries unchanged.
    ncp_names: u32,
    /// The NaviCust program descriptions — what the customizer's
    /// INFORMATION panel reads — as a *cart file*. Unlike every other
    /// archive here this one is LZ77-compressed (its own name on the
    /// cart says so), and it sits four bytes into what it decompresses
    /// to (see [`TEXT_ARCHIVE_OFFSET`]). Found, back when these were
    /// raw offsets, by the names archive being the plain file right
    /// behind it in the image.
    ncp_descriptions: &'static str,
    /// The navi emblem sheet (cart file): thirteen 16x16 icons of
    /// 2x2 tiles, one per navi in id order — the GBA pair's two sheets
    /// merged, MegaMan and Team ProtoMan's six from the ProtoMan cart
    /// followed by Team Colonel's six from the Colonel cart, byte for
    /// byte (which is how it was found). The palettes are the eight
    /// both GBA carts share; which one a navi takes is
    /// [`Offsets::navi_emblem_palette_ids`].
    navi_emblems: &'static str,
    navi_emblem_palettes: &'static str,
    /// Which of those palettes each navi's emblem takes (RAM): one byte
    /// per navi in id order, the GBA pair's own table carried over
    /// whole — which is how it was found, by searching the cart for its
    /// thirteen bytes. Addressed as data; nothing in the static image
    /// points at it.
    navi_emblem_palette_ids: u32,
    /// The overlay the PARTY CUSTOMIZER runs out of, which hosts the
    /// two tables below at the RAM addresses they give.
    partycust_overlay: u16,
    /// The party program tables (RAM, inside that overlay) — see
    /// [`RawPartyPrograms`]. Found by searching the cart for a table
    /// whose entries matched the costs read off the gauge in a driven
    /// customizer (`P.HP+50` one block, `P.HP+300` five, `P.Atk+1`
    /// one).
    party_programs: u32,
    /// Every navi's gauge, in blocks: twelve bytes, navi 1's first —
    /// MegaMan is not a party member and has no card. Confirmed
    /// against the gauge two navis draw (ShadowMan six, Colonel eight).
    partycust_capacities: u32,
    /// The item name archive (RAM), indexed by item id: the subchips
    /// from 0xa0, the party programs from 0xb0. Reached as data rather
    /// than through a pointer, the way [`Offsets::ncp_names`] is.
    item_names: u32,
    /// What the PET and the PARTY CUSTOMIZER's INFORMATION panel say an
    /// item does, as a *cart file* — indexed by item id like the
    /// names, and LZ77-compressed four bytes into what it decompresses
    /// to, like [`Offsets::ncp_descriptions`]. Found by decompressing
    /// every file and looking for one carrying all of the party
    /// programs' own numbers (`+50` through `+300`, `+30`, `+40`),
    /// which reads the same either side of the localization.
    item_descriptions: &'static str,
    /// Every team navi's chip attack before the customizer adds to it
    /// (RAM): ten bytes per navi, navi 1's first, indexed by how far
    /// the file is through the story
    /// ([`Save::story_rank`](crate::save::Save::story_rank)). Read out
    /// of the getter at US `0x02004e28`, which is `table[(navi - 1) *
    /// 10 + rank]`, and confirmed against the three navis a driven
    /// PARTY STATUS card was made to name.
    navi_chip_attack: u32,
}

/// The party program tables as the cart lays them out: four rows of
/// [`NUM_PARTY_PROGRAMS`](super::NUM_PARTY_PROGRAMS) bytes, each padded
/// out to a multiple of four. A row per field rather than an entry per
/// program, so the block is read once and indexed by program.
#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
#[allow(dead_code)]
struct RawPartyPrograms {
    /// The item id each program is bought and stocked as.
    item_ids: [u8; super::NUM_PARTY_PROGRAMS],
    _item_ids_pad: [u8; PARTY_PROGRAM_ROW_PAD],
    /// What one costs the member's gauge, in blocks.
    costs: [u8; super::NUM_PARTY_PROGRAMS],
    _costs_pad: [u8; PARTY_PROGRAM_ROW_PAD],
    /// Which family it is in — see [`PartyProgramKind`].
    kinds: [u8; super::NUM_PARTY_PROGRAMS],
    _kinds_pad: [u8; PARTY_PROGRAM_ROW_PAD],
    /// One the panel's own artwork indexes by, unread here.
    _art: [u8; super::NUM_PARTY_PROGRAMS],
    _art_pad: [u8; PARTY_PROGRAM_ROW_PAD],
}
const PARTY_PROGRAM_ROW_PAD: usize = 3;
const _: () = assert!(std::mem::size_of::<RawPartyPrograms>() == 0x40);

/// What sort of program the cart files one as — its kind row's byte,
/// which is what the customizer's gauge colours a block by. The four
/// families the cart has are one code each; anything else is a cart
/// this build does not know.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartyProgramKind {
    /// `P.Chp+N`, raising the member's chip attack.
    ChipAttack,
    /// `P.HP+N`, raising its max HP.
    MaxHp,
    /// `P.Atk+N`, raising the ATTACK its card shows.
    Attack,
    /// The battle packs and `P.Spport` — the ones that are not a single
    /// stat.
    Special,
}

#[rustfmt::skip]
pub static A5TE_00: Offsets = Offsets {
    chip_data:                  0x0203e9e8,
    chip_names_pointers:        0x020cecf8,
    chip_descriptions_pointers: 0x020ced00,
    navi_names_pointer:         0x020057b4,
    enemy_names_pointer:        0x020057b0,
    chip_icon_palette:          0x020fbf88,
    element_icons:              "/data/rom/a/sub_cdi_pix.bin",
    element_icon_palette:       "/data/rom/a/sub_kind_icon.clt",
    chip_art:                   "/data/rom/c/card_pix.bin",
    chip_art_palettes:          "/data/rom/c/card_clt.bin",
    ncp_data:                   0x020e_b3d0,
    ncp_names:                  0x020d_8a50,
    ncp_descriptions:           "/data/rom_usa/a/prgminf_LZ.bin",
    navi_emblems:               "/data/rom/a/navi_mark.bin",
    navi_emblem_palettes:       "/data/rom/a/custom_cur.clt",
    navi_emblem_palette_ids:    0x020c_ed64,
    partycust_overlay:          413,
    party_programs:             0x021e_c97c,
    partycust_capacities:       0x021e_c958,
    item_names:                 0x020d_8f2c,
    item_descriptions:          "/data/rom_usa/a/iteminf_LZ.bin",
    navi_chip_attack:           0x0203_e0d7,
};

#[rustfmt::skip]
pub static A5TJ_00: Offsets = Offsets {
    chip_data:                  0x0203e7c0,
    chip_names_pointers:        0x020cd734,
    chip_descriptions_pointers: 0x020cd73c,
    navi_names_pointer:         0x02005764,
    enemy_names_pointer:        0x02005760,
    chip_icon_palette:          0x020fa8ac,
    element_icons:              "/data/rom/a/sub_cdi_pix.bin",
    element_icon_palette:       "/data/rom/a/sub_kind_icon.clt",
    chip_art:                   "/data/rom/c/card_pix.bin",
    chip_art_palettes:          "/data/rom/c/card_clt.bin",
    ncp_data:                   0x020e_9cf4,
    ncp_names:                  0x020d_741c,
    ncp_descriptions:           "/data/rom/a/prgminf_LZ.bin",
    navi_emblems:               "/data/rom/a/navi_mark.bin",
    navi_emblem_palettes:       "/data/rom/a/custom_cur.clt",
    navi_emblem_palette_ids:    0x020c_d7a0,
    partycust_overlay:          413,
    party_programs:             0x021e_56a4,
    partycust_capacities:       0x021e_5680,
    item_names:                 0x020d_78d0,
    item_descriptions:          "/data/rom/a/iteminf_LZ.bin",
    navi_chip_attack:           0x0203_deaf,
};

/// Resolves the address kinds against the cart: `0x02xxxxxx` through
/// the ARM9 static image the header describes, cart files by the name
/// the filesystem tables spell — where a repack put the file is the
/// tables' business, not ours. Anything unmappable (a pointer into
/// heap, a truncated image, a file the cart doesn't carry) yields an
/// empty slice, and the readers treat short data as missing rather
/// than panicking.
struct Mapper {
    cart: nds::Cart,
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
            cart: nds::Cart::new(rom),
            arm9_rom_offset,
            arm9_ram_addr,
            arm9_len,
        }
    }

    /// Main RAM `start` through the ARM9 static image — where every
    /// runtime pointer the readers chase lands. What it doesn't cover
    /// is heap (or an overlay, which has its own lookup), and yields
    /// an empty slice.
    fn get(&self, start: u32) -> &[u8] {
        if start >= self.arm9_ram_addr && start < self.arm9_ram_addr + self.arm9_len as u32 {
            self.cart
                .rom()
                .get(self.arm9_rom_offset + (start - self.arm9_ram_addr) as usize..self.arm9_rom_offset + self.arm9_len)
                .unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// The named cart file's bytes, through the filesystem tables.
    fn file(&self, path: &str) -> &[u8] {
        self.cart.file(path)
    }

    fn read_u32(&self, addr: u32) -> Option<u32> {
        Some(u32::from_le_bytes(self.get(addr).get(..4)?.try_into().unwrap()))
    }
}

/// Where a decompressed text file's archive starts: the game keeps four
/// bytes of its own ahead of the offset table, exactly as the GBA
/// game's compressed archives do.
const TEXT_ARCHIVE_OFFSET: usize = 4;

fn read_palette(data: &[u8]) -> tango_gamesupport_common::dataview::rom::Palette {
    data.get(..std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>())
        .map(bytemuck::pod_read_unaligned)
        .unwrap_or([Default::default(); 16])
}

pub struct Assets {
    offsets: &'static Offsets,
    msg_parser: msg::Parser,
    mapper: Mapper,
    /// The overlay hosting the party program tables, decoded once at
    /// load. `None` for a cart whose overlay table doesn't reach it —
    /// the party readers answer zeroes and `None` off that, the way
    /// every reader treats missing data.
    partycust: Option<nds::Overlay>,
    chip_icon_palette: tango_gamesupport_common::dataview::rom::Palette,
    element_icon_palette: tango_gamesupport_common::dataview::rom::Palette,
    /// The one compressed archive the cart makes us decode: the
    /// NaviCust program descriptions, unpacked once at load rather than
    /// per lookup. Empty for a cart whose file isn't one.
    ncp_descriptions: Vec<u8>,
    item_descriptions: Vec<u8>,
}

impl Assets {
    pub fn new(offsets: &'static Offsets, charset: &[&str], rom: Vec<u8>) -> Self {
        let mapper = Mapper::new(rom);
        let partycust = mapper.cart.overlay(offsets.partycust_overlay);
        let chip_icon_palette = read_palette(mapper.get(offsets.chip_icon_palette));
        let element_icon_palette = read_palette(mapper.file(offsets.element_icon_palette));
        // The DS's LZ77 is the GBA's, so the shared decoder reads it.
        let ncp_descriptions =
            tango_gamesupport_common::dataview::rom::unlz77(&mut mapper.file(offsets.ncp_descriptions))
                .unwrap_or_default();
        let item_descriptions =
            tango_gamesupport_common::dataview::rom::unlz77(&mut mapper.file(offsets.item_descriptions))
                .unwrap_or_default();
        Self {
            offsets,
            msg_parser: msg::parser(charset),
            mapper,
            partycust,
            chip_icon_palette,
            element_icon_palette,
            ncp_descriptions,
            item_descriptions,
        }
    }

    /// Decode entry `index` of the text archive `pointer` points at,
    /// keeping only its text. `None` when either address runs out of
    /// mappable range, or the entry does not decode.
    fn text(&self, pointer: u32, index: usize) -> Option<String> {
        self.archive_text(self.mapper.read_u32(pointer)?, index)
    }

    /// The same, for an archive addressed outright rather than through
    /// a pointer — the cart names some of its tables from nowhere.
    fn archive_text(&self, archive: u32, index: usize) -> Option<String> {
        self.blob_text(self.mapper.get(archive), index)
    }

    /// And the same again, for one already decompressed into memory.
    fn blob_text(&self, archive: &[u8], index: usize) -> Option<String> {
        let entry = tango_gamesupport_common::dataview::msg::get_entry(archive, index)?;

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
        self.navi_name(if team == 0 { 1 } else { 7 })
    }

    /// What the cart calls navi `id` — the id the save's team slots and
    /// navi records use, 0 MegaMan and 1.. the two teams' six apiece.
    ///
    /// The roster archive gives every navi a block of six entries (the
    /// navi, its α, β and Y versions, its DS one, then a blank), so a
    /// navi's own name is the first of its block; MegaMan sits ahead of
    /// the blocks. That is how ProtoMan lands on entry 1 and Colonel on
    /// 37, which is where the file select's own two names come from.
    pub fn navi_name(&self, id: usize) -> Option<String> {
        let index = match id {
            0 => MEGAMAN_NAVI_INDEX,
            id => (id - 1) * NAVI_NAME_BLOCK + PROTOMAN_NAVI_INDEX,
        };
        self.text(self.offsets.navi_names_pointer, index)
            .filter(|name| !name.is_empty())
    }

    /// One of the PARTY CUSTOMIZER's programs, in the cart's own table
    /// order — `P.HP+50` first, `P.Spport` last.
    pub fn party_program(&self, index: usize) -> Option<PartyProgram<'_>> {
        (index < super::NUM_PARTY_PROGRAMS).then_some(PartyProgram { index, assets: self })
    }

    /// Navi `id`'s chip attack before the customizer adds to it, for a
    /// file `rank` far through the story — the figure the game's own
    /// PARTY STATUS card adds its `P.Chp` programs to. Zero for
    /// MegaMan, who has no party card.
    pub fn navi_chip_attack(&self, id: usize, rank: u8) -> u16 {
        const RANKS: usize = 10;
        let Some(index) = id.checked_sub(1).filter(|&index| index < crate::save::NUM_NAVIS - 1) else {
            return 0;
        };
        self.mapper
            .get(self.offsets.navi_chip_attack)
            .get(index * RANKS + (rank as usize).min(RANKS - 1))
            .copied()
            .unwrap_or(0) as u16
    }

    /// How many blocks navi `id`'s customizer gauge holds. Zero for
    /// MegaMan, who has no party card, and for anything off the roster.
    pub fn partycust_capacity(&self, id: usize) -> u8 {
        let Some(index) = id.checked_sub(1).filter(|&index| index < crate::save::NUM_NAVIS - 1) else {
            return 0;
        };
        self.partycust
            .as_ref()
            .and_then(|overlay| overlay.get(self.offsets.partycust_capacities).get(index))
            .copied()
            .unwrap_or(0)
    }
}

/// Where [`Assets::cross_name`] and [`Assets::leader_name`] read each
/// name. The two crosses are the last two entries of the enemy list,
/// after the viruses and the game's own modified MegaMan; both BassCross
/// values share one name, as the pair is a difference of team rather
/// than of who shows up.
const MEGAMAN_NAVI_INDEX: usize = 0;
const PROTOMAN_NAVI_INDEX: usize = 1;

/// How many entries the roster archive gives each navi: itself, its
/// three version-up names, its DS one, and a blank.
const NAVI_NAME_BLOCK: usize = 6;
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
        let tiles = self
            .assets
            .mapper
            .file(self.assets.offsets.chip_art)
            .get(raw.art_tiles as usize * 0x10..)?;
        let paletted = tango_gamesupport_common::dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common::dataview::rom::TILE_BYTES * 7 * 6)?,
            7,
        )
        .ok()?;
        let palette = self
            .assets
            .mapper
            .file(self.assets.offsets.chip_art_palettes)
            .get(raw.art_palette as usize * 0x10..)?
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

struct NavicustPart<'a> {
    id: usize,
    assets: &'a Assets,
}

/// A NaviCust program's table entry — the GBA game's own, with its two
/// bitmap pointers pointing at DS addresses.
#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
#[allow(dead_code)]
struct RawNavicustPart {
    _unk_00: u8,
    is_solid: u8,
    _unk_02: u8,
    color: u8,
    _effect_group: u8,
    _unk_05: [u8; 3],
    uncompressed_bitmap_ptr: u32,
    compressed_bitmap_ptr: u32,
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x10);

/// Decode a raw NaviCust colour byte — the GBA game's encoding, which
/// the save's colour bar uses too.
pub fn navicust_part_color(raw: u8) -> Option<tango_gamesupport_common::dataview::rom::NavicustPartColor> {
    use tango_gamesupport_common::dataview::rom::NavicustPartColor as C;
    Some(match raw {
        1 => C::White,
        2 => C::Yellow,
        3 => C::Pink,
        4 => C::Red,
        5 => C::Blue,
        6 => C::Green,
        _ => return None,
    })
}

/// How wide and tall a program's placement mask is.
const NAVICUST_BITMAP_SIZE: usize = 5;

impl NavicustPart<'_> {
    fn raw(&self) -> Option<RawNavicustPart> {
        Some(bytemuck::pod_read_unaligned(
            self.assets
                .mapper
                .get(self.assets.offsets.ncp_data)
                .get(self.id * std::mem::size_of::<RawNavicustPart>()..)?
                .get(..std::mem::size_of::<RawNavicustPart>())?,
        ))
    }

    /// The 5x5 mask at `ptr`, or an empty one for a pointer that runs
    /// out of mappable range — a patched cart renders blank rather than
    /// panicking.
    fn bitmap(&self, ptr: u32) -> tango_gamesupport_common::dataview::rom::NavicustBitmap {
        let cells = self
            .assets
            .mapper
            .get(ptr)
            .get(..NAVICUST_BITMAP_SIZE * NAVICUST_BITMAP_SIZE)
            .map(|raw| raw.iter().map(|&v| v != 0).collect::<Vec<_>>())
            .unwrap_or_else(|| vec![false; NAVICUST_BITMAP_SIZE * NAVICUST_BITMAP_SIZE]);
        ndarray::Array2::from_shape_vec((NAVICUST_BITMAP_SIZE, NAVICUST_BITMAP_SIZE), cells).unwrap()
    }
}

impl tango_gamesupport_common::dataview::rom::NavicustPart for NavicustPart<'_> {
    /// The program's name. The archive names each of the 48 program
    /// templates once — the four colour variants share an entry, as on
    /// GBA.
    fn name(&self) -> Option<String> {
        let entry = tango_gamesupport_common::dataview::msg::get_entry(
            self.assets.mapper.get(self.assets.offsets.ncp_names),
            self.id / 4,
        )?;
        Some(
            self.assets
                .msg_parser
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

    /// What the customizer's INFORMATION panel says about the program —
    /// `Custom Screen +1 chip`, `Max HP +500!`. Same archive shape as
    /// the names: one entry per program template, the four colour
    /// variants sharing it.
    fn description(&self) -> Option<String> {
        self.assets
            .blob_text(self.assets.ncp_descriptions.get(TEXT_ARCHIVE_OFFSET..)?, self.id / 4)
    }

    fn color(&self) -> Option<tango_gamesupport_common::dataview::rom::NavicustPartColor> {
        navicust_part_color(self.raw()?.color)
    }

    fn is_solid(&self) -> bool {
        // The GBA game's sense: the flag is set for the programs that
        // may be placed off the command line.
        self.raw().map(|raw| raw.is_solid == 0).unwrap_or(false)
    }

    fn uncompressed_bitmap(&self) -> tango_gamesupport_common::dataview::rom::NavicustBitmap {
        let ptr = self.raw().map(|raw| raw.uncompressed_bitmap_ptr).unwrap_or(0);
        self.bitmap(ptr)
    }

    fn compressed_bitmap(&self) -> Option<tango_gamesupport_common::dataview::rom::NavicustBitmap> {
        Some(self.bitmap(self.raw()?.compressed_bitmap_ptr))
    }
}

struct Navi<'a> {
    id: usize,
    assets: &'a Assets,
}

impl Navi<'_> {
    /// The 16x16 emblem off the sheet, or `None` when the sheet runs
    /// out of mappable range — a patched cart renders blank.
    fn try_emblem(&self) -> Option<image::RgbaImage> {
        const EMBLEM_TILES: usize = 2 * 2;
        let tiles = self
            .assets
            .mapper
            .file(self.assets.offsets.navi_emblems)
            .get(self.id * EMBLEM_TILES * tango_gamesupport_common::dataview::rom::TILE_BYTES..)?;
        let paletted = tango_gamesupport_common::dataview::rom::read_merged_tiles(
            tiles.get(..tango_gamesupport_common::dataview::rom::TILE_BYTES * EMBLEM_TILES)?,
            2,
        )
        .ok()?;
        let which = *self
            .assets
            .mapper
            .get(self.assets.offsets.navi_emblem_palette_ids)
            .get(self.id)? as usize;
        let palette = bytemuck::pod_read_unaligned::<tango_gamesupport_common::dataview::rom::Palette>(
            self.assets
                .mapper
                .file(self.assets.offsets.navi_emblem_palettes)
                .get(which * std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>()..)?
                .get(..std::mem::size_of::<tango_gamesupport_common::dataview::rom::Palette>())?,
        );
        Some(tango_gamesupport_common::dataview::rom::apply_palette(paletted, &palette))
    }
}

impl tango_gamesupport_common::dataview::rom::Navi for Navi<'_> {
    fn name(&self) -> Option<String> {
        self.assets.navi_name(self.id)
    }

    fn emblem(&self) -> image::RgbaImage {
        self.try_emblem().unwrap_or_else(|| {
            image::RgbaImage::new(
                (2 * tango_gamesupport_common::dataview::rom::TILE_WIDTH) as u32,
                (2 * tango_gamesupport_common::dataview::rom::TILE_HEIGHT) as u32,
            )
        })
    }
}

/// One of the PARTY CUSTOMIZER's programs: what the cart calls it,
/// what it costs the member's gauge, and what it gives them.
pub struct PartyProgram<'a> {
    index: usize,
    assets: &'a Assets,
}

impl PartyProgram<'_> {
    fn raw(&self) -> Option<RawPartyPrograms> {
        Some(bytemuck::pod_read_unaligned(
            self.assets
                .partycust
                .as_ref()?
                .get(self.assets.offsets.party_programs)
                .get(..std::mem::size_of::<RawPartyPrograms>())?,
        ))
    }

    /// The item id the file stocks this program as — what indexes the
    /// save's own item counts and the cart's name archive.
    pub fn item_id(&self) -> usize {
        self.raw().map(|raw| raw.item_ids[self.index]).unwrap_or(0) as usize
    }

    /// How many blocks of the member's gauge it fills.
    pub fn cost(&self) -> u8 {
        self.raw().map(|raw| raw.costs[self.index]).unwrap_or(0)
    }

    /// Which family the cart files it under — what the gauge colours
    /// its blocks by. `None` for a code this build has no family for.
    pub fn kind(&self) -> Option<PartyProgramKind> {
        Some(match self.raw()?.kinds[self.index] {
            2 => PartyProgramKind::ChipAttack,
            3 => PartyProgramKind::MaxHp,
            4 => PartyProgramKind::Attack,
            5 => PartyProgramKind::Special,
            _ => return None,
        })
    }

    /// What the cart calls it — `P.HP+50`, `P.サポート`.
    pub fn name(&self) -> Option<String> {
        self.assets
            .archive_text(self.assets.offsets.item_names, self.item_id())
            .filter(|name| !name.is_empty())
    }

    /// What the customizer's INFORMATION panel says it does.
    pub fn description(&self) -> Option<String> {
        self.assets
            .blob_text(
                self.assets.item_descriptions.get(TEXT_ARCHIVE_OFFSET..)?,
                self.item_id(),
            )
            .filter(|description| !description.is_empty())
    }

    /// What equipping it gives the member.
    pub fn bonus(&self) -> crate::save::PartycustBonus {
        PARTY_PROGRAM_BONUSES[self.index]
    }
}

/// What each party program gives the member who wears it.
///
/// This is the one thing about a program the cart keeps only as code:
/// the customizer applies one through a jump table (US `0x021e0560`)
/// whose cases add immediates to the navi record's four fields, and
/// takes it back off through the mirror-image table at `0x021e02d0`.
/// There is no data table behind those immediates to read — the cart's
/// own tables carry every program's item id, cost, category and panel
/// art, and nothing else. So the thirteen triples are transcribed from
/// those cases, in the table's own order, and everything else about a
/// program is read out of the cart.
///
/// The last three are why this can't be a formula over the categories:
/// the two battle packs bundle all three bonuses at once, and `P.Spport`
/// grants none of them, only the record's support flag.
const PARTY_PROGRAM_BONUSES: [crate::save::PartycustBonus; super::NUM_PARTY_PROGRAMS] = {
    use crate::save::PartycustBonus as B;
    const NONE: B = B {
        max_hp: 0,
        attack: 0,
        chip_attack: 0,
        support: false,
    };
    [
        B { max_hp: 50, ..NONE },
        B { max_hp: 100, ..NONE },
        B { max_hp: 200, ..NONE },
        B { max_hp: 300, ..NONE },
        B { attack: 1, ..NONE },
        B { attack: 2, ..NONE },
        B { attack: 3, ..NONE },
        B { chip_attack: 30, ..NONE },
        B { chip_attack: 40, ..NONE },
        B { chip_attack: 50, ..NONE },
        B {
            max_hp: 50,
            attack: 1,
            chip_attack: 30,
            support: false,
        },
        B {
            max_hp: 200,
            attack: 2,
            chip_attack: 40,
            support: false,
        },
        B { support: true, ..NONE },
    ]
};

/// What the cart draws its NaviCust grid on. The GBA games each carry
/// one colour per version; this cart holds both teams' files, and the
/// backdrop is the one thing here that would have to know which of them
/// is open — the layout is asked of the cart, not of a save — so both
/// get Team ProtoMan's.
const NAVICUST_BG: image::Rgba<u8> = image::Rgba([0x21, 0x8c, 0xa5, 0xff]);

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

    fn navicust_part(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common::dataview::rom::NavicustPart + '_>> {
        if id >= self.num_navicust_parts() {
            return None;
        }
        Some(Box::new(NavicustPart { id, assets: self }))
    }

    fn num_navicust_parts(&self) -> usize {
        super::NUM_NAVICUST_PARTS
    }

    fn navi(&self, id: usize) -> Option<Box<dyn tango_gamesupport_common::dataview::rom::Navi + '_>> {
        if id >= self.num_navis() {
            return None;
        }
        Some(Box::new(Navi { id, assets: self }))
    }

    fn num_navis(&self) -> usize {
        crate::save::NUM_NAVIS
    }

    /// The GBA game's grid, which the cart keeps: five by five, the
    /// command line third from the top, nothing placeable outside it.
    fn navicust_layout(&self) -> Option<tango_gamesupport_common::dataview::rom::NavicustLayout> {
        Some(tango_gamesupport_common::dataview::rom::NavicustLayout {
            command_line: 2,
            has_out_of_bounds: false,
            background: NAVICUST_BG,
        })
    }

    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        if id >= 13 {
            return None;
        }

        let buf = self.mapper.file(self.offsets.element_icons);
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
    fn maps_nothing_below_the_arm9_image() {
        // A low value used to read the image where it lay — until the
        // undub repack moved every file out from under those reads.
        // Files go through [`Mapper::file`] by name now, and a raw
        // offset maps to nothing at all.
        let mapper = Mapper::new(cart(&[9, 8, 7]));
        assert_eq!(mapper.get(0x4001), &[] as &[u8]);
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
