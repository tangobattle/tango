//! The Rockman.EXE Operate Shooting Star save.
//!
//! The cart carries **one** in-game file, unlike BN5DS's two — the file
//! select offers a single row and the flash holds it twice, as a pair of
//! alternating banks. So a dump parses to one [`Save`], and there is no
//! session payload steering a file choice the way BN5DS's is.
//!
//! Recognition is settled: the game stamps a format tag at the very
//! head of the flash and a `BANK` header on each of its two banks, and
//! finding both intact is what puts a dump in the save picker. Past
//! that the bank's interior is its own layout rather than BN1's — no
//! GBA-era string or offset survives in it — and what this maps of it
//! is the chip folder.

use tango_gamesupport_common::dataview::save::{self, Error};

/// The flash the cart carries: 256 KiB, the same chip BN5DS ships on.
/// That is what the emulator mounts and hands back, so it is the length
/// a dump is canonicalized to — but not the length one has to arrive
/// at, see [`Save::new`].
pub const SIZE: usize = 0x4_0000;

/// What a byte of flash reads as before anything is written to it, and
/// so what a dump that stops short of [`SIZE`] is padded out with.
const ERASED: u8 = 0xff;

/// The format tag at the head of the flash. `exe1ds` is the remake
/// naming itself after the game it remakes — the cart's own label for
/// its save format, not something derived.
pub const MAGIC: &[u8; 12] = b"exe1ds_v1001";

/// Where the two banks' headers sit. The game alternates between them
/// on every save, so the one with the higher generation counter holds
/// the current file — the same double-buffering BN5DS does with its
/// mirrored block pairs.
pub const BANK_OFFSETS: [usize; 2] = [0x80, 0x3300];

/// The tag at the head of each bank.
pub const BANK_MAGIC: &[u8; 4] = b"BANK";

/// A bank header is this long; the payload follows it directly.
pub const BANK_HEADER_SIZE: usize = 0x20;

/// Which of the two banks this is (u32 LE), at the header's `+4`. Reads
/// 0 and 1 in [`BANK_OFFSETS`] order on an intact dump.
const BANK_INDEX_OFFSET: usize = 0x04;

/// The bank's save-generation counter (u32 LE), bumped on every save.
/// The higher of the two is the live file.
const BANK_GENERATION_OFFSET: usize = 0x08;

/// The payload's length (u32 LE), which the header carries twice — at
/// `+0x14` and again at `+0x18`. Both read 0x2860 on this cart; the
/// second is checked against the first as the one cheap consistency
/// test the header offers.
const BANK_SIZE_OFFSET: usize = 0x14;
const BANK_SIZE_MIRROR_OFFSET: usize = 0x18;

/// The two words at `+0x0c` and `+0x10` are the bank's own integrity
/// stamps, and they are **not mapped**: neither a byte sum, a word sum
/// nor a CRC32 over the payload reproduces either one. Nothing here
/// needs them — a save is handed to the console as the bytes it was
/// read as — but an editor would, so [`Save::rebuild_checksum`] is a
/// no-op rather than a guess, and the save hands out no writable views.
const _INTEGRITY_WORDS: std::ops::Range<usize> = 0x0c..0x14;

/// One bank of the flash, as the parse read it.
#[derive(Clone, Copy)]
struct Bank {
    /// Where the bank's header starts in the dump.
    at: usize,
    /// The generation counter that decides which bank is live.
    generation: u32,
    /// How long the payload behind the header is.
    size: usize,
}

impl Bank {
    /// Read the bank at `at`, or `None` if it isn't one: no tag, a
    /// length the dump can't hold, or the two length fields disagreeing.
    fn parse(data: &[u8], at: usize, index: u32) -> Option<Bank> {
        let header = data.get(at..at + BANK_HEADER_SIZE)?;
        if &header[..BANK_MAGIC.len()] != BANK_MAGIC {
            return None;
        }
        let word = |off: usize| u32::from_le_bytes(header[off..off + 4].try_into().unwrap());
        if word(BANK_INDEX_OFFSET) != index {
            return None;
        }
        let size = word(BANK_SIZE_OFFSET);
        if size != word(BANK_SIZE_MIRROR_OFFSET) {
            return None;
        }
        let size = size as usize;
        data.get(at + BANK_HEADER_SIZE..at + BANK_HEADER_SIZE + size)?;
        Some(Bank {
            at,
            generation: word(BANK_GENERATION_OFFSET),
            size,
        })
    }

