//! The BCC save container and its program-deck view.
//!
//! # SRAM container
//!
//! 32 KiB of battery SRAM holding four 0x2000-byte blocks: two save
//! files (the game's two GP data slots), each stored twice — block
//! index = file + copy*2. A block is a 16-byte header (magic `"INTI"`,
//! u32 byte-sum of the payload, u32 payload length, then
//! `{0, 2, file, 1}`) followed by the 0x1734-byte payload, which is the
//! block the game keeps live in EWRAM.
//!
//! # Program decks
//!
//! The payload holds three deck blocks of 0x90 bytes at
//! [`DECKS_OFFSET`], one per Navi the player can field. Each is:
//!
//! ```text
//! +0x00  1 byte    base stat (untouched)
//! +0x01  1 byte    fallback navi chip id — the deck's navi when no
//!                  navi chip is bound at deck-array position 0
//! +0x02  12 bytes  the deck array: one byte per position, the index of
//!                  the folder entry equipped there (0xff = empty).
//!                  Position 0 is the navi socket: a *navi* chip entry
//!                  there IS the deck's navi (a program chip written
//!                  there gets blanked on load)
//! +0x0e  u16      MB-capacity bonus from the story's memory upgrades;
//!                  the deck's capacity = the navi's MB stat + this
//! +0x10  u32       the slot-in MB budget (the screen's "SLOT MAX") —
//!                  what the R/L chips draw from
//! +0x14  30 × 4    the chip folder: {chip id, deck position or 0xff, 0, 0}
//! ```
//!
//! The byte immediately after the three blocks ([`EQUIPPED_DECK_OFFSET`])
//! is which of them is equipped — the deck the PRG DECK screen edits and
//! the game fields. Probe-verified: forcing it to 0/1/2 on one save and
//! booting showed that save's three decks in turn (a full NormNavX board
//! at 360/370MB, a one-chip MegaMan board at 40/330MB, an empty one at
//! 0/330MB).
//!
//! All of this is probe-verified against the running game:
//!
//! * An organic (played) save carried BassGS as its deck navi with the
//!   BassGS chip bound at position 0 while the fallback byte said Bass —
//!   and the screen showed BassGS, so position 0 wins. Binding a minted
//!   GutsMan chip at position 0 of the fresh template and booting showed
//!   GutsMan. With position 0 empty, rewriting the fallback byte alone
//!   (MegaMan → Roll) changed the shown navi, so the byte is the
//!   fallback — replacement writes both.
//! * The screen's `used/capacity` MB pair: used is the sum of wired
//!   positions 1–9 only (neither the navi nor R/L count), capacity is
//!   the navi's MB stat plus the +0x0e bonus (BassGS 170 + 160 = the
//!   shown 330; fresh MegaMan 170 + 0 = 170).
//! * +0x10 held 20 on the fresh save and 80 on the organic one, exactly
//!   the "SLOT MAX ... MB" the screen shows next to the two slot-in
//!   chip icons. (An earlier note called this "cached total MB": the
//!   fresh save's 20MB deck made the two coincide.)
//!
//! The navi chips are ids [`NAVI_CHIP_IDS`] — the game's 49-navi roster
//! at the top of the 249-id chip table. A navi chip's stats record *is*
//! the navi: HP stat = battle HP, MB stat = base deck capacity.
//!
//! This view exposes the navi as one extra slot, [`NAVI_SLOT`], past the
//! [`DECK_SLOTS`] board slots — settable but not clearable (a deck
//! always has its navi), and deliberately outside `folder_size()` so
//! generic folder consumers never mistake it for a board slot.
//!
//! The board's eleven slots are deck-array positions 1..=11, so a
//! folder entry's back-reference byte is one more than the slot index
//! this view exposes; the navi sits at position 0.
//!
//! So the folder is the Navi's chip inventory and the deck is which
//! twelve of those are equipped — the two sides reference each other,
//! and every edit here keeps both in step.
//!
//! Nothing in the payload names a region: the US and JP games write the
//! same structure, and each loads the other's saves (verified by booting
//! a US-written save on the JP ROM, which lists the same folder and
//! reaches the same link battle). So there is no region to check and a
//! BCC save is accepted by either game.

use byteorder::ByteOrder as _;
use tango_gamesupport_common::dataview::save as dv_save;

/// One save file's payload, as the game keeps it in EWRAM.
pub const SAVE_SIZE: usize = 0x1734;
/// SRAM block stride; header + payload must fit.
const BLOCK_SIZE: usize = 0x2000;
const SRAM_SIZE: usize = 0x8000;
/// Block header magic: "INTI", the developer's mark.
const MAGIC: u32 = 0x49544e49;

