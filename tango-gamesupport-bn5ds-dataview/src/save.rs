//! The BN5 Double Team DS save.
//!
//! A cartridge dump is not one save: the game's file select offers two
//! in-game files, and the flash carries each as its own mirrored pair
//! of blocks. So the dump parses to a [`SaveSet`], which hands out one
//! [`Save`] per file — each a save in its own right, with its own
//! folders to read and edit. A `Save` keeps the whole dump behind it
//! (that is what gets written back to the cartridge) but only ever
//! reads and writes its own file's block.
//!
//! Which file the *cartridge* plays is the one the game itself calls
//! most recently saved — [`SaveSet::current`], off the generation
//! counters in the blocks' footers. Everything above reads it there:
//! the editor opens on it, a session boots it, the priming walk steers
//! the game's own file select to it, and a recording gets it for free
//! by storing the same bytes. Picking the other file is therefore an
//! edit to the cartridge rather than a note beside it — see
//! [`Save::make_current`].
//!
//! Recognition is settled: the game stamps its own format tag into
//! every block it formats, and finding that tag intact is what puts a
//! dump in the save picker. The interior is mapped as far as the GBA
//! game's editor reaches: the chip folders and pack, the
//! equipped-folder cluster, the NaviCust (grid, parts and colour bar),
//! the navi records the HP comes out of, the auto-battle data, the
//! GBA-slot [`Cross`] byte, and — read out of the ARM9's own save code
//! — the flash checksums, so an edited block can be made acceptable to
//! the game again.
//!
//! What the GBA game has and this cart hasn't: patch cards, which
//! Double Team dropped, so there is no view for them. The rest of the
//! layout was found through the ARM9's own section table (the save
//! object at US `0x021701d4`, filled at `0x020214bc`, whose fields are
//! the sections' addresses) plus the GBA game's own shapes.

use tango_gamesupport_common::dataview::save::Error;

/// The flash the cart carries: 256 KiB. That is what the emulator
/// mounts and hands back, so it is the length a dump is canonicalized
/// to — but not the length one has to arrive at, see [`SaveSet::parse`].
pub const SIZE: usize = 0x4_0000;

/// What a byte of flash reads as before anything is written to it, and
/// so what a dump that stops short of [`SIZE`] is padded out with.
const ERASED: u8 = 0xff;

/// The game divides the flash into blocks of this size and writes a
/// save as a mirrored pair of them.
pub const BLOCK_SIZE: usize = 0xa000;

/// Where in each block the game stamps [`MAGIC`].
pub const MAGIC_OFFSET: usize = 0x9f08;

/// The format tag the game writes at the tail of every block it
/// formats. The string is baked into both the US and JP ROMs, so one
/// check covers bn5ds and exe5ds alike.
///
/// The head of a block is live data, not a header — fingerprinting it
/// rejected real saves whose dumper left different bytes there.
pub const MAGIC: &[u8; 16] = b"EXE DS SAVE 0006";

/// Which file-select slot a block's contents belong to (0 or 1), at
/// the tail of the footer line `00 00 <slot> 01` that follows
/// [`MAGIC`]. The mount code in the ARM9 walks every block and keeps,
/// per file, the intact block with the highest generation counter.
pub const FILE_SLOT_OFFSET: usize = 0x9f1a;

/// The block's save-generation counter (u32 LE), bumped on every save.
/// A file alternates between two mirrored pairs of blocks, so the pair
/// with the highest counter holds the file's current contents — and
/// the highest counter overall belongs to whichever file was saved
/// most recently. The first-save format stamps every block with the
/// same baked starting value (0x3b7 observed), and an untouched file
/// keeps it, so only real saves separate the counters.
pub const GENERATION_OFFSET: usize = 0x9f1c;

/// The save image: the first 0x8440 bytes of a block are the data the
/// game loads and checksums; from there to the footer is fill.
pub const SAVE_IMAGE_SIZE: usize = 0x8440;

/// The save image's interior checksum: a u32 byte-sum of the whole
/// image with these four bytes excluded. Verified at load, separately
/// from the footer pairs — a save whose data changed without it is
/// rejected and the file select boots the new-game path (probed live,
/// July 2026). A byte-sum is permutation-invariant, which is how chip
/// *reorders* shipped without rebuilding it: rearranged bytes keep
/// their sum. Sits inside the image, so cs2 covers it and it must be
/// rebuilt first.
pub const INTERIOR_CHECKSUM_OFFSET: usize = 0x9c;

/// The checksum pairs at each block's tail, as four u16 LE:
/// `[cs1, !cs1, cs2, !cs2]`, each value paired with `0x10000 - value`.
/// `cs2` covers the save image; `cs1` covers the footer from +0x9f04
/// through +0x9f20 — the cs2 pair, the format tag, the file-slot line
/// and the generation counter — so cs1 must be recomputed after cs2
/// lands. Read out of the ARM9's own save code (US build: writer at
/// 0x02020b68, verifier at 0x02020bfc, the sum itself at 0x02020c90).
pub const CHECKSUM_OFFSET: usize = 0x9f00;

/// The stretch `cs1` sums: from the cs2 pair through the end of the
/// generation counter.
const FOOTER_SUM_START: usize = 0x9f04;
const FOOTER_SUM_END: usize = 0x9f20;

/// The chip folders: 3 folders of 30 chips each, one u16 LE per chip in
/// the same encoding the GBA game uses (id in bits 0..9, code in bits
/// 9..16). Verified byte-identical across a generation's mirrored pair.
pub const FOLDER_OFFSET: usize = 0x498;

/// Folders per save. The array at [`FOLDER_OFFSET`] holds exactly three
/// before the entries stop decoding, matching the GBA game.
pub const NUM_FOLDERS: usize = 3;

/// The chip pack: one 12-byte entry per chip id — a count byte per
/// code slot (the stat table's `codes[4]` order, which is the pack
/// API's "variant"), then a u16 LE per slot holding that code's
/// acquisition key. The 320 entries fill this section of the ARM9's
/// own section table exactly; ids past the table — Program Advances
/// and the DS navi chips — have no pack slot, as on GBA.
pub const PACK_OFFSET: usize = 0x54c;
pub const NUM_PACK_CHIPS: usize = 320;
const PACK_ENTRY_SIZE: usize = 12;

/// The acquisition keys count *down* from this as codes are obtained —
/// a maxed save observed 648 of them running 0x7fff..0x7d78, handed out
/// in library order rather than by id. 0 means the code was never
/// owned, which is how a code slot the chip doesn't even have reads.
const PACK_KEY_FIRST: u16 = 0x7fff;

/// Which folder is equipped, as an index into [`FOLDER_OFFSET`]'s
/// array. Found by the GBA game's own geometry: there the equipped
/// byte sits 0x11 before the navi-stats array with the per-folder
/// Regular-chip bytes right behind it, and the same cluster sits at
/// the same distance from [`NAVI_STATS_OFFSET`] here — confirmed
/// against a cart known to have its second folder selected.
pub const EQUIPPED_FOLDER_OFFSET: usize = 0x2e99;

/// The per-folder Regular chip: one byte per folder, the chip's index
/// into that folder, with 0xff (anything past 29) meaning none — the
/// GBA game's own encoding.
pub const REGULAR_CHIP_OFFSET: usize = 0x2e9a;

/// The navi stats array: 13 slots of 0x60 bytes (slot 0 the player,
/// 1.. the team link navis, as on GBA), each starting u16 LE base max
/// HP, current HP, effective max HP. It is the anchor
/// [`EQUIPPED_FOLDER_OFFSET`] was found against, and what
/// [`NaviView`](tango_gamesupport_common::dataview::save::NaviView)
/// reports HP out of.
pub const NAVI_STATS_OFFSET: usize = 0x2eaa;

/// The navi record array proper, as the game's own accessor walks it:
/// record `i` at `NAVI_RECORD_OFFSET + i * 0x60`, record 0 being the
/// player's. [`NAVI_STATS_OFFSET`] is the HP triple 0x3e into record 0
/// — the same array reached from the other end.
///
/// Read out of the ARM9: the save object at US `0x021701d4` holds this
/// block's address at `+0x74`, and the accessor at `0x02007784` indexes
/// it (through an identity remap table) by `0x60`.
const NAVI_RECORD_OFFSET: usize = 0x2e6c;

/// How long a navi record is, and how far into one its HP triple sits —
/// [`NAVI_STATS_OFFSET`] is record 0's.
const NAVI_RECORD_SIZE: usize = 0x60;
const NAVI_STATS_INTO_RECORD: usize = NAVI_STATS_OFFSET - NAVI_RECORD_OFFSET;