    /// The bank's payload — the save proper.
    fn payload<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        &data[self.at + BANK_HEADER_SIZE..][..self.size]
    }
}

/// A parsed cartridge dump: the whole flash, plus which of its two banks
/// is the live one.
#[derive(Clone)]
pub struct Save {
    buf: Vec<u8>,
    live: Bank,
}

impl Save {
    /// Parse a cartridge dump. Accepts a dump carrying the format tag
    /// and at least one intact bank — a cart saved exactly once has its
    /// second bank still erased, and that is a real save, not a damaged
    /// one.
    ///
    /// A dump shorter than [`SIZE`] is one of those too. The game never
    /// writes past the second bank's payload (0x5b80 of a 256 KiB
    /// chip), so everything downstream of that is flash the cart has
    /// only ever read as erased — and a dumper that sizes the chip by
    /// what the game touched, or one that trims the erased tail, hands
    /// over a short file with every byte that matters in it. Such a
    /// dump is padded back out rather than rejected, which is also what
    /// the emulator does with it: the cart's flash length comes from
    /// its ROM entry, and a short image is copied into the front of an
    /// erased chip. Padding here just means the save Tango reads, the
    /// SRAM it hands the console and the image the replay records are
    /// the same bytes whatever length the file arrived at.
    pub fn new(buf: &[u8]) -> Result<Self, Error> {
        if buf.len() > SIZE {
            return Err(Error::InvalidSize(buf.len()));
        }
        let mut buf = buf.to_vec();
        buf.resize(SIZE, ERASED);
        if &buf[..MAGIC.len()] != MAGIC {
            return Err(Error::InvalidGameName(buf[..MAGIC.len()].to_vec()));
        }
        let live = BANK_OFFSETS
            .iter()
            .enumerate()
            .filter_map(|(index, &at)| Bank::parse(&buf, at, index as u32))
            // The game mounts the higher generation; ties can't happen
            // on a cart that has ever been saved, and picking either is
            // right if one ever did.
            .max_by_key(|bank| bank.generation)
            .ok_or_else(|| Error::InvalidGameName(BANK_MAGIC.to_vec()))?;
        Ok(Save { buf, live })
    }

    /// The live bank's generation counter — how many times this cart has
    /// been saved, as the game counts it.
    pub fn generation(&self) -> u32 {
        self.live.generation
    }

    /// The live bank's payload — what every offset below is relative to.
    fn payload(&self) -> &[u8] {
        self.live.payload(&self.buf)
    }
}

impl save::Save for Save {
    /// The chip folder, read-only. There is no `view_chips_mut` to go
    /// with it: writing the folder back would have to restamp the
    /// bank's integrity words, and those are unmapped (see
    /// [`_INTEGRITY_WORDS`]).
    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        self.buf.clone()
    }

    /// The live bank's payload: the save proper, behind its header.
    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(self.payload())
    }

    /// A no-op, deliberately: the bank's integrity words are unmapped
    /// (see [`_INTEGRITY_WORDS`]), so there is nothing honest to
    /// recompute. Nothing calls it either — the save hands out no
    /// writable views, and netplay passes the dump through byte for
    /// byte.
    fn rebuild_checksum(&mut self) {}
}

/// Where the payload keeps the equipped folder: thirty `(id, code)`
/// pairs, the shape BN1 uses — but at this save's own offset, not
/// BN1's. Found by scanning a played cart for thirty consecutive pairs
/// whose code is one the ROM's chip table actually lists for that chip;
/// exactly one run in the payload qualifies, and it reads back as a
/// coherent folder (three FightrSwrd B, five Paladin B, Recov120 `*`).
const FOLDER_OFFSET: usize = 0x017c;

/// The remake's wildcard code. The chip table numbers it 27, where the
/// later GBA games put theirs at 26 — which is where Tango's
/// [`save::ChipCode::Star`] sits, so the two disagree by one and this
/// is where that is reconciled.
const RAW_STAR_CODE: u8 = 27;

