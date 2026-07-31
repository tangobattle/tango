//! Just enough of the DS cartridge format to reach the data the assets
//! read: the overlay table, and the compression the overlays are stored
//! under.
//!
//! BN5DS's cart hands its chip data out of the ARM9 static image, which
//! is a plain slice of the ROM. This one does not — its static image
//! stops at `0x0201e784`, and everything the folder wants (the chip
//! table, the text archives, the icon bank, every chip's artwork) lives
//! in overlays, 1024 of whose 1114 are compressed. So reading an
//! address here means naming the overlay that hosts it and decoding
//! that overlay first.

/// Where the header keeps the overlay table and the FAT.
const FAT_OFFSET: usize = 0x48;
const OVERLAY_TABLE_OFFSET: usize = 0x50;
const OVERLAY_TABLE_LEN: usize = 0x54;

/// One overlay table entry, in bytes.
const OVERLAY_ENTRY_SIZE: usize = 0x20;

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

/// The cartridge image, kept whole so artwork overlays can be decoded
/// as they are asked for — there are 186 of them and a folder only ever
/// draws the handful it shows.
pub struct Cart {
    rom: Vec<u8>,
}

impl Cart {
    pub fn new(rom: Vec<u8>) -> Self {
        Self { rom }
    }

    fn word(&self, at: usize) -> u32 {
        self.rom
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .unwrap_or(0)
    }

    /// Decode overlay `id`. `None` for an id the table doesn't hold, a
    /// file range outside the image, or a compressed overlay that
    /// doesn't decode — all of which mean a ROM this build can't read,
    /// and all of which the callers render as missing art.
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
        let start = self.word(fat + file * 8) as usize;
        let end = self.word(fat + file * 8 + 4) as usize;
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
}