/// First deck block, and the stride to the next.
const DECKS_OFFSET: usize = 0x36c;
const DECK_STRIDE: usize = 0x90;
/// How many Navi decks a save carries.
pub const NUM_DECKS: usize = 3;
/// Which deck is equipped, in the byte just past the last deck block.
pub const EQUIPPED_DECK_OFFSET: usize = DECKS_OFFSET + NUM_DECKS * DECK_STRIDE;
/// Program deck slots per Navi — the deck array's usable positions.
pub const DECK_SLOTS: usize = 11;
/// The extra slot this view addresses the deck's navi chip at (see the
/// module docs): one past the board slots, outside `folder_size()`.
pub const NAVI_SLOT: usize = DECK_SLOTS;
/// The chip ids that are navi chips — the roster the navi socket may hold.
pub const NAVI_CHIP_IDS: std::ops::Range<usize> = 200..249;
/// The equippable program chips. Id 0 is the "NO DATA" sentinel, and
/// above the range sit the game's placeholder entries (DataChp1 through
/// Deleted, 191..200) and then the navi roster ([`NAVI_CHIP_IDS`]).
pub const PROGRAM_CHIP_IDS: std::ops::Range<usize> = 1..191;
/// Chip folder entries per Navi.
pub const FOLDER_ENTRIES: usize = 30;
/// Positions in the deck array, the navi's position 0 included.
const DECK_ARRAY_POSITIONS: usize = 12;
/// The deck's fallback navi chip id byte within its block.
const NAVI_OFFSET: usize = 0x01;
/// The MB-capacity bonus (u16) within a deck block.
const MB_BONUS_OFFSET: usize = 0x0e;
/// The slot-in MB budget (u32) within a deck block.
const SLOT_IN_OFFSET: usize = 0x10;

/// The deck array: position 0 is the navi socket, the board slots
/// follow — slot `s` lives at `DECK_ARRAY_OFFSET + FIRST_POSITION + s`.
const DECK_ARRAY_OFFSET: usize = 0x02;
const FIRST_POSITION: usize = 1;
const FOLDER_OFFSET: usize = 0x14;
const FOLDER_ENTRY_SIZE: usize = 4;
/// An empty deck slot, and an unequipped folder entry's slot byte.
const NONE: u8 = 0xff;

#[derive(Clone)]
pub struct Save {
    buf: [u8; SAVE_SIZE],
}

/// Extract the payload of SRAM block `index` if its header validates.
fn parse_block(sram: &[u8], index: usize, file: u8) -> Option<[u8; SAVE_SIZE]> {
    let block = sram.get(index * BLOCK_SIZE..)?.get(..BLOCK_SIZE)?;
    if byteorder::LittleEndian::read_u32(&block[0..4]) != MAGIC {
        return None;
    }
    if byteorder::LittleEndian::read_u32(&block[8..12]) as usize != SAVE_SIZE {
        return None;
    }
    if block[0xc] != 0 || block[0xe] != file || block[0xf] != 1 {
        return None;
    }
    let payload: [u8; SAVE_SIZE] = block[0x10..][..SAVE_SIZE].try_into().unwrap();
    let sum = payload.iter().map(|&b| b as u32).sum::<u32>();
    if byteorder::LittleEndian::read_u32(&block[4..8]) != sum {
        return None;
    }
    Some(payload)
}

impl Save {
    pub fn new(sram: &[u8]) -> Result<Self, dv_save::Error> {
        if sram.len() < SRAM_SIZE {
            return Err(dv_save::Error::InvalidSize(sram.len()));
        }
        // File 0 first, then its backup copy, then file 1's — the first
        // block whose header and byte-sum check out wins, which is the
        // game's own retry ladder.
        let payload = (0..2u8)
            .flat_map(|file| (0..2usize).map(move |copy| (file, copy)))
            .find_map(|(file, copy)| parse_block(sram, file as usize + copy * 2, file));
        let Some(buf) = payload else {
            // No block passed: not a BCC save (or every copy is
            // corrupt). Report it as a checksum mismatch against the
            // first block's stored sum, which is the useful diagnostic.
            let stored = sram
                .get(4..8)
                .map(byteorder::LittleEndian::read_u32)
                .unwrap_or_default();
            return Err(dv_save::Error::ChecksumMismatch {
                actual: stored,
                expected: vec![],
                shift: 0,
            });
        };

        Ok(Self { buf })
    }