pub struct ChipsView<'a> {
    save: &'a Save,
}

impl save::ChipsView for ChipsView<'_> {
    /// One, and it is the one the game battles with — this cart has no
    /// folder switching.
    fn num_folders(&self) -> usize {
        1
    }

    fn equipped_folder_index(&self) -> usize {
        0
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= self.folder_size() {
            return None;
        }
        let raw = self.save.payload().get(FOLDER_OFFSET + chip_index * 2..)?.get(..2)?;
        let code = match raw[1] {
            RAW_STAR_CODE => save::ChipCode::Star,
            // An empty slot reads back as a code no chip has, which is
            // what `from_u8` rejecting it turns into `None` here.
            other => num_traits::FromPrimitive::from_u8(other)?,
        };
        Some(save::Chip {
            id: raw[0] as usize,
            code,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dump the game has written: the flash tag, then both banks.
    fn cart(generations: [u32; 2]) -> Vec<u8> {
        let mut buf = vec![0xff; SIZE];
        buf[..MAGIC.len()].copy_from_slice(MAGIC);
        for (index, &at) in BANK_OFFSETS.iter().enumerate() {
            buf[at..at + 4].copy_from_slice(BANK_MAGIC);
            buf[at + BANK_INDEX_OFFSET..][..4].copy_from_slice(&(index as u32).to_le_bytes());
            buf[at + BANK_GENERATION_OFFSET..][..4].copy_from_slice(&generations[index].to_le_bytes());
            for off in [BANK_SIZE_OFFSET, BANK_SIZE_MIRROR_OFFSET] {
                buf[at + off..][..4].copy_from_slice(&0x2860u32.to_le_bytes());
            }
            // Something to tell the two banks' payloads apart.
            buf[at + BANK_HEADER_SIZE] = index as u8;
        }
        buf
    }

    #[test]
    fn the_newer_bank_is_the_live_one() {
        use tango_gamesupport_common::dataview::save::Save as _;
        for (generations, expected) in [([0xeb, 0xec], 1u8), ([0xec, 0xeb], 0)] {
            let save = Save::new(&cart(generations)).unwrap();
            assert_eq!(save.as_raw_wram()[0], expected, "generations {generations:?}");
        }
    }

    #[test]
    fn a_cart_saved_once_still_parses() {
        let mut buf = cart([1, 0]);
        // The bank the game hasn't reached yet is erased flash.
        buf[BANK_OFFSETS[1]..BANK_OFFSETS[1] + BANK_HEADER_SIZE].fill(0xff);
        assert!(Save::new(&buf).is_ok());
    }

    /// A dump that stops after the last byte the game ever writes is
    /// the whole save, and reads back as one padded out to the chip.
    #[test]
    fn a_dump_short_of_the_flash_still_parses() {
        use tango_gamesupport_common::dataview::save::Save as _;
        let full = cart([1, 2]);
        for len in [0x1_0000, 0x2_0000] {
            let save = Save::new(&full[..len]).unwrap();
            assert_eq!(save.generation(), 2, "{len:#x}");
            assert_eq!(save.to_sram_dump(), full, "{len:#x}");
        }
    }

    #[test]
    fn other_games_saves_are_rejected() {
        // The tag is what identifies the cart; a dump of the right size
        // without it is some other game's.
        assert!(Save::new(&vec![0; SIZE]).is_err());
        // BN5DS's dump is the same size and carries its own tag.
        let mut bn5ds = vec![0xff; SIZE];
        bn5ds[0x9f08..][..16].copy_from_slice(b"EXE DS SAVE 0006");
        assert!(Save::new(&bn5ds).is_err());
        // The tag alone isn't enough — a dump with no bank is damaged.
        let mut headless = vec![0xff; SIZE];
        headless[..MAGIC.len()].copy_from_slice(MAGIC);
        assert!(Save::new(&headless).is_err());
        // A short dump is padded up to the chip; one longer than the
        // chip is not a dump of it.
        let mut oversize = cart([1, 2]);
        oversize.resize(SIZE + 1, 0xff);
        assert!(Save::new(&oversize).is_err());
    }
}
