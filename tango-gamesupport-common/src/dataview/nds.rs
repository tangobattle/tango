//! Just enough of the DS cartridge format to reach the data the DS
//! games' assets read: the file name and allocation tables, the ARM9
//! overlay table, and the compression overlays are stored under.
//!
//! Everything here answers by *name* — a file's path, an overlay's id —
//! rather than by position in the image. The distinction is load-
//! bearing: a patched cart is routinely rebuilt outright (the BN5DS
//! undub repacks the whole image to swap one sound archive, and every
//! other file lands somewhere new, byte-identical but moved), and a
//! rebuild rewrites these tables because the game itself reads through
//! them. An asset reached the way the game reaches it survives any
//! repack; one reached at a raw image offset reads whatever happens to
//! live there now.

/// Where the header keeps the tables: the file name table, the file
/// allocation table, and the ARM9 overlay table — each an offset and a
/// byte length.
const FNT_OFFSET: usize = 0x40;
const FAT_OFFSET: usize = 0x48;
const FAT_LEN: usize = 0x4c;
const OVERLAY_TABLE_OFFSET: usize = 0x50;
const OVERLAY_TABLE_LEN: usize = 0x54;

/// One overlay table entry, in bytes.
const OVERLAY_ENTRY_SIZE: usize = 0x20;

/// One file allocation table entry, in bytes: a start and an end.
const FAT_ENTRY_SIZE: usize = 8;