    pub fn from_wram(buf: &[u8]) -> Result<Self, dv_save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(dv_save::Error::InvalidSize(buf.len()))?,
        })
    }

    /// The deck's MB-capacity bonus from the story's memory upgrades:
    /// the deck's capacity = the navi chip's MB stat + this.
    pub fn mb_capacity_bonus(&self, deck: usize) -> u16 {
        byteorder::LittleEndian::read_u16(&self.buf[deck_base(deck) + MB_BONUS_OFFSET..][..2])
    }

    /// The deck's slot-in MB budget (the screen's "SLOT MAX") — what
    /// the R/L chips draw from.
    pub fn slot_in_max(&self, deck: usize) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[deck_base(deck) + SLOT_IN_OFFSET..][..4])
    }
}

/// Byte offset of deck `deck`'s block.
fn deck_base(deck: usize) -> usize {
    DECKS_OFFSET + deck * DECK_STRIDE
}

/// Byte offset of deck `deck`'s folder entry `entry`.
fn entry_base(deck: usize, entry: usize) -> usize {
    deck_base(deck) + FOLDER_OFFSET + entry * FOLDER_ENTRY_SIZE
}

/// The deck-array position a folder entry records for `slot`: the
/// board slots are positions 1..=11, the navi socket is position 0.
fn slot_position(slot: usize) -> u8 {
    if slot == NAVI_SLOT {
        0
    } else {
        (FIRST_POSITION + slot) as u8
    }
}

/// Byte offset of the deck-array position holding slot `slot`.
fn slot_byte(deck: usize, slot: usize) -> usize {
    deck_base(deck) + DECK_ARRAY_OFFSET + slot_position(slot) as usize
}

impl dv_save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn dv_save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn dv_save::ChipsViewMut + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navi(&self) -> Option<Box<dyn dv_save::NaviView + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut sram = vec![0xff; SRAM_SIZE];
        let sum = self.buf.iter().map(|&b| b as u32).sum::<u32>();
        // Written as file 0, both copies; file 1 is left erased, which
        // the game reads as an empty second GP data slot.
        for copy in 0..2usize {
            let block = &mut sram[copy * 2 * BLOCK_SIZE..][..BLOCK_SIZE];
            byteorder::LittleEndian::write_u32(&mut block[0..4], MAGIC);
            byteorder::LittleEndian::write_u32(&mut block[4..8], sum);
            byteorder::LittleEndian::write_u32(&mut block[8..12], SAVE_SIZE as u32);
            block[0xc..0x10].copy_from_slice(&[0, 2, 0, 1]);
            block[0x10..][..SAVE_SIZE].copy_from_slice(&self.buf);
        }
        sram
    }

    fn rebuild_checksum(&mut self) {
        // The checksum covers the payload but lives in the SRAM block
        // header, so it is computed on the way out by `to_sram_dump`.
    }
}

/// The deck's navi, for the save strip beside Play. BCC has no navi
/// roster in the ROM sense (no emblems, no navi table) — the navi *is*
/// a chip — so this reports the equipped navi chip and its HP, which
/// is the deck's battle HP, and nothing else. Read-only on purpose:
/// `view_navi_mut` staying `None` keeps the shared change-navi picker
/// (which needs emblems this game doesn't have) out of the way, while
/// the deck board does the swapping through [`NAVI_SLOT`].
pub struct NaviView<'a> {
    save: &'a Save,
}

impl NaviView<'_> {
    /// The navi chip bound in the socket, falling back to the byte —
    /// the same resolution [`ChipsView::chip`] does for [`NAVI_SLOT`].
    fn navi_chip(&self) -> Option<usize> {
        dv_save::ChipsView::chip(
            &ChipsView { save: self.save },
            dv_save::ChipsView::equipped_folder_index(&ChipsView { save: self.save }),
            NAVI_SLOT,
        )
        .map(|c| c.id)
    }
}

impl dv_save::NaviView for NaviView<'_> {
    fn navi(&self) -> usize {
        self.navi_chip().unwrap_or_default()
    }

    fn max_hp(&self, assets: &dyn tango_gamesupport_common::dataview::rom::Assets) -> u16 {
        // The navi chip's own HP stat — BCC's chip model, so this goes
        // through the game's concrete assets rather than the shared
        // chip trait (which has no HP).
        let Some(id) = self.navi_chip() else { return 0 };
        assets
            .underlying_any()
            .downcast_ref::<crate::rom::Assets>()
            .and_then(|a| a.chip_info(id))
            .map(|c| c.hp())
            .unwrap_or_default()
    }

    fn folder_limits(
        &self,
        _assets: &dyn tango_gamesupport_common::dataview::rom::Assets,
    ) -> dv_save::FolderLimits {
        // BCC budgets a deck in MB, not in chip classes; the deck board
        // enforces that itself.
        dv_save::FolderLimits::default()
    }
}

