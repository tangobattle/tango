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
//! Recognition is settled: the game stamps its own format tag into
//! every block it formats, and finding that tag intact is what puts a
//! dump in the save picker. The interior mapping reaches far enough to
//! edit: the chip folders and pack (the GBA game's own shapes), the
//! equipped-folder cluster, and — read out of the ARM9's own save code
//! — the flash checksums, so an edited block can be made acceptable to
//! the game again. Editing is nonetheless turned off for now: the
//! write path is all here, but `Save::view_chips_mut` hands out
//! nothing, so the editor is the plain viewer.

use tango_gamesupport_common::dataview::save::Error;

/// A DS cartridge dump is 256 KiB.
pub const SIZE: usize = 0x4_0000;

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
/// HP, current HP, effective max HP. Nothing reads it yet — it is the
/// anchor [`EQUIPPED_FOLDER_OFFSET`] was found against.
pub const NAVI_STATS_OFFSET: usize = 0x2eaa;

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
    /// Accept `data` if it is a dump of this game's flash: the right
    /// size, with the game's own format tag in at least one block.
    ///
    /// The game falls back to a generation's twin copy when one block
    /// is damaged, so a single intact tag is as much as it requires
    /// too.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() != SIZE {
            return Err(Error::InvalidSize(data.len()));
        }

        // A file's live block is its intact block with the highest
        // generation counter; the game default-formats every block on
        // first save (an unused file carries baked defaults, tag and
        // all), so tag presence can't tell what's in use — only real
        // saves move the counter. A generation's two mirrored copies
        // are identical on an intact cart, so the tie between them is
        // broken toward the first block just to be deterministic.
        let mut files: Vec<(u8, usize)> = Vec::new();
        for block in (0..SIZE / BLOCK_SIZE).filter(|&block| formatted(data, block)) {
            let slot = data[block * BLOCK_SIZE + FILE_SLOT_OFFSET];
            match files.iter_mut().find(|(s, _)| *s == slot) {
                Some(file) if generation(data, block) > generation(data, file.1) => file.1 = block,
                Some(_) => {}
                None => files.push((slot, block)),
            }
        }
        if files.is_empty() {
            return Err(Error::InvalidGameName(data[MAGIC_OFFSET..][..MAGIC.len()].to_vec()));
        }
        files.sort_unstable();

        Ok(SaveSet {
            data: data.to_vec(),
            files,
        })
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

    /// The file saved most recently — what the editor opens on, being
    /// the one the player was last playing.
    pub fn current(&self) -> Save {
        let &(slot, _) = self
            .files
            .iter()
            .max_by_key(|&&(_, block)| generation(&self.data, block))
            .expect("a parsed SaveSet holds at least one file");
        self.save(slot).expect("the slot came from this set")
    }
}

/// Which of the cartridge's files a session plays — this game's
/// [`tango_match::SessionPayload`]. Minted by the save view's file
/// picker, revealed with the netplay commit, recorded per replay side,
/// and downcast back by the priming walk, which steers the game's own
/// file select to this slot. One serialized byte: the file-select slot
/// ([`Save::slot`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlayedFile(pub u8);

impl tango_match::SessionPayload for PlayedFile {
    fn serialize(&self) -> Vec<u8> {
        vec![self.0]
    }

    fn clone_box(&self) -> tango_match::BoxedSessionPayload {
        Box::new(*self)
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
}

impl tango_gamesupport_common::dataview::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    /// Editing is off for this game for now: no writable view, so the
    /// editor is the plain viewer (an unwritable section is the ordinary
    /// answer here — see `Editability`). The write path below is intact
    /// and covered by the tests, which build the view directly; turning
    /// editing back on is handing out `Some` from this method again.
    fn view_chips_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::ChipsViewMut + '_>> {
        None
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

#[cfg(test)]
mod tests {
    use super::*;
    use tango_gamesupport_common::dataview::save::{ChipCode, ChipsView as _, ChipsViewMut as _, Save as _};

    fn chip_bytes(id: u16, code: u16) -> [u8; 2] {
        (id | (code << 9)).to_le_bytes()
    }

    /// The writable chips view. [`Save::view_chips_mut`] hands out
    /// `None` while editing is off, so the tests reach the write path
    /// the way that method would once it is back on.
    fn chips_mut(save: &mut Save) -> ChipsView<&mut Save> {
        ChipsView { save }
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

    #[test]
    fn rejects_the_wrong_size() {
        let mut data = plausible();
        data.truncate(SIZE / 2);
        assert!(SaveSet::parse(&data).is_err());
        data.resize(SIZE * 2, 0);
        assert!(SaveSet::parse(&data).is_err());
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
        // Whichever file is being played, the cartridge a session gets
        // is the dump as it stands — the choice travels beside the
        // bytes (a [`PlayedFile`] session payload), never in them.
        assert_eq!(set.save(0).unwrap().to_sram_dump(), data);
        assert_eq!(set.save(1).unwrap().to_sram_dump(), data);
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
