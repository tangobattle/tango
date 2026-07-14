//! Replay container for SIO-rollback matches. Because the pair is a
//! deterministic function of the two joypad streams, a replay is just the
//! boot configuration plus confirmed inputs per tick — no savestate
//! stream, no per-game knowledge, one format for every link game.

/// Bumped on any incompatible layout change.
const MAGIC: &[u8; 12] = b"TANGOSIOLNK\x01";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a siolink replay")]
    BadMagic,
    #[error("truncated replay")]
    Truncated,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SideMeta {
    pub rom_crc32: u32,
    /// Save image the side booted with, if any. Embedding it keeps the
    /// replay self-sufficient given the ROMs.
    pub save: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    /// Micros since the unix epoch the carts' RTC was pinned to; None if
    /// the match ran without a pinned clock (non-RTC games only).
    pub rtc_unix_micros: Option<u64>,
    pub sides: [SideMeta; 2],
}

pub struct Writer {
    buf: Vec<u8>,
    ticks_at: usize,
    ticks: u32,
}

impl Writer {
    pub fn new(metadata: &Metadata) -> Self {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&metadata.rtc_unix_micros.map_or(u64::MAX, |v| v).to_le_bytes());
        for side in &metadata.sides {
            buf.extend_from_slice(&side.rom_crc32.to_le_bytes());
            match &side.save {
                Some(save) => {
                    buf.extend_from_slice(&(save.len() as u32).to_le_bytes());
                    buf.extend_from_slice(save);
                }
                None => buf.extend_from_slice(&u32::MAX.to_le_bytes()),
            }
        }
        // Tick count backpatched by finish(); inputs follow.
        let ticks_at = buf.len();
        buf.extend_from_slice(&0u32.to_le_bytes());
        Writer {
            buf,
            ticks_at,
            ticks: 0,
        }
    }

    /// Append one confirmed tick's input pair. GBA joypads are 10 bits;
    /// stored as u16 per side.
    pub fn push(&mut self, keys: [u32; 2]) {
        for k in keys {
            self.buf.extend_from_slice(&(k as u16).to_le_bytes());
        }
        self.ticks += 1;
    }

    pub fn finish(mut self) -> Vec<u8> {
        let at = self.ticks_at;
        self.buf[at..at + 4].copy_from_slice(&self.ticks.to_le_bytes());
        self.buf
    }
}

pub struct Replay {
    pub metadata: Metadata,
    pub inputs: Vec<[u32; 2]>,
}

impl Replay {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Cursor { data, at: 0 };
        if r.take(MAGIC.len())? != MAGIC.as_slice() {
            return Err(Error::BadMagic);
        }
        let rtc = u64::from_le_bytes(r.take(8)?.try_into().unwrap());
        let mut metadata = Metadata {
            rtc_unix_micros: (rtc != u64::MAX).then_some(rtc),
            sides: Default::default(),
        };
        for side in &mut metadata.sides {
            side.rom_crc32 = u32::from_le_bytes(r.take(4)?.try_into().unwrap());
            let len = u32::from_le_bytes(r.take(4)?.try_into().unwrap());
            if len != u32::MAX {
                side.save = Some(r.take(len as usize)?.to_vec());
            }
        }
        let ticks = u32::from_le_bytes(r.take(4)?.try_into().unwrap());
        let mut inputs = Vec::with_capacity(ticks as usize);
        for _ in 0..ticks {
            let p0 = u16::from_le_bytes(r.take(2)?.try_into().unwrap());
            let p1 = u16::from_le_bytes(r.take(2)?.try_into().unwrap());
            inputs.push([p0 as u32, p1 as u32]);
        }
        Ok(Replay { metadata, inputs })
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.data.len() - self.at < n {
            return Err(Error::Truncated);
        }
        let s = &self.data[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
}