/// How many navis the record array holds: MegaMan and both teams' six.
pub const NUM_NAVIS: usize = 13;

/// Which GBA-slot cross the player brings, at record 0 `+0x4c` — the
/// byte the game's own file select writes when it finds a cartridge in
/// the DS's GBA slot and the player accepts it. See [`Cross`] for the
/// values.
pub const CROSS_OFFSET: usize = NAVI_RECORD_OFFSET + 0x4c;

/// The folder's Regular memory, in MB, at record 0 `+9`. The GBA game
/// keeps this byte 0x24 before its equipped-folder byte and so does the
/// cart: the whole cluster the GBA game reads from `0x52a8` is record
/// 0 here, at the same distances into it.
pub const REGULAR_MEMORY_OFFSET: usize = NAVI_RECORD_OFFSET + 0x09;

/// Which navi the player is: 0 MegaMan, 1.. a team navi, in the roster
/// order the navi records use. Read out of the ARM9 — the getter at US
/// `0x0208d7a4` reads byte 1 of the save image's first section, and the
/// setter at `+0x14` writes it — and it is the byte the GBA game keeps
/// at its own `0x2941`.
///
/// It is not a battle loadout, though the GBA game's is: this cart
/// hands you your team **in** a battle (the touch screen's NAVI CHANGE
/// panel), and the byte is who the field is currently being played as,
/// which the Liberation missions move. A save carrying a nonzero one
/// boots into that navi's world, not into a net battle — verified by
/// poking it and watching the priming walk land on the PET screen — so
/// nothing here writes it, and a save that has one is treated the way
/// BN5 treats a link navi: no NaviCust of MegaMan's to edit.
pub const NAVI_OFFSET: usize = 0x0001;

/// The materialized NaviCust grid: 5x5 cells, each holding a part
/// slot + 1 (0 for an empty cell), then seven bytes the section pads
/// with. The game's own section table gives the section as 0x20 bytes
/// at this offset, immediately before the parts themselves.
pub const NAVICUST_GRID_OFFSET: usize = 0x24dc;
const NAVICUST_GRID_SECTION_SIZE: usize = 0x20;

/// The NaviCust parts: 25 slots of 8 bytes — id, a byte the game keeps
/// zero, column, row, rotation, compressed flag, and two more zeroes —
/// the GBA game's own entry. The section table gives it as 0xc8 bytes,
/// which is exactly the 25.
pub const NAVICUST_PARTS_OFFSET: usize = 0x24fc;
const NUM_NAVICUST_SLOTS: usize = 25;
const NAVICUST_PART_SIZE: usize = 8;

/// The NaviCust colour bar: six bytes holding the distinct part colours
/// in placement order, zero-padded — the shape and encoding the GBA
/// game uses. Found by computing what the bar for a played cart's grid
/// must read and finding that run in the save image.
pub const NAVICUST_COLOR_BAR_OFFSET: usize = 0x6910;
const NAVICUST_COLOR_BAR_LEN: usize = 6;

/// The auto-battle data: the 42-slot deck the game materializes, then
/// the two use-count arrays it ranks chips by. Both arrays are u16 per
/// chip id and
/// [`NUM_AUTO_BATTLE_DATA_CHIPS`](crate::NUM_AUTO_BATTLE_DATA_CHIPS)
/// long — found through the ARM9's own "count one use" helpers (US
/// `0x0209952c` bumps the first, `0x02099518` the second), which is
/// also what tells the two apart.
pub const AUTO_BATTLE_DATA_OFFSET: usize = 0x334c;
pub const CHIP_USE_COUNT_OFFSET: usize = 0x4944;
pub const SECONDARY_CHIP_USE_COUNT_OFFSET: usize = 0x4c24;

/// How many slots the materialized deck holds.
const NUM_AUTO_BATTLE_DATA_SLOTS: usize = 42;

/// Where the deck's eight combo slots sit in it. The GBA game leaves
/// them empty and the shared materializer does too, but this cart
/// writes real markers there (`0x8000 | n` on a played cart), so a
/// rebuild leaves them alone rather than blanking something the game
/// put there.
const AUTO_BATTLE_DATA_COMBO_SLOTS: std::ops::Range<usize> = 33..41;

/// The MegaMan a save brings to a battle: plain, or one of the two
/// crosses the game unlocks from a cartridge in the GBA slot.
///
/// This is the game's own byte, in the game's own encoding — writing it
/// is what its file select does after asking. Slot 2 is never emulated
/// here: what the cartridge would have bought is what the player picks,
/// and PvP re-asserts it (see the crate's `pvp` module) because the
/// file select clears the byte on every boot, cartridge or not.
///
/// BassCross is two values because the game keeps two, chosen by the
/// save's own team ([`TEAM_OFFSET`]) rather than by the player: a Team
/// ProtoMan save's is [`Cross::BassProto`] and a Team Colonel save's
/// [`Cross::BassColonel`]. [`Cross::bass_for`] is that rule, so a pick
/// lands the value the game would have written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Cross {
    /// No cross: plain MegaMan, which is every save that has not been
    /// through the prompt.
    None = 0,
    /// BassCross MegaMan on a Team ProtoMan save.
    BassProto = 1,
    /// BassCross MegaMan on a Team Colonel save.
    BassColonel = 2,
    /// SolCross MegaMan, from the Boktai cartridge (Boktai 2 in the US
    /// build, Boktai 3 in the JP one — one value either way).
    Sol = 3,
}

/// Which of the cartridge's two teams a save plays, at this offset in
/// the save image: 0 Team ProtoMan, 1 Team Colonel. The game reads it
/// through the accessor at US `0x02001d74` to pick which BassCross
/// value to write.
pub const TEAM_OFFSET: usize = 0x0b;

impl Cross {
    /// The byte, as the save stores it.
    pub fn raw(self) -> u8 {
        self as u8
    }

    /// A stored byte read back, or [`Cross::None`] for anything the
    /// game does not write.
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Cross::BassProto,
            2 => Cross::BassColonel,
            3 => Cross::Sol,
            _ => Cross::None,
        }
    }

    /// BassCross as it would be written on a save whose [`TEAM_OFFSET`]
    /// byte is `team` — the game's own choice between the two values.
    pub fn bass_for(team: u8) -> Self {
        if team == 0 {
            Cross::BassProto
        } else {
            Cross::BassColonel
        }
    }

    /// Whether this is a BassCross, either team's.
    pub fn is_bass(self) -> bool {
        matches!(self, Cross::BassProto | Cross::BassColonel)
    }
}

/// The game's own checksum (ARM9 0x02020c90): the sum of each u16
/// XORed with the byte count still remaining at that point. The
/// length key is why no plain range sum ever matched the stored
/// values.
fn checksum(buf: &[u8]) -> u16 {
    let mut remaining = buf.len() as u16;
    let mut sum = 0u16;
    for pair in buf.chunks_exact(2) {
        sum = sum.wrapping_add(u16::from_le_bytes([pair[0], pair[1]]) ^ remaining);
        remaining = remaining.wrapping_sub(2);
    }
    sum
}

/// The generation counter stamped in `block`'s footer.
fn generation(data: &[u8], block: usize) -> u32 {
    u32::from_le_bytes(
        data[block * BLOCK_SIZE + GENERATION_OFFSET..][..4]
            .try_into()
            .unwrap(),
    )
}

/// Whether `block` carries the game's format tag — the one thing that
/// tells a written block from erased flash.
fn formatted(data: &[u8], block: usize) -> bool {
    &data[block * BLOCK_SIZE + MAGIC_OFFSET..][..MAGIC.len()] == MAGIC
}

/// Recompute `block`'s checksums the way the game does: the interior
/// byte-sum first (it lives inside the image), then cs2 over the save
/// image that now contains it, then cs1 over the footer that now
/// contains cs2.
fn rebuild_block(data: &mut [u8], block: usize) {
    let base = block * BLOCK_SIZE;

    let image = &data[base..][..SAVE_IMAGE_SIZE];
    let interior = image.iter().map(|&v| v as u32).sum::<u32>().wrapping_sub(
        image[INTERIOR_CHECKSUM_OFFSET..][..4]
            .iter()
            .map(|&v| v as u32)
            .sum::<u32>(),
    );
    data[base + INTERIOR_CHECKSUM_OFFSET..][..4].copy_from_slice(&interior.to_le_bytes());

    let cs2 = checksum(&data[base..][..SAVE_IMAGE_SIZE]);
    data[base + CHECKSUM_OFFSET + 4..][..2].copy_from_slice(&cs2.to_le_bytes());
    data[base + CHECKSUM_OFFSET + 6..][..2].copy_from_slice(&0u16.wrapping_sub(cs2).to_le_bytes());

    let cs1 = checksum(&data[base + FOOTER_SUM_START..base + FOOTER_SUM_END]);
    data[base + CHECKSUM_OFFSET..][..2].copy_from_slice(&cs1.to_le_bytes());
    data[base + CHECKSUM_OFFSET + 2..][..2].copy_from_slice(&0u16.wrapping_sub(cs1).to_le_bytes());
}

