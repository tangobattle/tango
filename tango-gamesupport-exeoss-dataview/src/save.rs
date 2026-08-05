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
//! is the chip folder and the pack behind it, which is enough to edit
//! the folder.
//!
//! What makes editing possible is that the bank's integrity stamps are
//! mapped now: both are plain CRC32s (the game's own routine is at ARM9
//! 0x20017c0, reached through the hash object it keeps at 0x02040558),
//! one over the header and one over the payload, and the game checks
//! both — a bank failing either is one it refuses and falls back past,
//! exactly as [`Save::new`] does.

use tango_gamesupport_common_dataview::save::{self, Error};

/// The save memory the cart carries: 64 KiB of EEPROM. Smaller than
/// BN5DS's 256 KiB flash, which this used to claim to match — the game
/// writes nothing past 0x5be0 either way, so the difference never
/// showed up in what was read, only in what got written back.
///
/// It is what the emulator mounts and hands back (`B6XJ` is save type 3
/// in melonDS's ROM list, and a booted cart reports 0x10000), so it is
/// the length a dump is canonicalized to — but not the length one has
/// to arrive at, see [`Save::new`].
pub const SIZE: usize = 0x1_0000;

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
///
/// Both follow from the flash header, which spells the geometry out
/// past its tag: two banks (`+0x1c`), 0x3280 apart (`+0x20`), the first
/// one [`BANK_HEADER_SIZE`] in (`+0x24`, which is the flash header's own
/// length too).
pub const BANK_OFFSETS: [usize; 2] = [0x80, 0x3300];

/// The tag at the head of each bank.
pub const BANK_MAGIC: &[u8; 4] = b"BANK";

/// A bank header is this long; the payload follows it directly. Only
/// the first [`CHECKSUMMED_HEADER_SIZE`] bytes of it are fields — the
/// rest reads as zero on a written cart, and the game neither writes
/// nor checksums it.
pub const BANK_HEADER_SIZE: usize = 0x80;

/// Which of the two banks this is (u32 LE), at the header's `+4`. Reads
/// 0 and 1 in [`BANK_OFFSETS`] order on an intact dump.
const BANK_INDEX_OFFSET: usize = 0x04;

/// The bank's save-generation counter (u32 LE), bumped on every save.
/// The higher of the two is the live file.
const BANK_GENERATION_OFFSET: usize = 0x08;

/// The header's own CRC32 (u32 LE), over the [`CHECKSUMMED_HEADER_SIZE`]
/// bytes of fields with this word and [`PAYLOAD_CHECKSUM_OFFSET`]'s
/// zeroed. The game recomputes and compares it as it mounts the flash
/// (ARM9 0x2001778, from the bank walk at 0x2001544), and a bank that
/// fails is struck off the list before anything reads its payload.
const HEADER_CHECKSUM_OFFSET: usize = 0x0c;

/// The payload's CRC32 (u32 LE), over [`BANK_SIZE_OFFSET`] bytes from
/// the payload's first. The game recomputes it against the bytes it
/// just read (ARM9 0x2001d7c) and, on a mismatch, drops that bank and
/// tries the next-newest — which is what [`Save::new`] does with it.
const PAYLOAD_CHECKSUM_OFFSET: usize = 0x10;

/// The payload's length (u32 LE), which the header carries twice — at
/// `+0x14` and again at `+0x18`. Both read 0x2860 on this cart; the
/// second is what the bank has room for rather than a mirror (the game
/// writes `max(reserved, size)` there), so it is checked against the
/// first as the one cheap consistency test the header offers.
const BANK_SIZE_OFFSET: usize = 0x14;
const BANK_SIZE_MIRROR_OFFSET: usize = 0x18;

/// How much of the header the header's own checksum covers: the fields,
/// up to and including the two size words.
const CHECKSUMMED_HEADER_SIZE: usize = 0x1c;

/// The game's integrity stamp, both times it uses one: a stock CRC32.
fn checksum(buf: &[u8]) -> u32 {
    crc32fast::hash(buf)
}