fn word(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

/// The cartridge image, kept whole, with its file name table walked
/// into a path lookup. Overlays are decoded as they are asked for —
/// OSS keeps one per chip's artwork, over a thousand in all, and a
/// folder only ever draws the handful it shows.
pub struct Cart {
    rom: Vec<u8>,
    /// Every named file on the cart: the name table's paths, `/`-joined
    /// from the root, against the allocation table's ranges.
    files: std::collections::HashMap<String, std::ops::Range<usize>>,
}

impl Cart {
    pub fn new(rom: Vec<u8>) -> Self {
        let files = walk_fnt(&rom);
        Self { rom, files }
    }

    /// The whole image, for the header fields the filesystem doesn't
    /// cover — BN5DS maps its ARM9 static image off the load
    /// parameters at 0x20.
    pub fn rom(&self) -> &[u8] {
        &self.rom
    }

    fn word(&self, at: usize) -> u32 {
        word(&self.rom, at)
    }

    /// The named file's bytes — the path as the name table spells it,
    /// `/`-rooted — or an empty slice when the cart has no such file.
    /// Readers treat short data as missing rather than panicking, so a
    /// cart without the file renders blank.
    pub fn file(&self, path: &str) -> &[u8] {
        self.files
            .get(path)
            .and_then(|range| self.rom.get(range.clone()))
            .unwrap_or(&[])
    }

    /// Decode overlay `id`. `None` for an id the table doesn't hold, a
    /// file range outside the image, or a compressed overlay that
    /// doesn't decode — all of which mean a ROM this build can't read,
    /// and all of which the callers render as missing.
    pub fn overlay(&self, id: u16) -> Option<Overlay> {
        let table = self.word(OVERLAY_TABLE_OFFSET) as usize;
        let count = self.word(OVERLAY_TABLE_LEN) as usize / OVERLAY_ENTRY_SIZE;
        if id as usize >= count {
            return None;
        }
        let entry = table + id as usize * OVERLAY_ENTRY_SIZE;
        let ram = self.word(entry + 0x04);
        let size = self.word(entry + 0x08) as usize;
        let file = self.word(entry + 0x18) as usize;
        // The top byte of the last word flags whether the file is
        // compressed; the low three are the compressed length, which
        // the FAT range already gives us.
        let compressed = self.word(entry + 0x1c) >> 24 != 0;

        let fat = self.word(FAT_OFFSET) as usize;
        let start = self.word(fat + file * FAT_ENTRY_SIZE) as usize;
        let end = self.word(fat + file * FAT_ENTRY_SIZE + 4) as usize;
        let stored = self.rom.get(start..end)?;

        let mut data = if compressed {
            blz_decode(stored)?
        } else {
            stored.to_vec()
        };
        data.resize(size, 0);
        Some(Overlay { ram, data })
    }
}

/// An overlay's decompressed image, with the address it loads at — the
/// pair an address lookup needs.
pub struct Overlay {
    ram: u32,
    data: Vec<u8>,
}

impl Overlay {
    /// The bytes from `addr` to the end of the overlay, or an empty
    /// slice when `addr` isn't in it. Readers treat short data as
    /// missing rather than panicking, so an address that lands in
    /// another overlay (or in heap) renders blank.
    pub fn get(&self, addr: u32) -> &[u8] {
        addr.checked_sub(self.ram)
            .and_then(|off| self.data.get(off as usize..))
            .unwrap_or(&[])
    }
}

/// Walk the file name table against the file allocation table: every
/// named file's full path and its range of the image. Nothing here
/// trusts the ROM — a malformed table yields whatever was walked
/// before the malformation, not a panic and not a loop.
fn walk_fnt(rom: &[u8]) -> std::collections::HashMap<String, std::ops::Range<usize>> {
    let mut files = std::collections::HashMap::new();
    let fnt = word(rom, FNT_OFFSET) as usize;
    let fat = word(rom, FAT_OFFSET) as usize;
    let fat_count = word(rom, FAT_LEN) as usize / FAT_ENTRY_SIZE;

    // Directories are 0xf000-biased ids into the table at its front;
    // each entry is a sub-table offset and the file id its first file
    // entry takes. A directory only ever appears under one parent, so
    // the seen set is loop protection against a malformed table, not
    // something a well-formed cart exercises.
    let mut seen = std::collections::HashSet::from([0xf000u16]);
    let mut stack = vec![(0xf000u16, "/".to_string())];
    while let Some((dir, prefix)) = stack.pop() {
        let entry = fnt + (dir & 0xfff) as usize * 8;
        let mut at = fnt + word(rom, entry) as usize;
        let mut file = word(rom, entry + 4) as u16 as usize;
        loop {
            let Some(&kind) = rom.get(at) else { break };
            at += 1;
            if kind == 0 {
                break;
            }
            let Some(name) = rom.get(at..at + (kind & 0x7f) as usize) else {
                break;
            };
            at += name.len();
            let name = String::from_utf8_lossy(name);
            if kind & 0x80 != 0 {
                let Some(sub) = rom.get(at..at + 2) else { break };
                at += 2;
                let sub = u16::from_le_bytes(sub.try_into().unwrap());
                if seen.insert(sub) {
                    stack.push((sub, format!("{prefix}{name}/")));
                }
            } else {
                if file < fat_count {
                    let start = word(rom, fat + file * FAT_ENTRY_SIZE) as usize;
                    let end = word(rom, fat + file * FAT_ENTRY_SIZE + 4) as usize;
                    files.insert(format!("{prefix}{name}"), start..end);
                }
                file += 1;
            }
        }
    }
    files
}

/// Decode the DS's "BLZ": LZSS run backwards from the end of the file,
/// with the header the compressor left uncompressed at the front. The
/// footer carries the three lengths — how much of the front is plain,
/// how long the coded run is, and how much the file grows.
///
/// `None` for anything malformed. Nothing here trusts the ROM: a bad
/// footer means the caller gets no overlay, not a panic.
fn blz_decode(data: &[u8]) -> Option<Vec<u8>> {
    let word = |at: usize| -> Option<u32> { Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?)) };

    let packed_len = data.len();
    let inc_len = word(packed_len.checked_sub(4)?)? as usize;
    if inc_len == 0 {
        // The compressor left this file alone; the footer is all it added.
        return Some(data[..packed_len - 4].to_vec());
    }
    let header_len = *data.get(packed_len.checked_sub(5)?)? as usize;
    if !(0x08..=0x0b).contains(&header_len) || packed_len <= header_len {
        return None;
    }
    let encoded_len = (word(packed_len.checked_sub(8)?)? & 0x00ff_ffff) as usize;
    let plain_len = packed_len.checked_sub(encoded_len)?;
    let coded_len = encoded_len.checked_sub(header_len)?;
    let raw_len = plain_len + encoded_len + inc_len;

    // The coded run is stored back to front, and decodes to a run that
    // is also back to front: reverse in, reverse out.
    let mut coded = data.get(plain_len..plain_len + coded_len)?.to_vec();
    coded.reverse();

    let target = raw_len.checked_sub(plain_len)?;
    let mut out: Vec<u8> = Vec::with_capacity(target);
    let mut at = 0usize;
    let mut mask = 0u8;
    let mut flags = 0u8;
    while out.len() < target {
        if mask == 0 {
            flags = *coded.get(at)?;
            at += 1;
            mask = 0x80;
        }
        if flags & mask == 0 {
            out.push(*coded.get(at)?);
            at += 1;
        } else {
            let hi = *coded.get(at)? as usize;
            let lo = *coded.get(at + 1)? as usize;
            at += 2;
            let pair = hi << 8 | lo;
            let len = ((pair >> 12) + 3).min(target - out.len());
            let back = (pair & 0xfff) + 3;
            for _ in 0..len {
                let b = *out.get(out.len().checked_sub(back)?)?;
                out.push(b);
            }
        }
        mask >>= 1;
    }
    out.reverse();

    let mut raw = data[..plain_len].to_vec();
    raw.append(&mut out);
    Some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_uncompressed_blz_file_is_its_own_body() {
        // inc_len == 0 says the compressor gave up on this one.
        let mut data = b"hello".to_vec();
        data.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(blz_decode(&data).unwrap(), b"hello");
    }

    #[test]
    fn a_bad_footer_decodes_to_nothing() {
        // A header length outside 8..=0x0b is not a BLZ file.
        let mut data = vec![0u8; 32];
        data[32 - 5] = 0x40;
        data[32 - 4..].copy_from_slice(&1u32.to_le_bytes());
        assert!(blz_decode(&data).is_none());
        // Nor is a file too short to hold a footer at all.
        assert!(blz_decode(&[1, 2, 3]).is_none());
    }

    #[test]
    fn an_overlay_reads_by_address() {
        let overlay = Overlay {
            ram: 0x0204_e0a0,
            data: vec![1, 2, 3, 4],
        };
        assert_eq!(overlay.get(0x0204_e0a1), &[2, 3, 4]);
        // Before the overlay, and past its end, are somebody else's.
        assert_eq!(overlay.get(0x0204_e09f), &[] as &[u8]);
        assert_eq!(overlay.get(0x0204_e0a4), &[] as &[u8]);
    }

    /// A minimal cart image: an FNT with a file at the root and one in
    /// a subdirectory, a FAT with their ranges, and the two files'
    /// bytes.
    fn cart_with_files() -> Cart {
        let mut rom = vec![0u8; 0x200];
        rom[FNT_OFFSET..][..4].copy_from_slice(&0x80u32.to_le_bytes());
        rom[FAT_OFFSET..][..4].copy_from_slice(&0x100u32.to_le_bytes());
        rom[FAT_LEN..][..4].copy_from_slice(&16u32.to_le_bytes());
        // Root (dir 0xf000): sub-table at +0x10, first file id 0; its
        // sub-table names "a.bin" then directory "sub" (0xf001). The
        // subdirectory: sub-table at +0x28, first file id 1, one file
        // "b.bin".
        rom[0x80..][..4].copy_from_slice(&0x10u32.to_le_bytes());
        rom[0x86..][..2].copy_from_slice(&1u16.to_le_bytes()); // 1 directory besides the root
        rom[0x88..][..4].copy_from_slice(&0x28u32.to_le_bytes());
        rom[0x8c..][..2].copy_from_slice(&1u16.to_le_bytes());
        rom[0x90..0x96].copy_from_slice(b"\x05a.bin");
        rom[0x96] = 0x83;
        rom[0x97..0x9a].copy_from_slice(b"sub");
        rom[0x9a..][..2].copy_from_slice(&0xf001u16.to_le_bytes());
        rom[0xa8..0xae].copy_from_slice(b"\x05b.bin");
        // FAT: file 0 at 0x180..0x183, file 1 at 0x190..0x192.
        rom[0x100..][..4].copy_from_slice(&0x180u32.to_le_bytes());
        rom[0x104..][..4].copy_from_slice(&0x183u32.to_le_bytes());
        rom[0x108..][..4].copy_from_slice(&0x190u32.to_le_bytes());
        rom[0x10c..][..4].copy_from_slice(&0x192u32.to_le_bytes());
        rom[0x180..0x183].copy_from_slice(&[1, 2, 3]);
        rom[0x190..0x192].copy_from_slice(&[9, 8]);
        Cart::new(rom)
    }

    #[test]
    fn files_resolve_by_path() {
        let cart = cart_with_files();
        assert_eq!(cart.file("/a.bin"), &[1, 2, 3]);
        assert_eq!(cart.file("/sub/b.bin"), &[9, 8]);
        // A name the cart doesn't have is missing, not a panic.
        assert_eq!(cart.file("/sub/c.bin"), &[] as &[u8]);
    }

    #[test]
    fn an_overlay_resolves_through_the_table_and_the_fat() {
        let mut rom = vec![0u8; 0x200];
        rom[FAT_OFFSET..][..4].copy_from_slice(&0x100u32.to_le_bytes());
        rom[FAT_LEN..][..4].copy_from_slice(&8u32.to_le_bytes());
        rom[OVERLAY_TABLE_OFFSET..][..4].copy_from_slice(&0x80u32.to_le_bytes());
        rom[OVERLAY_TABLE_LEN..][..4].copy_from_slice(&(OVERLAY_ENTRY_SIZE as u32).to_le_bytes());
        // Overlay 0: loads at 0x02000000, 4 bytes, file 0, stored plain.
        rom[0x84..][..4].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        rom[0x88..][..4].copy_from_slice(&4u32.to_le_bytes());
        rom[0x98..][..4].copy_from_slice(&0u32.to_le_bytes());
        rom[0x100..][..4].copy_from_slice(&0x180u32.to_le_bytes());
        rom[0x104..][..4].copy_from_slice(&0x184u32.to_le_bytes());
        rom[0x180..0x184].copy_from_slice(&[4, 5, 6, 7]);
        let cart = Cart::new(rom);
        assert_eq!(cart.overlay(0).unwrap().get(0x0200_0001), &[5, 6, 7]);
        // An id past the table is a cart this build can't read.
        assert!(cart.overlay(1).is_none());
    }
}