/// Every save on one cartridge: the whole flash dump, and which block
/// holds the live copy of each in-game file. Handing out a [`Save`] per
/// file is what makes the two files independently readable and
/// editable — see the module docs.
#[derive(Clone)]
pub struct SaveSet {
    data: Vec<u8>,
    /// `(file-select slot, live block)` per file the cart holds, in
    /// slot order.
    files: Vec<(u8, usize)>,
}

impl SaveSet {
    /// Accept `data` if it is a dump of this game's flash: no longer
    /// than the chip, with the game's own format tag in at least one
    /// block.
    ///
    /// The game falls back to a generation's twin copy when one block
    /// is damaged, so a single intact tag is as much as it requires
    /// too.
    ///
    /// A dump of any length is accepted and canonicalized to [`SIZE`],
    /// which is the length the emulator mounts and hands back.
    ///
    /// Short of the chip it is padded with erased flash. Dumpers trim
    /// the erased tail, or size the chip by what the game touched, and
    /// either way the short file carries every block the game wrote;
    /// the padding lands where flash the cart never wrote would be, and
    /// reads back as unformatted blocks — which is exactly what the
    /// game's own mount makes of them.
    ///
    /// Past the chip it is cut off. A dump longer than 256 KiB is a
    /// container, not a bigger cartridge: DS save managers routinely
    /// write a fixed 512 KiB whatever the cart holds, and every byte of
    /// such a file past the chip is flash that does not exist. Taking
    /// the front of it is not a guess — it is what `CartRetail` does
    /// with the same file, copying `min(file, chip)` into an erased
    /// chip and dropping the rest, and what the GBA saves here have
    /// always done by slicing the range they need out of a dump.
    /// Length was never what identified a save anyway: that is the
    /// format tag below, and a file that does not carry it is refused
    /// at any size.
    ///
    /// So the save Tango reads, the SRAM it hands the console and the
    /// image the replay records are the same bytes whatever length the
    /// file arrived at.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut data = data.to_vec();
        data.resize(SIZE, ERASED);

        // A file's live block is its intact block with the highest
        // generation counter; the game default-formats every block on
        // first save (an unused file carries baked defaults, tag and
        // all), so tag presence can't tell what's in use — only real
        // saves move the counter. A generation's two mirrored copies
        // are identical on an intact cart, so the tie between them is
        // broken toward the first block just to be deterministic.
        let mut files: Vec<(u8, usize)> = Vec::new();
        for block in (0..SIZE / BLOCK_SIZE).filter(|&block| formatted(&data, block)) {
            let slot = data[block * BLOCK_SIZE + FILE_SLOT_OFFSET];
            match files.iter_mut().find(|(s, _)| *s == slot) {
                Some(file) if generation(&data, block) > generation(&data, file.1) => file.1 = block,
                Some(_) => {}
                None => files.push((slot, block)),
            }
        }
        if files.is_empty() {
            return Err(Error::InvalidGameName(data[MAGIC_OFFSET..][..MAGIC.len()].to_vec()));
        }
        files.sort_unstable();

        Ok(SaveSet { data, files })
    }

    /// The file-select slots this cart holds, in slot order. Both exist
    /// on any cart the game formatted; a damaged dump may carry one.
    pub fn slots(&self) -> Vec<u8> {
        self.files.iter().map(|&(slot, _)| slot).collect()
    }

    /// File `slot`'s save, or `None` if this cart has no such file.
    pub fn save(&self, slot: u8) -> Option<Save> {
        let &(_, block) = self.files.iter().find(|(s, _)| *s == slot)?;
        Some(Save {
            data: self.data.clone(),
            block,
            slot,
            slots: self.slots(),
        })
    }

    /// The file saved most recently — **the** save this cartridge
    /// carries, as far as everything above is concerned: what the
    /// editor opens on, what a session plays, and what the priming walk
    /// steers the game's own file select to. Which file that is lives
    /// in the cartridge's own bytes (the generation counters the game
    /// stamps), so the choice needs nothing riding beside them — see
    /// [`Save::make_current`], which is how the editor changes it.
    pub fn current(&self) -> Save {
        let &(slot, _) = self
            .files
            .iter()
            .max_by_key(|&&(_, block)| generation(&self.data, block))
            .expect("a parsed SaveSet holds at least one file");
        self.save(slot).expect("the slot came from this set")
    }
}

/// One in-game file, handed out by [`SaveSet`]. The whole dump rides
/// along because that is what gets written back to the cartridge, but
/// every read and write goes through this file's own block.
#[derive(Clone)]
pub struct Save {
    data: Vec<u8>,
    block: usize,
    slot: u8,
    slots: Vec<u8>,
}

impl Save {
    /// Which of the game's file-select slots this save is.
    pub fn slot(&self) -> u8 {
        self.slot
    }

    /// Every file-select slot the cart behind this save holds — what a
    /// frontend's file picker offers. Carried along so drawing the
    /// picker doesn't mean re-parsing the dump every frame.
    pub fn slots(&self) -> &[u8] {
        &self.slots
    }

    /// The block this file's views read from.
    fn active(&self) -> &[u8] {
        &self.data[self.block * BLOCK_SIZE..][..BLOCK_SIZE]
    }

    fn active_mut(&mut self) -> &mut [u8] {
        &mut self.data[self.block * BLOCK_SIZE..][..BLOCK_SIZE]
    }

    /// Which team this file plays (see [`TEAM_OFFSET`]).
    pub fn team(&self) -> u8 {
        self.active()[TEAM_OFFSET]
    }

    /// The GBA-slot cross this file brings (see [`Cross`]).
    pub fn cross(&self) -> Cross {
        Cross::from_raw(self.active()[CROSS_OFFSET])
    }

    /// Which navi this file is being played as (see [`NAVI_OFFSET`]).
    pub fn navi(&self) -> usize {
        self.active()[NAVI_OFFSET] as usize
    }

    /// Navi `id`'s stats block: base max HP, current HP, effective max
    /// HP, as three u16 LE. `None` past the roster.
    fn navi_stats(&self, id: usize) -> Option<[u16; 3]> {
        let raw = self
            .active()
            .get(NAVI_RECORD_OFFSET + id * NAVI_RECORD_SIZE + NAVI_STATS_INTO_RECORD..)?
            .get(..3 * std::mem::size_of::<u16>())?;
        Some(std::array::from_fn(|i| {
            u16::from_le_bytes(raw[i * 2..][..2].try_into().unwrap())
        }))
    }

    /// Set the cross this file brings, writing the game's own byte.
    /// Checksums are not rebuilt here — the editor commits through
    /// [`rebuild_checksum`](tango_gamesupport_common::dataview::save::Save::rebuild_checksum)
    /// like every other edit.
    pub fn set_cross(&mut self, cross: Cross) {
        self.active_mut()[CROSS_OFFSET] = cross.raw();
    }

    /// Make this file the cartridge's most recently saved one, so
    /// [`SaveSet::current`] hands it out — which is how the pick
    /// reaches a session, a recording and the game's own file select
    /// without anything riding beside the bytes.
    ///
    /// The game alternates a file between two mirrored pairs of blocks
    /// and calls the highest counter current, so being current is a
    /// matter of degree rather than a flag: this stamps the file's live
    /// pair one past every counter on the cartridge. Both blocks of the
    /// pair, because the game reads whichever of them it reaches first
    /// — the same reason `rebuild_checksum` mirrors.
    ///
    /// A no-op when this file already leads.
    pub fn make_current(&mut self) {
        let blocks = self.data.len() / BLOCK_SIZE;
        let highest = (0..blocks)
            .filter(|&b| formatted(&self.data, b))
            .map(|b| generation(&self.data, b))
            .max()
            .unwrap_or(0);
        if generation(&self.data, self.block) == highest {
            return;
        }
        let next = highest.wrapping_add(1);
        for block in [self.block, self.block ^ 1] {
            let base = block * BLOCK_SIZE + GENERATION_OFFSET;
            self.data[base..][..4].copy_from_slice(&next.to_le_bytes());
        }
    }
}