/// [`HEADER_CHECKSUM_OFFSET`]'s value for the header starting at
/// `header`: its fields with both checksum words zeroed, which is the
/// state the game hashes them in.
fn header_checksum(header: &[u8]) -> u32 {
    let mut fields = [0u8; CHECKSUMMED_HEADER_SIZE];
    fields.copy_from_slice(&header[..CHECKSUMMED_HEADER_SIZE]);
    fields[HEADER_CHECKSUM_OFFSET..PAYLOAD_CHECKSUM_OFFSET + std::mem::size_of::<u32>()].fill(0);
    checksum(&fields)
}

/// One bank of the flash, as the parse read it.
#[derive(Clone, Copy)]
struct Bank {
    /// Where the bank's header starts in the dump.
    at: usize,
    /// The generation counter that decides which bank is live.
    generation: u32,
    /// How long the payload behind the header is.
    size: usize,
    /// The payload checksum the header stores.
    payload_checksum: u32,
}

impl Bank {
    /// Read the bank at `at`, or `None` if it isn't one: no tag, a
    /// payload longer than the room the header says it has or than the
    /// dump can hold, or a header the game's own check would refuse.
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
        if size > word(BANK_SIZE_MIRROR_OFFSET) {
            return None;
        }
        if word(HEADER_CHECKSUM_OFFSET) != header_checksum(header) {
            return None;
        }
        let size = size as usize;
        data.get(at + BANK_HEADER_SIZE..at + BANK_HEADER_SIZE + size)?;
        Some(Bank {
            at,
            generation: word(BANK_GENERATION_OFFSET),
            size,
            payload_checksum: word(PAYLOAD_CHECKSUM_OFFSET),
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
    /// A dump of any length is one of those too, canonicalized to
    /// [`SIZE`] either way.
    ///
    /// Short of the chip it is padded. The game never writes past the
    /// second bank's payload (0x5be0), so everything downstream of that
    /// is flash the cart has only ever read as erased — and a dumper
    /// that sizes the chip by what the game touched, or one that trims
    /// the erased tail, hands over a short file with every byte that
    /// matters in it.
    ///
    /// Past the chip it is cut off, for the reason BN5DS's
    /// `SaveSet::parse` gives at length: a longer file is a container,
    /// DS save managers write a fixed size whatever the cart holds, and
    /// the emulator truncates the same file the same way. Length never
    /// identified a save here — the format tag below does, at any size.
    ///
    /// So the save Tango reads, the SRAM it hands the console and the
    /// image the replay records are the same bytes whatever length the
    /// file arrived at.
    ///
    /// Which bank it opens on is the game's own ladder rather than
    /// simply the newest: newest first, and past any whose payload does
    /// not match the checksum its header carries, since that is a bank
    /// the console will refuse too. Reading a file the game would not
    /// load is the one way this could show a folder that isn't the one
    /// a match would be played with.
    pub fn new(buf: &[u8]) -> Result<Self, Error> {
        let mut buf = buf.to_vec();
        buf.resize(SIZE, ERASED);
        if &buf[..MAGIC.len()] != MAGIC {
            return Err(Error::InvalidGameName(buf[..MAGIC.len()].to_vec()));
        }
        let mut banks = BANK_OFFSETS
            .iter()
            .enumerate()
            .filter_map(|(index, &at)| Bank::parse(&buf, at, index as u32))
            .collect::<Vec<_>>();
        // The game mounts the higher generation; ties can't happen on a
        // cart that has ever been saved, and picking either is right if
        // one ever did.
        banks.sort_by_key(|bank| std::cmp::Reverse(bank.generation));
        let newest = *banks.first().ok_or_else(|| Error::InvalidGameName(BANK_MAGIC.to_vec()))?;
        let live = banks
            .into_iter()
            .find(|bank| checksum(bank.payload(&buf)) == bank.payload_checksum)
            .ok_or_else(|| Error::ChecksumMismatch {
                actual: newest.payload_checksum,
                expected: vec![checksum(newest.payload(&buf))],
                shift: 0,
            })?;
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

    fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.buf[self.live.at + BANK_HEADER_SIZE..][..self.live.size]
    }
}

impl save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn save::ChipsViewMut + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    /// There is no navi to pick on this cart, but there is a navi: the
    /// HP MegaMan brings and the rules his folder is built under. No
    /// writable half — nothing here is the player's to set.
    fn view_navi(&self) -> Option<Box<dyn save::NaviView + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        self.buf.clone()
    }