pub struct ChipsView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> ChipsView<S> {
    /// The folder entry equipped in `slot`, if any.
    fn slot_entry(&self, deck: usize, slot: usize) -> Option<usize> {
        let v = *self.save.buf.get(slot_byte(deck, slot))?;
        (v != NONE && (v as usize) < FOLDER_ENTRIES).then_some(v as usize)
    }

    /// Folder entry `entry`'s chip id, or `None` for an empty entry.
    fn entry_chip(&self, deck: usize, entry: usize) -> Option<usize> {
        let id = *self.save.buf.get(entry_base(deck, entry))?;
        (id != 0).then_some(id as usize)
    }

    /// A folder entry no deck slot points at — free to restock with
    /// whatever the deck is being asked to hold.
    ///
    /// One always exists: the board plus the navi socket are twelve
    /// positions against [`FOLDER_ENTRIES`] entries, so at most twelve
    /// of the thirty can be spoken for. That is what lets `set_chip`
    /// equip any chip into any slot.
    fn restockable_entry(&self, deck: usize) -> Option<usize> {
        let equipped: Vec<usize> = (0..=NAVI_SLOT).filter_map(|slot| self.slot_entry(deck, slot)).collect();
        // Empty entries first, so restocking spends a held chip only
        // once the folder has genuinely run out of room.
        (0..FOLDER_ENTRIES)
            .find(|&e| !equipped.contains(&e) && self.entry_chip(deck, e).is_none())
            .or_else(|| (0..FOLDER_ENTRIES).find(|e| !equipped.contains(e)))
    }
}

impl<S: std::ops::Deref<Target = Save>> dv_save::ChipsView for ChipsView<S> {
    fn num_folders(&self) -> usize {
        NUM_DECKS
    }

    fn folder_size(&self) -> usize {
        DECK_SLOTS
    }