impl tango_gamesupport_common::dataview::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::ChipsViewMut + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navi(&self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::NaviView + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    /// The NaviCust, unless the file is being played as a team navi —
    /// the customizer is MegaMan's, exactly as it is on GBA (see
    /// [`NAVI_OFFSET`] for what a nonzero navi means on this cart).
    fn view_navicust(&self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::NavicustView + '_>> {
        if self.navi() != 0 {
            return None;
        }
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_navicust_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::NavicustViewMut + '_>> {
        if self.navi() != 0 {
            return None;
        }
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_auto_battle_data(
        &self,
    ) -> Option<Box<dyn tango_gamesupport_common::dataview::save::AutoBattleDataView + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn view_auto_battle_data_mut(
        &mut self,
    ) -> Option<Box<dyn tango_gamesupport_common::dataview::save::AutoBattleDataViewMut + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        self.data.clone()
    }

    /// The whole dump doubles as the "wram": the cart's ROM assets are
    /// read from the cart image, not derived from a save — the trait
    /// just wants the save's raw bytes.
    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.data)
    }

    /// Recompute this file's block checksum pairs, and bring the
    /// generation's twin copy along so whichever of the mirrored pair
    /// the game reads first loads.
    fn rebuild_checksum(&mut self) {
        rebuild_block(&mut self.data, self.block);
        // Blocks pair up as (0,1), (2,3), (4,5); the twin is the other
        // half of this block's pair.
        let base = self.block * BLOCK_SIZE;
        let twin = (self.block ^ 1) * BLOCK_SIZE;
        self.data.copy_within(base..base + BLOCK_SIZE, twin);
    }

}

pub struct ChipsView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::ChipsView for ChipsView<S> {
    fn num_folders(&self) -> usize {
        NUM_FOLDERS
    }