    /// The live bank's payload: the save proper, behind its header.
    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(self.payload())
    }

    /// Restamp the live bank's two checksums over whatever the edit
    /// left. The header's own is unmoved by a folder edit in practice —
    /// it covers the header's fields with both checksum words zeroed,
    /// and an edit touches none of the rest — but it is recomputed
    /// rather than assumed, so this stays right whatever comes to be
    /// written.
    ///
    /// The flash header's checksum (its own `+0x18`, over its first 0x2c
    /// bytes) is left alone — an edit reaches the payload, never the
    /// geometry those bytes describe.
    fn rebuild_checksum(&mut self) {
        let payload = checksum(self.payload());
        self.live.payload_checksum = payload;
        let at = self.live.at;
        self.buf[at + PAYLOAD_CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()]
            .copy_from_slice(&payload.to_le_bytes());
        let header = header_checksum(&self.buf[at..][..BANK_HEADER_SIZE]);
        self.buf[at + HEADER_CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()].copy_from_slice(&header.to_le_bytes());
    }
}

/// Where the payload keeps the HP MegaMan brings: the u16 the game
/// hands him when a link battle opens. Found by poking it and reading
/// the number the battle opened on — 777 in, 777 over his head. The
/// word beside it at `+0x1e` reads the same 1000 on the played cart, as
/// a cart saved at full health would, and is not this one: poking it to
/// 1500 leaves the battle opening on 1000 all the same.
const MAX_HP_OFFSET: usize = 0x0020;

/// What the cart's own folder editor allows, and so what this one does:
/// five copies of any one chip, five navi chips. The played cart's
/// folder sits at both caps — five Paladin B, and five navis from
/// ProtoMan V2 to Rogue — which is the shape a folder built under them
/// takes.
const MAX_COPIES: usize = 5;
const NAVI_LIMIT: usize = 5;

/// Where the payload keeps the equipped folder: thirty `(id, code)`
/// pairs, the shape BN1 uses — but at this save's own offset, not
/// BN1's. Found by scanning a played cart for thirty consecutive pairs
/// whose code is one the ROM's chip table actually lists for that chip;
/// exactly one run in the payload qualifies, and it reads back as a
/// coherent folder (three FightrSwrd B, five Paladin B, Recov120 `*`).
const FOLDER_OFFSET: usize = 0x011c;

/// How wide a folder slot is: the id byte and the code byte.
const FOLDER_ENTRY_SIZE: usize = 2;

/// The one folder the cart battles with, and the thirty slots in it —
/// what [`save::ChipsView`] answers with, kept as constants so the
/// reading and writing halves bound themselves by the same numbers.
const NUM_FOLDERS: usize = 1;
const FOLDER_SIZE: usize = 30;

/// The chip pack — what the player owns, and so what the folder editor
/// may offer. It starts where the folder ends, and gives every chip id
/// a row of [`PACK_ENTRY_SIZE`]: [`PACK_CODES`] counts, one per code in
/// the order the ROM's own table lists that chip's codes, then a pad
/// byte and five u16s this does not map (they read 0xffff wherever the
/// count beside them is 0, and hold a small per-code number otherwise;
/// nothing here needs them, and nothing here writes them).
///
/// That the counts are in the ROM's code order is what pins the row's
/// shape: across all 240 ids, no count sits in a slot past the end of
/// what the chip table lists for that chip, and every code the played
/// cart's folder equips has a count against it.
const PACK_OFFSET: usize = 0x0158;
const PACK_ENTRY_SIZE: usize = 0x10;
const PACK_CODES: usize = 5;

/// The remake's wildcard code. The chip table numbers it 27, where the
/// later GBA games put theirs at 26 — which is where Tango's
/// [`save::ChipCode::Star`] sits, so the two disagree by one and this
/// is where that is reconciled.
const RAW_STAR_CODE: u8 = 27;

/// What an emptied folder slot is written as, and so what
/// [`save::ChipsView::chip`] reads back as nothing: a code no chip has.
/// A committed folder never carries one — the editor only saves a full
/// thirty — so this is the editor's own in-between state rather than
/// something the cart is known to write.
const EMPTY_SLOT: u8 = 0xff;