    /// The deck the game currently fields (see [`EQUIPPED_DECK_OFFSET`]),
    /// so the editor shows what the PRG DECK screen shows rather than
    /// always deck 0. A junk index reads as deck 0 — the save is the
    /// game's, but nothing here may index past the three blocks.
    fn equipped_folder_index(&self) -> usize {
        let deck = self.save.buf[EQUIPPED_DECK_OFFSET] as usize;
        if deck < NUM_DECKS {
            deck
        } else {
            0
        }
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<dv_save::Chip> {
        if folder_index >= NUM_DECKS || chip_index > NAVI_SLOT {
            return None;
        }
        let id = if chip_index == NAVI_SLOT {
            // The navi chip bound at position 0 wins; the fallback byte
            // covers a deck with an empty socket (see the module docs).
            let bound = self
                .slot_entry(folder_index, NAVI_SLOT)
                .and_then(|e| self.entry_chip(folder_index, e))
                .filter(|id| NAVI_CHIP_IDS.contains(id));
            match bound {
                Some(id) => id,
                None => {
                    let id = *self.save.buf.get(deck_base(folder_index) + NAVI_OFFSET)? as usize;
                    NAVI_CHIP_IDS.contains(&id).then_some(id)?
                }
            }
        } else {
            let entry = self.slot_entry(folder_index, chip_index)?;
            self.entry_chip(folder_index, entry)?
        };
        Some(dv_save::Chip {
            id,
            // BCC chips have no code letters; every chip is one variant.
            code: dv_save::ChipCode::Star,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if variant != 0 || id == 0 || id > u8::MAX as usize {
            return None;
        }
        // The equipped Navi's folder is its chip inventory: how many
        // copies of this chip it holds, equipped or not.
        let deck = dv_save::ChipsView::equipped_folder_index(self);
        Some(
            (0..FOLDER_ENTRIES)
                .filter(|&e| self.entry_chip(deck, e) == Some(id))
                .count(),
        )
    }
}

impl<S: std::ops::DerefMut<Target = Save>> ChipsView<S> {
    /// Point deck slot `slot` at `entry` (or empty it), keeping the
    /// folder entry's own back-reference in step — the game reads both
    /// directions, so they must agree.
    fn bind(&mut self, deck: usize, slot: usize, entry: Option<usize>) {
        let old = {
            let v = self.save.buf[slot_byte(deck, slot)];
            (v != NONE && (v as usize) < FOLDER_ENTRIES).then_some(v as usize)
        };
        if let Some(old) = old {
            self.save.buf[entry_base(deck, old) + 1] = NONE;
        }
        self.save.buf[slot_byte(deck, slot)] = match entry {
            Some(e) => {
                self.save.buf[entry_base(deck, e) + 1] = slot_position(slot);
                e as u8
            }
            None => NONE,
        };
    }

    /// A folder entry holding `id` that this deck can equip into `slot`:
    /// an unclaimed copy first, otherwise one equipped in a *later* slot,
    /// which equipping here moves.
    ///
    /// The fallback is what makes a reorder work. The editor rewrites the
    /// deck slot by slot, so shuffling two chips asks for a chip that is
    /// still equipped further down; without stealing that copy each
    /// rewrite would mint a duplicate in the folder. Only later slots are
    /// candidates — an earlier slot has already been rewritten to its
    /// final contents, and stealing from it would undo that.
    fn claimable_entry(&self, deck: usize, slot: usize, id: usize) -> Option<usize> {
        let holds_id = |e: usize| self.save.buf[entry_base(deck, e)] as usize == id;
        let position = |e: usize| self.save.buf[entry_base(deck, e) + 1];
        (0..FOLDER_ENTRIES)
            .find(|&e| holds_id(e) && position(e) == NONE)
            .or_else(|| (0..FOLDER_ENTRIES).find(|&e| holds_id(e) && position(e) > slot_position(slot)))
    }
}

impl<S: std::ops::DerefMut<Target = Save>> dv_save::ChipsViewMut for ChipsView<S> {
    fn set_chip(&mut self, folder_index: usize, chip_index: usize, chip: dv_save::Chip) -> bool {
        if folder_index >= NUM_DECKS || chip_index > NAVI_SLOT || chip.id == 0 || chip.id > u8::MAX as usize {
            return false;
        }
        // Only an actual navi chip may sit in the navi socket.
        if chip_index == NAVI_SLOT && !NAVI_CHIP_IDS.contains(&chip.id) {
            return false;
        }
        // Equipping claims one of this Navi's own copies — in game the
        // deck can only hold chips the folder has. If every copy is
        // spoken for, the chip is being *added* rather than moved, so
        // the folder is restocked with it: the editor manages the
        // inventory to match the deck instead of refusing the edit. An
        // entry the deck doesn't point at is always available (see
        // `restockable_entry`), so equipping never silently no-ops —
        // the folder is invisible in the editor, and a click that did
        // nothing read as a broken button.
        let entry = match self.claimable_entry(folder_index, chip_index, chip.id) {
            Some(e) => e,
            None => {
                let Some(e) = self.restockable_entry(folder_index) else {
                    return false;
                };
                self.save.buf[entry_base(folder_index, e)] = chip.id as u8;
                e
            }
        };
        self.bind(folder_index, chip_index, Some(entry));
        if chip_index == NAVI_SLOT {
            // Keep the fallback byte in step: it's what the game (and
            // this view) reads when the socket is empty, and the game's
            // own saves keep the two agreeing.
            self.save.buf[deck_base(folder_index) + NAVI_OFFSET] = chip.id as u8;
        }
        true
    }

    fn clear_chip(&mut self, folder_index: usize, chip_index: usize) -> bool {
        if folder_index >= NUM_DECKS || chip_index >= DECK_SLOTS {
            return false;
        }
        self.bind(folder_index, chip_index, None);
        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        if variant != 0 || id == 0 || id > u8::MAX as usize {
            return false;
        }
        let deck = dv_save::ChipsView::equipped_folder_index(self);
        let mut have: Vec<usize> = (0..FOLDER_ENTRIES)
            .filter(|&e| self.save.buf[entry_base(deck, e)] as usize == id)
            .collect();
        // Drop the surplus from the back, unequipping as we go so no
        // deck slot is left pointing at a cleared entry.
        while have.len() > count {
            let e = have.pop().unwrap();
            let base = entry_base(deck, e);
            let position = self.save.buf[base + 1];
            if position != NONE && (position as usize) < DECK_ARRAY_POSITIONS {
                self.save.buf[deck_base(deck) + DECK_ARRAY_OFFSET + position as usize] = NONE;
            }
            self.save.buf[base..base + FOLDER_ENTRY_SIZE].copy_from_slice(&[0, NONE, 0, 0]);
        }
        while have.len() < count {
            let Some(e) = (0..FOLDER_ENTRIES).find(|&e| self.save.buf[entry_base(deck, e)] == 0) else {
                return false;
            };
            self.save.buf[entry_base(deck, e)..][..FOLDER_ENTRY_SIZE].copy_from_slice(&[id as u8, NONE, 0, 0]);
            have.push(e);
        }
        true
    }

    fn rebuild_anticheat(&mut self) {
        // BCC has no anti-cheat shadow copy.
    }
}