    fn equipped_folder_index(&self) -> usize {
        let index = self.save.active()[EQUIPPED_FOLDER_OFFSET] as usize;
        // A value past the folder count would be a misread of the
        // save, not a folder; show the first folder rather than an
        // empty grid.
        if index >= NUM_FOLDERS {
            return 0;
        }
        index
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<Option<usize>> {
        let index = self.save.active()[REGULAR_CHIP_OFFSET + folder_index];
        Some(if index >= 30 { None } else { Some(index as usize) })
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<tango_gamesupport_common::dataview::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= self.folder_size() {
            return None;
        }

        let raw = u16::from_le_bytes(
            self.save.active()[FOLDER_OFFSET + (folder_index * self.folder_size() + chip_index) * 2..][..2]
                .try_into()
                .unwrap(),
        );

        Some(tango_gamesupport_common::dataview::save::Chip {
            id: (raw & 0x1ff) as usize,
            code: num_traits::FromPrimitive::from_u16(raw >> 9)?,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= NUM_PACK_CHIPS || variant >= 4 {
            return None;
        }
        Some(self.save.active()[PACK_OFFSET + id * PACK_ENTRY_SIZE + variant] as usize)
    }
}

impl<S: std::ops::Deref<Target = Save>> ChipsView<S> {
    /// The lowest acquisition key in the pack, or [`PACK_KEY_FIRST`] + 1
    /// when nothing is owned yet, so the first grant lands on
    /// [`PACK_KEY_FIRST`] itself.
    fn lowest_pack_key(&self) -> u16 {
        let block = self.save.active();
        (0..NUM_PACK_CHIPS)
            .flat_map(|id| {
                let entry = PACK_OFFSET + id * PACK_ENTRY_SIZE;
                (0..4).map(move |variant| entry + 4 + variant * 2)
            })
            .map(|off| u16::from_le_bytes(block[off..][..2].try_into().unwrap()))
            .filter(|&key| key != 0)
            .min()
            .unwrap_or(PACK_KEY_FIRST + 1)
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::ChipsViewMut for ChipsView<S> {
    fn set_equipped_folder(&mut self, folder_index: usize) -> bool {
        if folder_index >= NUM_FOLDERS {
            return false;
        }
        self.save.active_mut()[EQUIPPED_FOLDER_OFFSET] = folder_index as u8;
        true
    }

    fn set_chip(
        &mut self,
        folder_index: usize,
        chip_index: usize,
        chip: tango_gamesupport_common::dataview::save::Chip,
    ) -> bool {
        if folder_index >= NUM_FOLDERS || chip_index >= 30 || chip.id > 0x1ff {
            return false;
        }
        let raw = chip.id as u16 | ((chip.code as u16) << 9);
        self.save.active_mut()[FOLDER_OFFSET + (folder_index * 30 + chip_index) * 2..][..2]
            .copy_from_slice(&raw.to_le_bytes());
        true
    }

    fn clear_chip(&mut self, folder_index: usize, chip_index: usize) -> bool {
        if folder_index >= NUM_FOLDERS || chip_index >= 30 {
            return false;
        }
        // 0xffff reads back as an invalid code, so `chip()` returns
        // None — i.e. an empty slot.
        self.save.active_mut()[FOLDER_OFFSET + (folder_index * 30 + chip_index) * 2..][..2].fill(0xff);
        true
    }

    fn set_regular_chip_index(&mut self, folder_index: usize, chip_index: Option<usize>) -> bool {
        if folder_index >= NUM_FOLDERS {
            return false;
        }
        // 0xff (out of the 0..30 range) reads back as "no regular".
        let raw = match chip_index {
            Some(i) if i < 30 => i as u8,
            None => 0xff,
            Some(_) => return false,
        };
        self.save.active_mut()[REGULAR_CHIP_OFFSET + folder_index] = raw;
        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        if id >= NUM_PACK_CHIPS || variant >= 4 || count > 0xff {
            return false;
        }
        let entry = PACK_OFFSET + id * PACK_ENTRY_SIZE;
        self.save.active_mut()[entry + variant] = count as u8;

        // Keep the slot's acquisition key in the shape the game keeps
        // it: zero while never owned, and — for a code being granted
        // for the first time — the next key below every one already
        // handed out, which is how the game's own descending counter
        // would have issued it.
        let key_offset = entry + 4 + variant * 2;
        let key = u16::from_le_bytes(self.save.active()[key_offset..][..2].try_into().unwrap());
        let new_key = if count == 0 {
            0
        } else if key == 0 {
            self.lowest_pack_key().saturating_sub(1)
        } else {
            key
        };
        self.save.active_mut()[key_offset..][..2].copy_from_slice(&new_key.to_le_bytes());
        true
    }

    /// Nothing to rebuild: no GBA-style anti-cheat mirror is known on
    /// the DS cart — the load path verifies the two checksum pairs and
    /// nothing else that has been mapped.
    fn rebuild_anticheat(&mut self) {}
}

pub struct NavicustView<S> {
    save: S,
}

/// One NaviCust part slot, as the save stores it — the GBA game's entry
/// unchanged.
#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawNavicustPart {
    id: u8,
    _unk_01: u8,
    col: u8,
    row: u8,
    rot: u8,
    compressed: u8,
    _unk_06: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == NAVICUST_PART_SIZE);

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::NavicustView for NavicustView<S> {
    fn count(&self) -> usize {
        NUM_NAVICUST_SLOTS
    }

    fn size(&self) -> [usize; 2] {
        [NAVICUST_SIZE, NAVICUST_SIZE]
    }

    fn navicust_part(&self, i: usize) -> Option<tango_gamesupport_common::dataview::save::NavicustPart> {
        if i >= self.count() {
            return None;
        }
        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.active()[NAVICUST_PARTS_OFFSET + i * NAVICUST_PART_SIZE..][..NAVICUST_PART_SIZE],
        );
        if raw.id == 0 {
            return None;
        }
        Some(tango_gamesupport_common::dataview::save::NavicustPart {
            id: raw.id as usize,
            col: raw.col,
            row: raw.row,
            rot: raw.rot,
            compressed: raw.compressed != 0,
        })
    }

    fn materialized(&self) -> tango_gamesupport_common::dataview::navicust::MaterializedNavicust {
        tango_gamesupport_common::dataview::navicust::materialized_from_wram(
            &self.save.active()[NAVICUST_GRID_OFFSET..][..NAVICUST_SIZE * NAVICUST_SIZE],
            [NAVICUST_SIZE, NAVICUST_SIZE],
        )
    }

    fn navicust_color_bar(&self) -> Vec<Option<tango_gamesupport_common::dataview::rom::NavicustPartColor>> {
        self.save.active()[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN]
            .iter()
            .map(|&raw| crate::rom::navicust_part_color(raw))
            .collect()
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::NavicustViewMut
    for NavicustView<S>
{
    fn set_navicust_part(
        &mut self,
        i: usize,
        part: Option<tango_gamesupport_common::dataview::save::NavicustPart>,
    ) -> bool {
        if i >= NUM_NAVICUST_SLOTS {
            return false;
        }
        let raw = match part {
            Some(part) => {
                if part.id >= crate::NUM_NAVICUST_PARTS {
                    return false;
                }
                RawNavicustPart {
                    id: part.id as u8,
                    col: part.col,
                    row: part.row,
                    rot: part.rot,
                    compressed: u8::from(part.compressed),
                    ..Default::default()
                }
            }
            // An all-zero part (id 0) reads back as an empty slot.
            None => RawNavicustPart::default(),
        };
        self.save.active_mut()[NAVICUST_PARTS_OFFSET + i * NAVICUST_PART_SIZE..][..NAVICUST_PART_SIZE]
            .copy_from_slice(bytemuck::bytes_of(&raw));
        true
    }

    fn clear_materialized(&mut self) {
        self.save.active_mut()[NAVICUST_GRID_OFFSET..][..NAVICUST_GRID_SECTION_SIZE].fill(0);
        self.save.active_mut()[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN].fill(0);
    }

    fn rebuild_materialized(&mut self, assets: &dyn tango_gamesupport_common::dataview::rom::Assets) {
        let materialized = tango_gamesupport_common::dataview::navicust::materialize(
            &*self,
            [NAVICUST_SIZE, NAVICUST_SIZE],
            assets,
        );
        let mut grid = [0u8; NAVICUST_GRID_SECTION_SIZE];
        for (cell, slot) in grid.iter_mut().zip(materialized) {
            // Cells hold the slot + 1, so 0 stays "empty".
            *cell = slot.map(|slot| slot as u8 + 1).unwrap_or(0);
        }
        self.save.active_mut()[NAVICUST_GRID_OFFSET..][..NAVICUST_GRID_SECTION_SIZE].copy_from_slice(&grid);

        // The colour bar: the distinct part colours in placement order.
        let bar = tango_gamesupport_common::dataview::navicust::materialize_color_bar(&*self, assets);
        let mut bytes = [0u8; NAVICUST_COLOR_BAR_LEN];
        for (slot, color) in bar.iter().flatten().enumerate().take(NAVICUST_COLOR_BAR_LEN) {
            bytes[slot] = tango_gamesupport_common::dataview::navicust::color_to_raw(
                color,
                crate::rom::navicust_part_color,
            );
        }
        self.save.active_mut()[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN].copy_from_slice(&bytes);
    }
}

/// How wide and tall the grid is.
const NAVICUST_SIZE: usize = 5;

pub struct NaviView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::NaviView for NaviView<S> {
    fn navi(&self) -> usize {
        self.save.navi()
    }

    /// The HP the navi brings. MegaMan's own is what HP Memories bought
    /// plus what his NaviCust adds — the same sum the GBA game makes,
    /// minus the patch cards this cart has none of. A team navi reports
    /// the effective figure its own record carries.
    fn max_hp(&self, _assets: &dyn tango_gamesupport_common::dataview::rom::Assets) -> u16 {
        let navi = self.navi();
        let Some([base, _current, effective]) = self.save.navi_stats(navi) else {
            return 0;
        };
        if navi != 0 {
            return effective;
        }

        let mut max_hp = base;
        if let Some(navicust) = tango_gamesupport_common::dataview::save::Save::view_navicust(&*self.save) {
            for part in placed_parts(&*navicust) {
                for effect in crate::rom::navicust::navicust_part_effects(part) {
                    if let crate::rom::navicust::NavicustEffect::MaxHp(n) = effect {
                        max_hp += *n;
                    }
                }
            }
        }
        max_hp
    }

    /// What a folder may hold. The class caps come off the NaviCust's
    /// command line the way the GBA game reads them, Regular memory off
    /// the folder cluster, and the Dark cap and per-chip copy rule are
    /// the game's own constants — with no patch cards on this cart to
    /// move any of them.
    fn folder_limits(
        &self,
        _assets: &dyn tango_gamesupport_common::dataview::rom::Assets,
    ) -> tango_gamesupport_common::dataview::save::FolderLimits {
        let mut mega: isize = BASE_MEGA_LIMIT;
        let mut giga: usize = BASE_GIGA_LIMIT;

        if let Some(navicust) = tango_gamesupport_common::dataview::save::Save::view_navicust(&*self.save) {
            for part in command_line_parts(&*navicust) {
                for effect in crate::rom::navicust::navicust_part_effects(part) {
                    match effect {
                        crate::rom::navicust::NavicustEffect::MegaLimit(n) => mega += *n as isize,
                        crate::rom::navicust::NavicustEffect::GigaLimit(n) => giga += *n as usize,
                        _ => {}
                    }
                }
            }
        }

        tango_gamesupport_common::dataview::save::FolderLimits {
            mega_limit: Some(mega.clamp(0, MAX_CLASS_LIMIT as isize) as usize),
            giga_limit: Some(giga.clamp(0, MAX_CLASS_LIMIT)),
            dark_limit: Some(DARK_LIMIT),
            reg_memory: Some(self.save.active()[REGULAR_MEMORY_OFFSET]),
            max_copies: |chip| {
                if chip.dark() {
                    return 1;
                }
                match chip.class() {
                    tango_gamesupport_common::dataview::rom::ChipClass::Mega
                    | tango_gamesupport_common::dataview::rom::ChipClass::Giga => 1,
                    tango_gamesupport_common::dataview::rom::ChipClass::Standard => 4,
                    _ => 0,
                }
            },
            ..Default::default()
        }
    }
}

/// What a folder may hold before the NaviCust moves it, and the ceiling
/// either class cap is clamped to — the GBA game's numbers, which the
/// cart keeps.
const BASE_MEGA_LIMIT: isize = 5;
const BASE_GIGA_LIMIT: usize = 1;
const MAX_CLASS_LIMIT: usize = 10;
const DARK_LIMIT: usize = 3;

/// The part ids actually on the grid, each counted once however many
/// cells it covers.
fn placed_parts(navicust: &dyn tango_gamesupport_common::dataview::save::NavicustView) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    navicust
        .materialized()
        .into_iter()
        .flatten()
        .filter(|slot| seen.insert(*slot))
        .filter_map(|slot| navicust.navicust_part(slot).map(|part| part.id))
        .collect()
}

/// The same, restricted to the command line — the row a program has to
/// sit on for its folder effect to count.
fn command_line_parts(navicust: &dyn tango_gamesupport_common::dataview::save::NavicustView) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    navicust
        .materialized()
        .row(NAVICUST_COMMAND_LINE)
        .iter()
        .flatten()
        .copied()
        .filter(|slot| seen.insert(*slot))
        .filter_map(|slot| navicust.navicust_part(slot).map(|part| part.id))
        .collect()
}

/// Which row of the grid is the command line. The cart's own layout
/// (see [`crate::rom`]) says the same; this is the save layer's copy so
/// the folder limits don't have to be handed the cart to read it.
const NAVICUST_COMMAND_LINE: usize = 2;

pub struct AutoBattleDataView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> AutoBattleDataView<S> {
    fn count_at(&self, base: usize, id: usize) -> Option<usize> {
        if id >= crate::NUM_AUTO_BATTLE_DATA_CHIPS {
            return None;
        }
        let raw = self.save.active().get(base + id * std::mem::size_of::<u16>()..)?;
        Some(u16::from_le_bytes(raw.get(..2)?.try_into().unwrap()) as usize)
    }
}

impl<S: std::ops::DerefMut<Target = Save>> AutoBattleDataView<S> {
    fn set_count_at(&mut self, base: usize, id: usize, count: usize) -> bool {
        if id >= crate::NUM_AUTO_BATTLE_DATA_CHIPS || count > u16::MAX as usize {
            return false;
        }
        self.save.active_mut()[base + id * std::mem::size_of::<u16>()..][..2]
            .copy_from_slice(&(count as u16).to_le_bytes());
        true
    }

    /// Write the deck the shared materializer built, leaving the combo
    /// slots as the game left them (see
    /// [`AUTO_BATTLE_DATA_COMBO_SLOTS`]).
    fn set_materialized(
        &mut self,
        materialized: &tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData,
    ) {
        for (slot, chip) in materialized.as_slice().iter().enumerate() {
            if AUTO_BATTLE_DATA_COMBO_SLOTS.contains(&slot) {
                continue;
            }
            let raw = chip.map(|chip| chip as u16).unwrap_or(0xffff);
            self.save.active_mut()[AUTO_BATTLE_DATA_OFFSET + slot * std::mem::size_of::<u16>()..][..2]
                .copy_from_slice(&raw.to_le_bytes());
        }
    }
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::AutoBattleDataView
    for AutoBattleDataView<S>
{
    fn chip_use_count(&self, id: usize) -> Option<usize> {
        self.count_at(CHIP_USE_COUNT_OFFSET, id)
    }

    fn secondary_chip_use_count(&self, id: usize) -> Option<usize> {
        self.count_at(SECONDARY_CHIP_USE_COUNT_OFFSET, id)
    }

    fn materialized(&self) -> tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData {
        tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData::from_wram(
            &self.save.active()[AUTO_BATTLE_DATA_OFFSET..]
                [..NUM_AUTO_BATTLE_DATA_SLOTS * std::mem::size_of::<u16>()],
        )
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::AutoBattleDataViewMut
    for AutoBattleDataView<S>
{
    fn set_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        self.set_count_at(CHIP_USE_COUNT_OFFSET, id, count)
    }

    fn set_secondary_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        self.set_count_at(SECONDARY_CHIP_USE_COUNT_OFFSET, id, count)
    }

    fn clear_materialized(&mut self) {
        self.set_materialized(
            &tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData::empty(),
        );
    }

    fn rebuild_materialized(&mut self, assets: &dyn tango_gamesupport_common::dataview::rom::Assets) {
        let materialized =
            tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData::materialize(
                &*self, assets,
            );
        self.set_materialized(&materialized);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_gamesupport_common::dataview::save::{ChipCode, ChipsViewMut, NavicustPart, Save as _};

    fn chip_bytes(id: u16, code: u16) -> [u8; 2] {
        (id | (code << 9)).to_le_bytes()
    }

    fn chips_mut(save: &mut Save) -> Box<dyn ChipsViewMut + '_> {
        save.view_chips_mut().expect("the folder is editable")
    }

    fn plausible() -> Vec<u8> {
        let mut data = vec![0u8; SIZE];
        for block in 0..SIZE / BLOCK_SIZE {
            data[block * BLOCK_SIZE + MAGIC_OFFSET..][..MAGIC.len()].copy_from_slice(MAGIC);
        }
        data
    }

    #[test]
    fn accepts_a_dump_of_this_game() {
        assert!(SaveSet::parse(&plausible()).is_ok());
    }

    #[test]
    fn accepts_a_dump_with_a_single_intact_block() {
        let mut data = vec![0u8; SIZE];
        data[5 * BLOCK_SIZE + MAGIC_OFFSET..][..MAGIC.len()].copy_from_slice(MAGIC);
        assert!(SaveSet::parse(&data).is_ok());
    }

    /// A dump whose erased tail was trimmed off is padded back out, and
    /// the blocks that fell in the trim read as the unformatted flash
    /// they are.
    #[test]
    fn accepts_a_dump_short_of_the_flash() {
        use tango_gamesupport_common::dataview::save::Save as _;
        let mut full = vec![ERASED; SIZE];
        for block in 0..2 {
            full[block * BLOCK_SIZE + MAGIC_OFFSET..][..MAGIC.len()].copy_from_slice(MAGIC);
            full[block * BLOCK_SIZE + FILE_SLOT_OFFSET] = 0;
        }
        // Everything past the two written blocks is erased flash, so
        // cutting it off loses nothing.
        let set = SaveSet::parse(&full[..2 * BLOCK_SIZE]).unwrap();
        assert_eq!(set.slots(), vec![0]);
        assert_eq!(set.current().to_sram_dump(), full);
    }

    /// The shape a DS save manager writes: the cart's flash, then as
    /// much erased padding again. The cart is in there; the rest is
    /// address space the chip does not answer for.
    #[test]
    fn accepts_a_dump_longer_than_the_flash() {
        use tango_gamesupport_common::dataview::save::Save as _;
        let full = plausible();
        let mut padded = full.clone();
        padded.resize(SIZE * 2, ERASED);
        let set = SaveSet::parse(&padded).unwrap();
        assert_eq!(set.slots(), SaveSet::parse(&full).unwrap().slots());
        assert_eq!(set.current().to_sram_dump(), full);
    }

    #[test]
    fn rejects_another_games_dump_of_the_same_size() {
        assert!(SaveSet::parse(&vec![0u8; SIZE]).is_err());
    }

    #[test]
    fn round_trips_its_bytes() {
        let data = plausible();
        let save = SaveSet::parse(&data).unwrap().current();
        assert_eq!(save.to_sram_dump(), data);
    }

    #[test]
    fn reads_the_folder() {
        let mut data = plausible();
        data[FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(193, 12));
        // Last slot of the last folder.
        data[FOLDER_OFFSET + (2 * 30 + 29) * 2..][..2].copy_from_slice(&chip_bytes(207, 26));

        let save = SaveSet::parse(&data).unwrap().current();
        let chips = save.view_chips().unwrap();
        assert_eq!(
            chips.chip(0, 0),
            Some(tango_gamesupport_common::dataview::save::Chip {
                id: 193,
                code: ChipCode::M,
            })
        );
        assert_eq!(
            chips.chip(2, 29),
            Some(tango_gamesupport_common::dataview::save::Chip {
                id: 207,
                code: ChipCode::Star,
            })
        );
        assert_eq!(chips.chip(3, 0), None);
        assert_eq!(chips.chip(0, 30), None);
    }

    #[test]
    fn reads_the_newest_generation() {
        let mut data = plausible();
        // Blocks 0/1 hold generation 5, blocks 2/3 generation 6: the
        // game alternates pairs, so the higher counter is current.
        data[0 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&5u32.to_le_bytes());
        data[1 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&5u32.to_le_bytes());
        data[2 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&6u32.to_le_bytes());
        data[3 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&6u32.to_le_bytes());
        data[0 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(1, 0));
        data[2 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(2, 0));

        let save = SaveSet::parse(&data).unwrap().current();
        let chips = save.view_chips().unwrap();
        assert_eq!(chips.chip(0, 0).unwrap().id, 2);
    }

    #[test]
    fn skips_a_damaged_block_no_matter_its_counter() {
        let mut data = plausible();
        // The highest counter sits in a block whose tag is gone; its
        // intact twin (same generation) is what should be read.
        data[2 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&9u32.to_le_bytes());
        data[2 * BLOCK_SIZE + MAGIC_OFFSET] = 0;
        data[3 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&9u32.to_le_bytes());
        data[3 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(7, 3));

        let save = SaveSet::parse(&data).unwrap().current();
        let chips = save.view_chips().unwrap();
        assert_eq!(chips.chip(0, 0).unwrap().id, 7);
    }

    #[test]
    fn reads_the_equipped_folder_and_regular_chips() {
        let mut data = plausible();
        data[EQUIPPED_FOLDER_OFFSET] = 1;
        data[REGULAR_CHIP_OFFSET] = 0xff;
        data[REGULAR_CHIP_OFFSET + 1] = 7;

        let save = SaveSet::parse(&data).unwrap().current();
        let chips = save.view_chips().unwrap();
        assert_eq!(chips.equipped_folder_index(), 1);
        assert_eq!(chips.regular_chip_index(0), Some(None));
        assert_eq!(chips.regular_chip_index(1), Some(Some(7)));
    }

    #[test]
    fn an_equipped_index_past_the_folders_reads_as_the_first() {
        let mut data = plausible();
        data[EQUIPPED_FOLDER_OFFSET] = 9;

        let save = SaveSet::parse(&data).unwrap().current();
        assert_eq!(save.view_chips().unwrap().equipped_folder_index(), 0);
    }

    #[test]
    fn checksum_matches_the_games_algorithm() {
        // By hand: (0x1234 ^ 4) + (0xabcd ^ 2).
        assert_eq!(checksum(&[0x34, 0x12, 0xcd, 0xab]), 0x1230u16.wrapping_add(0xabcf));
    }

    /// Put `part` in slot `slot` of the grid's `cells`, so the views
    /// that read the materialized grid see it placed.
    fn place(data: &mut [u8], slot: usize, id: u8, cells: &[usize]) {
        data[NAVICUST_PARTS_OFFSET + slot * NAVICUST_PART_SIZE] = id;
        for &cell in cells {
            data[NAVICUST_GRID_OFFSET + cell] = slot as u8 + 1;
        }
    }

    #[test]
    fn reads_and_writes_navicust_parts() {
        let save = SaveSet::parse(&plausible()).unwrap().current();
        let mut save = save;
        {
            let mut navicust = save.view_navicust_mut().expect("the navicust is editable");
            assert!(navicust.set_navicust_part(
                3,
                Some(NavicustPart {
                    id: 188,
                    col: 1,
                    row: 4,
                    rot: 2,
                    compressed: true,
                })
            ));
            // Past the cart's table, and past the slots.
            assert!(!navicust.set_navicust_part(
                3,
                Some(NavicustPart {
                    id: crate::NUM_NAVICUST_PARTS,
                    col: 0,
                    row: 0,
                    rot: 0,
                    compressed: false,
                })
            ));
            assert!(!navicust.set_navicust_part(NUM_NAVICUST_SLOTS, None));
        }
        let navicust = save.view_navicust().unwrap();
        assert_eq!(navicust.count(), NUM_NAVICUST_SLOTS);
        assert_eq!(navicust.size(), [NAVICUST_SIZE, NAVICUST_SIZE]);
        let part = navicust.navicust_part(3).unwrap();
        assert_eq!((part.id, part.col, part.row, part.rot, part.compressed), (188, 1, 4, 2, true));
        // An id of 0 is an empty slot, not part 0.
        assert!(navicust.navicust_part(0).is_none());
    }

    /// MegaMan's HP is what the record says plus what the NaviCust
    /// adds; a team navi reports its own record's effective figure.
    #[test]
    fn the_navi_reports_hp_the_navicust_moves() {
        let mut data = plausible();
        data[NAVI_STATS_OFFSET..][..2].copy_from_slice(&1000u16.to_le_bytes());
        // HP+400 (template 46) on the grid, and HP+100 (43) in a slot
        // the grid doesn't show — an unplaced part grants nothing.
        place(&mut data, 0, 46 * 4, &[0, 1]);
        place(&mut data, 1, 43 * 4, &[]);

        let mut save = SaveSet::parse(&data).unwrap().current();
        let assets = crate::rom::Assets::new(&crate::rom::A5TE_00, crate::rom::EN_CHARSET, vec![]);
        assert_eq!(save.view_navi().unwrap().max_hp(&assets), 1400);

        // A file being played as a team navi reports that navi's own
        // effective HP, and hands out no navicust to edit.
        save.active_mut()[NAVI_OFFSET] = 2;
        let stats = NAVI_RECORD_OFFSET + 2 * NAVI_RECORD_SIZE + NAVI_STATS_INTO_RECORD;
        save.active_mut()[stats + 4..][..2].copy_from_slice(&777u16.to_le_bytes());
        assert_eq!(save.view_navi().unwrap().max_hp(&assets), 777);
        assert!(save.view_navicust().is_none());
        assert!(save.view_navicust_mut().is_none());
    }

    /// The folder's limits: the class caps move with the command line's
    /// own programs, Regular memory comes off the folder cluster.
    #[test]
    fn folder_limits_follow_the_command_line() {
        let mut data = plausible();
        data[REGULAR_MEMORY_OFFSET] = 60;
        // MegFldr2 (+2) on the command line, GigFldr1 (+1) off it.
        place(&mut data, 0, 5 * 4, &[NAVICUST_COMMAND_LINE * NAVICUST_SIZE]);
        place(&mut data, 1, 6 * 4, &[0]);

        let save = SaveSet::parse(&data).unwrap().current();
        let assets = crate::rom::Assets::new(&crate::rom::A5TE_00, crate::rom::EN_CHARSET, vec![]);
        let limits = save.view_navi().unwrap().folder_limits(&assets);
        assert_eq!(limits.mega_limit, Some(BASE_MEGA_LIMIT as usize + 2));
        assert_eq!(limits.giga_limit, Some(BASE_GIGA_LIMIT));
        assert_eq!(limits.dark_limit, Some(DARK_LIMIT));
        assert_eq!(limits.reg_memory, Some(60));
    }

    #[test]
    fn auto_battle_data_counts_round_trip() {
        let mut save = SaveSet::parse(&plausible()).unwrap().current();
        {
            let mut abd = save.view_auto_battle_data_mut().unwrap();
            assert!(abd.set_chip_use_count(5, 300));
            assert!(abd.set_secondary_chip_use_count(5, 7));
            // Past the arrays the cart keeps.
            assert!(!abd.set_chip_use_count(crate::NUM_AUTO_BATTLE_DATA_CHIPS, 1));
        }
        let abd = save.view_auto_battle_data().unwrap();
        assert_eq!(abd.chip_use_count(5), Some(300));
        assert_eq!(abd.secondary_chip_use_count(5), Some(7));
        assert_eq!(abd.chip_use_count(crate::NUM_AUTO_BATTLE_DATA_CHIPS), None);
    }

    /// Rebuilding the deck leaves the cart's own combo markers alone —
    /// the shared materializer has nothing to say about those slots.
    #[test]
    fn rebuilding_the_deck_keeps_the_combo_slots() {
        let mut data = plausible();
        let combo = AUTO_BATTLE_DATA_OFFSET + AUTO_BATTLE_DATA_COMBO_SLOTS.start * 2;
        data[combo..][..2].copy_from_slice(&0x8000u16.to_le_bytes());
        data[AUTO_BATTLE_DATA_OFFSET..][..2].copy_from_slice(&123u16.to_le_bytes());

        let mut save = SaveSet::parse(&data).unwrap().current();
        {
            let mut abd = save.view_auto_battle_data_mut().unwrap();
            abd.clear_materialized();
        }
        let block = save.active();
        assert_eq!(&block[combo..][..2], &0x8000u16.to_le_bytes(), "a combo slot was blanked");
        assert_eq!(
            &block[AUTO_BATTLE_DATA_OFFSET..][..2],
            &0xffffu16.to_le_bytes(),
            "an ordinary slot was not cleared"
        );
    }

    /// A cart with both files: file 0 in blocks 0-3 (generation 9 the
    /// live one), file 1 in blocks 4/5, each holding a distinct chip in
    /// folder 0 slot 0 so the tests can tell them apart.
    fn two_files() -> Vec<u8> {
        let mut data = plausible();
        for block in 0..2 {
            data[block * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&9u32.to_le_bytes());
        }
        for block in 2..4 {
            data[block * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&8u32.to_le_bytes());
        }
        for block in 4..6 {
            data[block * BLOCK_SIZE + FILE_SLOT_OFFSET] = 1;
            data[block * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&3u32.to_le_bytes());
        }
        data[0 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(1, 0));
        data[2 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(99, 0));
        data[4 * BLOCK_SIZE + FOLDER_OFFSET..][..2].copy_from_slice(&chip_bytes(2, 0));
        data
    }

    #[test]
    fn hands_out_a_save_per_file() {
        let data = two_files();
        let set = SaveSet::parse(&data).unwrap();
        assert_eq!(set.slots(), vec![0, 1]);

        // Each file reads its own live block — file 0's older
        // generation (chip 99) is not what it hands out.
        let file0 = set.save(0).unwrap();
        let file1 = set.save(1).unwrap();
        assert_eq!(file0.slot(), 0);
        assert_eq!(file1.slot(), 1);
        assert_eq!(file0.view_chips().unwrap().chip(0, 0).unwrap().id, 1);
        assert_eq!(file1.view_chips().unwrap().chip(0, 0).unwrap().id, 2);

        // Every save knows the whole cart's files, for a file picker.
        assert_eq!(file0.slots(), &[0, 1]);
        assert_eq!(file1.slots(), &[0, 1]);

        // The most recently saved file is what the editor opens on, and
        // handing out a file never touches the dump's bytes.
        assert_eq!(set.current().slot(), 0);
        assert_eq!(file1.to_sram_dump(), data);
        assert!(set.save(9).is_none());
    }

    #[test]
    fn a_session_boots_the_dump_untouched() {
        let data = two_files();
        let set = SaveSet::parse(&data).unwrap();
        // Reading a file never touches the dump's bytes: which file a
        // session plays is already in them (the generation counters),
        // so handing one out has nothing to write.
        assert_eq!(set.save(0).unwrap().to_sram_dump(), data);
        assert_eq!(set.save(1).unwrap().to_sram_dump(), data);
    }

    /// Picking a file is an edit to the cartridge: the file becomes the
    /// one the game itself calls most recently saved, which is what
    /// [`SaveSet::current`] — and so every session, recording and
    /// priming walk — reads.
    #[test]
    fn making_a_file_current_moves_what_the_set_hands_out() {
        let data = two_files();
        let set = SaveSet::parse(&data).unwrap();
        assert_eq!(set.current().slot(), 0);

        let mut file1 = set.save(1).unwrap();
        file1.make_current();
        file1.rebuild_checksum();
        let dump = file1.to_sram_dump();
        let set = SaveSet::parse(&dump).unwrap();
        assert_eq!(set.current().slot(), 1);
        // Both files are still there, each still reading its own block.
        assert_eq!(set.slots(), vec![0, 1]);
        assert_eq!(set.save(0).unwrap().view_chips().unwrap().chip(0, 0).unwrap().id, 1);
        assert_eq!(set.save(1).unwrap().view_chips().unwrap().chip(0, 0).unwrap().id, 2);

        // Already leading: nothing moves, and picking it again is a
        // no-op rather than another bump.
        let mut again = set.save(1).unwrap();
        let before = again.to_sram_dump();
        again.make_current();
        assert_eq!(again.to_sram_dump(), before);
    }

    /// The cross is the game's own byte, per file, and survives the
    /// round trip through a rebuilt block.
    #[test]
    fn the_cross_round_trips_per_file() {
        let mut data = two_files();
        // File 0 is Team ProtoMan, file 1 Team Colonel — which is what
        // picks between the two BassCross values. (Their live blocks are
        // 0 and 4; see `two_files`.)
        data[0 * BLOCK_SIZE + TEAM_OFFSET] = 0;
        data[4 * BLOCK_SIZE + TEAM_OFFSET] = 1;
        let set = SaveSet::parse(&data).unwrap();
        assert_eq!(set.save(0).unwrap().cross(), Cross::None);
        assert_eq!(Cross::bass_for(set.save(0).unwrap().team()), Cross::BassProto);
        assert_eq!(Cross::bass_for(set.save(1).unwrap().team()), Cross::BassColonel);

        let mut file1 = set.save(1).unwrap();
        file1.set_cross(Cross::Sol);
        file1.rebuild_checksum();
        let set = SaveSet::parse(&file1.to_sram_dump()).unwrap();
        assert_eq!(set.save(1).unwrap().cross(), Cross::Sol);
        // The other file is untouched — the byte lives in each file's
        // own block.
        assert_eq!(set.save(0).unwrap().cross(), Cross::None);
    }

    #[test]
    fn each_file_edits_only_its_own_block() {
        let data = two_files();
        let set = SaveSet::parse(&data).unwrap();

        let mut file1 = set.save(1).unwrap();
        {
            let mut chips = chips_mut(&mut file1);
            assert!(chips.set_chip(
                0,
                0,
                tango_gamesupport_common::dataview::save::Chip {
                    id: 55,
                    code: ChipCode::B,
                },
            ));
        }
        file1.rebuild_checksum();

        // File 0's blocks are untouched, and re-reading the edited dump
        // through the set sees the edit only in file 1.
        let dump = file1.to_sram_dump();
        assert_eq!(dump[..2 * BLOCK_SIZE], data[..2 * BLOCK_SIZE]);
        let set = SaveSet::parse(&dump).unwrap();
        assert_eq!(set.save(0).unwrap().view_chips().unwrap().chip(0, 0).unwrap().id, 1);
        assert_eq!(set.save(1).unwrap().view_chips().unwrap().chip(0, 0).unwrap().id, 55);
    }

    #[test]
    fn edits_rebuild_into_a_block_the_game_would_load() {
        let mut data = plausible();
        // Make block 2's pair the active file contents.
        data[2 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&7u32.to_le_bytes());
        data[3 * BLOCK_SIZE + GENERATION_OFFSET..][..4].copy_from_slice(&7u32.to_le_bytes());

        let mut save = SaveSet::parse(&data).unwrap().current();
        {
            let mut chips = chips_mut(&mut save);
            assert!(chips.set_chip(
                0,
                0,
                tango_gamesupport_common::dataview::save::Chip {
                    id: 42,
                    code: ChipCode::C,
                },
            ));
            assert!(chips.set_equipped_folder(2));
            assert!(chips.set_regular_chip_index(0, Some(5)));
            assert!(chips.set_pack_count(42, 2, 99));
        }
        save.rebuild_checksum();

        let dump = save.to_sram_dump();
        // The edited pair is mirrored...
        assert_eq!(dump[2 * BLOCK_SIZE..3 * BLOCK_SIZE], dump[3 * BLOCK_SIZE..4 * BLOCK_SIZE]);
        // ...its checksum pairs hold the game's invariants...
        let block = &dump[2 * BLOCK_SIZE..3 * BLOCK_SIZE];
        let word = |o: usize| u16::from_le_bytes(block[o..o + 2].try_into().unwrap());
        assert_eq!(word(CHECKSUM_OFFSET).wrapping_add(word(CHECKSUM_OFFSET + 2)), 0);
        assert_eq!(word(CHECKSUM_OFFSET + 4).wrapping_add(word(CHECKSUM_OFFSET + 6)), 0);
        assert_eq!(word(CHECKSUM_OFFSET + 4), checksum(&block[..SAVE_IMAGE_SIZE]));
        assert_eq!(
            word(CHECKSUM_OFFSET),
            checksum(&block[FOOTER_SUM_START..FOOTER_SUM_END])
        );
        // ...including the interior byte-sum the game verifies at load.
        // These edits change byte VALUES (a reorder wouldn't — byte
        // sums are permutation-invariant, which is how a stale interior
        // once hid), so a stale one would boot the new-game path.
        let image = &block[..SAVE_IMAGE_SIZE];
        assert_eq!(
            u32::from_le_bytes(image[INTERIOR_CHECKSUM_OFFSET..][..4].try_into().unwrap()),
            image.iter().map(|&v| v as u32).sum::<u32>().wrapping_sub(
                image[INTERIOR_CHECKSUM_OFFSET..][..4]
                    .iter()
                    .map(|&v| v as u32)
                    .sum::<u32>()
            ),
        );
        // ...and a re-parse sees the edits.
        let reparsed = SaveSet::parse(&dump).unwrap().current();
        let chips = reparsed.view_chips().unwrap();
        assert_eq!(chips.chip(0, 0).unwrap().id, 42);
        assert_eq!(chips.equipped_folder_index(), 2);
        assert_eq!(chips.regular_chip_index(0), Some(Some(5)));
        assert_eq!(chips.pack_count(42, 2), Some(99));
    }

    #[test]
    fn pack_counts_keep_their_acquisition_keys_in_shape() {
        let data = plausible();
        let mut save = SaveSet::parse(&data).unwrap().current();
        let key_at = |save: &Save, id: usize, variant: usize| {
            let off = PACK_OFFSET + id * PACK_ENTRY_SIZE + 4 + variant * 2;
            u16::from_le_bytes(save.active()[off..][..2].try_into().unwrap())
        };

        // The first grant on an empty pack takes the first key, and the
        // next one takes the key below it.
        let mut chips = chips_mut(&mut save);
        assert!(chips.set_pack_count(7, 2, 4));
        drop(chips);
        assert_eq!(key_at(&save, 7, 2), PACK_KEY_FIRST);
        let mut chips = chips_mut(&mut save);
        assert!(chips.set_pack_count(9, 0, 1));
        drop(chips);
        assert_eq!(key_at(&save, 9, 0), PACK_KEY_FIRST - 1);

        // A key sticks across count changes and clears with the count.
        let mut chips = chips_mut(&mut save);
        assert!(chips.set_pack_count(7, 2, 1));
        drop(chips);
        assert_eq!(key_at(&save, 7, 2), PACK_KEY_FIRST);
        let mut chips = chips_mut(&mut save);
        assert!(chips.set_pack_count(7, 2, 0));
        drop(chips);
        assert_eq!(key_at(&save, 7, 2), 0);

        // Ids without a pack slot stay unpokeable.
        let mut chips = chips_mut(&mut save);
        assert!(!chips.set_pack_count(NUM_PACK_CHIPS, 0, 1));
        assert_eq!(chips.pack_count(NUM_PACK_CHIPS, 0), None);
    }
}