pub struct ChipsView<S> {
    save: S,
}

/// Where slot `chip_index` of folder `folder_index` sits in the
/// payload, or `None` if the cart has no such slot.
fn folder_slot(folder_index: usize, chip_index: usize) -> Option<usize> {
    (folder_index < NUM_FOLDERS && chip_index < FOLDER_SIZE)
        .then_some(FOLDER_OFFSET + chip_index * FOLDER_ENTRY_SIZE)
}

impl<S: std::ops::Deref<Target = Save>> save::ChipsView for ChipsView<S> {
    /// One, and it is the one the game battles with — this cart has no
    /// folder switching.
    fn num_folders(&self) -> usize {
        NUM_FOLDERS
    }

    fn folder_size(&self) -> usize {
        FOLDER_SIZE
    }

    fn equipped_folder_index(&self) -> usize {
        0
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<save::Chip> {
        let raw = self
            .save
            .payload()
            .get(folder_slot(folder_index, chip_index)?..)?
            .get(..FOLDER_ENTRY_SIZE)?;
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

    /// How many of this chip in this code the pack holds, folder copies
    /// included — a singleton navi chip the played cart has equipped
    /// counts 1, not 0.
    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= crate::NUM_CHIPS || variant >= PACK_CODES {
            return None;
        }
        self.save
            .payload()
            .get(PACK_OFFSET + id * PACK_ENTRY_SIZE + variant)
            .map(|&count| count as usize)
    }
}

impl<S: std::ops::DerefMut<Target = Save>> ChipsView<S> {
    /// Write a folder slot's pair of bytes, or answer `false` for a
    /// slot the cart hasn't got.
    fn write_slot(&mut self, folder_index: usize, chip_index: usize, raw: [u8; FOLDER_ENTRY_SIZE]) -> bool {
        let Some(at) = folder_slot(folder_index, chip_index) else {
            return false;
        };
        let Some(slot) = self.save.payload_mut().get_mut(at..at + FOLDER_ENTRY_SIZE) else {
            return false;
        };
        slot.copy_from_slice(&raw);
        true
    }
}

impl<S: std::ops::DerefMut<Target = Save>> save::ChipsViewMut for ChipsView<S> {
    fn set_chip(&mut self, folder_index: usize, chip_index: usize, chip: save::Chip) -> bool {
        let Ok(id) = u8::try_from(chip.id) else {
            return false;
        };
        let code = match chip.code {
            save::ChipCode::Star => RAW_STAR_CODE,
            code => code as u8,
        };
        self.write_slot(folder_index, chip_index, [id, code])
    }

    fn clear_chip(&mut self, folder_index: usize, chip_index: usize) -> bool {
        self.write_slot(folder_index, chip_index, [EMPTY_SLOT; FOLDER_ENTRY_SIZE])
    }

    /// Nothing to rebuild: the cart keeps no shadow copy of the folder
    /// to agree with, the way BN4 onwards do. Its integrity stamps are
    /// the bank's, and those are
    /// [`rebuild_checksum`](save::Save::rebuild_checksum)'s.
    fn rebuild_anticheat(&mut self) {}
}

pub struct NaviView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> save::NaviView for NaviView<S> {
    /// A placeholder, as BN1–4's is: the cart has one navi and no
    /// roster to name him from, so the ROM answers for no navi id and
    /// the identity strip shows the HP alone.
    fn navi(&self) -> usize {
        0
    }

    fn max_hp(&self, _assets: &dyn tango_gamesupport_common_dataview::rom::Assets) -> u16 {
        self.save
            .payload()
            .get(MAX_HP_OFFSET..MAX_HP_OFFSET + std::mem::size_of::<u16>())
            .map(|raw| u16::from_le_bytes(raw.try_into().unwrap()))
            .unwrap_or(0)
    }

    /// The remake's folder rules — see [`MAX_COPIES`] and
    /// [`NAVI_LIMIT`]. They are the cart's own and don't move: it has
    /// no styles, no NaviCust and no patch cards to raise or lower them
    /// with, which is why nothing about the save is read to answer this.
    fn folder_limits(
        &self,
        _assets: &dyn tango_gamesupport_common_dataview::rom::Assets,
    ) -> save::FolderLimits {
        save::FolderLimits {
            navi_limit: Some(NAVI_LIMIT),
            max_copies: |_| MAX_COPIES,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_gamesupport_common_dataview::save::Save as _;

    /// A dump the game has written: the flash tag, then both banks,
    /// checksums and all.
    fn cart(generations: [u32; 2]) -> Vec<u8> {
        let mut buf = vec![0xff; SIZE];
        buf[..MAGIC.len()].copy_from_slice(MAGIC);
        for (index, &at) in BANK_OFFSETS.iter().enumerate() {
            buf[at..at + BANK_HEADER_SIZE].fill(0);
            buf[at..at + 4].copy_from_slice(BANK_MAGIC);
            buf[at + BANK_INDEX_OFFSET..][..4].copy_from_slice(&(index as u32).to_le_bytes());
            buf[at + BANK_GENERATION_OFFSET..][..4].copy_from_slice(&generations[index].to_le_bytes());
            for off in [BANK_SIZE_OFFSET, BANK_SIZE_MIRROR_OFFSET] {
                buf[at + off..][..4].copy_from_slice(&PAYLOAD_SIZE.to_le_bytes());
            }
            // Something to tell the two banks' payloads apart.
            buf[at + BANK_HEADER_SIZE] = index as u8;
            stamp(&mut buf, at);
        }
        buf
    }

    /// What the cart's payload is; the fabricated dumps use the real
    /// one's length so their geometry is the game's.
    const PAYLOAD_SIZE: u32 = 0x2860;

    /// Restamp the bank at `at` the way the game does, so a fabricated
    /// dump is one the parse (and the console) would accept.
    fn stamp(buf: &mut [u8], at: usize) {
        let payload = checksum(&buf[at + BANK_HEADER_SIZE..][..PAYLOAD_SIZE as usize]);
        buf[at + PAYLOAD_CHECKSUM_OFFSET..][..4].copy_from_slice(&payload.to_le_bytes());
        let header = header_checksum(&buf[at..][..BANK_HEADER_SIZE]);
        buf[at + HEADER_CHECKSUM_OFFSET..][..4].copy_from_slice(&header.to_le_bytes());
    }

    #[test]
    fn the_newer_bank_is_the_live_one() {
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

    /// The shape a DS save manager writes: the chip, then as much
    /// padding again. The cart is in there; the rest is address space
    /// no chip answers for.
    #[test]
    fn a_dump_longer_than_the_flash_still_parses() {
        let full = cart([1, 2]);
        let mut padded = full.clone();
        padded.resize(SIZE * 2, ERASED);
        let save = Save::new(&padded).unwrap();
        assert_eq!(save.generation(), 2);
        assert_eq!(save.to_sram_dump(), full);
    }

    /// A dump that stops after the last byte the game ever writes is
    /// the whole save, and reads back as one padded out to the chip.
    #[test]
    fn a_dump_short_of_the_flash_still_parses() {
        let full = cart([1, 2]);
        // 0x5be0 is the last byte the game ever writes; everything from
        // there to the end of the chip is erased either way.
        for len in [0x5be0, 0xc000] {
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
        // Nor does length identify anything: a foreign dump is refused
        // whatever size it arrives at.
        assert!(Save::new(&vec![0; SIZE * 2]).is_err());
    }

    /// A bank the game would refuse is one this refuses too: a header
    /// whose own checksum is stale doesn't count as a bank at all, and
    /// a payload that doesn't match its checksum sends the parse down
    /// to the older bank — the console's own fallback.
    #[test]
    fn a_bank_the_console_would_refuse_is_not_the_live_one() {
        let mut buf = cart([1, 2]);
        // Tamper with the newer bank's payload without restamping.
        buf[BANK_OFFSETS[1] + BANK_HEADER_SIZE + 0x40] ^= 0xff;
        let save = Save::new(&buf).unwrap();
        assert_eq!(save.generation(), 1);

        // Both banks bad: nothing to open on.
        let mut both = buf.clone();
        both[BANK_OFFSETS[0] + BANK_HEADER_SIZE + 0x40] ^= 0xff;
        assert!(matches!(Save::new(&both), Err(Error::ChecksumMismatch { .. })));

        // A stale header checksum takes the bank out of the running
        // before its payload is ever read.
        let mut stale = cart([1, 2]);
        stale[BANK_OFFSETS[1] + BANK_GENERATION_OFFSET] = 0x7f;
        let save = Save::new(&stale).unwrap();
        assert_eq!(save.generation(), 1);
    }

    /// The folder round-trips through the editor's own path: set a
    /// slot, read it back, and the dump the console gets carries the
    /// checksums that make it loadable.
    #[test]
    fn an_edited_folder_reads_back_and_restamps() {
        let mut save = Save::new(&cart([2, 1])).unwrap();
        let edits = [
            (0, save::Chip { id: 33, code: save::ChipCode::B }),
            (29, save::Chip { id: 119, code: save::ChipCode::Star }),
        ];
        {
            let mut view = save.view_chips_mut().unwrap();
            for (slot, chip) in &edits {
                assert!(view.set_chip(0, *slot, chip.clone()));
            }
            assert!(view.clear_chip(0, 1));
            // Out of range on either axis is refused rather than
            // written somewhere else.
            assert!(!view.set_chip(1, 0, edits[0].1.clone()));
            assert!(!view.set_chip(0, 30, edits[0].1.clone()));
            assert!(!view.clear_chip(0, 30));
        }
        save.rebuild_checksum();

        let view = save.view_chips().unwrap();
        for (slot, chip) in &edits {
            assert_eq!(view.chip(0, *slot).as_ref(), Some(chip));
        }
        assert_eq!(view.chip(0, 1), None);
        drop(view);

        // The wildcard goes out as the cart's own code, not Tango's.
        assert_eq!(save.payload()[folder_slot(0, 29).unwrap() + 1], RAW_STAR_CODE);

        // And the result is a save again, on the bank the edit went to.
        let reparsed = Save::new(&save.to_sram_dump()).unwrap();
        assert_eq!(reparsed.generation(), 2);
        assert_eq!(
            reparsed.view_chips().unwrap().chip(0, 0).as_ref(),
            Some(&edits[0].1)
        );
    }

    /// The navi answers with the HP the cart hands him and the rules
    /// his folder is built under.
    #[test]
    fn the_navi_brings_hp_and_folder_rules() {
        use tango_gamesupport_common_dataview::rom::Assets as _;

        let mut save = Save::new(&cart([2, 1])).unwrap();
        let at = save.live.at + BANK_HEADER_SIZE + MAX_HP_OFFSET;
        save.buf[at..][..std::mem::size_of::<u16>()].copy_from_slice(&1000u16.to_le_bytes());

        // No ROM behind it: neither answer reads one.
        let assets = crate::rom::Assets::new(&crate::rom::B6XJ_00, crate::rom::JA_CHARSET, vec![]);
        let view = save.view_navi().unwrap();
        assert_eq!(view.max_hp(&assets), 1000);

        let limits = view.folder_limits(&assets);
        assert_eq!(limits.navi_limit, Some(NAVI_LIMIT));
        assert_eq!((limits.max_copies)(assets.chip(1).unwrap().as_ref()), MAX_COPIES);
        // The rest are other games' rules: this cart has no mega/giga
        // classes, no Regular chip and no Tag pair.
        assert_eq!(limits.mega_limit, None);
        assert_eq!(limits.giga_limit, None);
        assert_eq!(limits.reg_memory, None);
        assert_eq!(limits.tag_memory, None);
    }

    /// The pack answers per code, and only for chips and codes that
    /// exist.
    #[test]
    fn the_pack_counts_per_code() {
        let mut save = Save::new(&cart([2, 1])).unwrap();
        let at = save.live.at + BANK_HEADER_SIZE + PACK_OFFSET + 33 * PACK_ENTRY_SIZE;
        save.buf[at..][..PACK_CODES].copy_from_slice(&[11, 5, 2, 6, 6]);

        let view = save.view_chips().unwrap();
        assert_eq!(view.pack_count(33, 0), Some(11));
        assert_eq!(view.pack_count(33, 4), Some(6));
        // Past the row's codes, and past the table.
        assert_eq!(view.pack_count(33, PACK_CODES), None);
        assert_eq!(view.pack_count(crate::NUM_CHIPS, 0), None);
    }
}
