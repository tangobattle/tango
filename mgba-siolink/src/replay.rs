//! Replay container for SIO-rollback matches. Because the pair is a
//! deterministic function of the two joypad streams, a replay is just the
//! boot configuration plus confirmed inputs per tick — no savestate
//! stream, no per-game knowledge, one format for every link game.
//!
//! The input stream uses the same compact per-tick encoding as tango's
//! trap-engine replays: a joypad is only 10 bits, and most ticks are idle
//! or repeat the previous tick, so one tag byte usually stands in for a
//! whole `[p0, p1]` pair, with an explicit low byte appended only for a
//! side that changed. A whole-session recording (priming included) is
//! therefore dominated by long idle runs that cost a byte each, not the
//! flat 4 bytes/tick the naive layout spent.

/// Bumped on any incompatible layout change. `\x02` = compact tag stream
/// (was `\x01`, four flat bytes per tick).
const MAGIC: &[u8; 12] = b"TANGOSIOLNK\x02";

/// Per-tick tag byte. Mirrors tango-pvp's replay encoding minus the
/// round-start bit (the SIO pair has no round concept):
///   bit 7 (op): 0 = default is zero, 1 = default is the previous tick
///   bit 6:      p0 takes the default (no p0 low byte follows)
///   bit 5:      p1 takes the default (no p1 low byte follows)
///   bits 0..=1: high 2 bits of an explicit p0
///   bits 2..=3: high 2 bits of an explicit p1
///
/// `0x00` is the end-of-stream sentinel. Both-explicit-both-zero would
/// also encode as `0x00`, so the writer forces op=1 whenever both sides
/// are explicit, keeping the tag clear of the sentinel.
const OP_PREV: u8 = 0b1000_0000;
const P0_DEFAULT: u8 = 0b0100_0000;
const P1_DEFAULT: u8 = 0b0010_0000;
const END_OF_REPLAY: u8 = 0x00;

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
    /// Last `(p0, p1)` emitted, for the "default = previous" tag form.
    prev: (u16, u16),
}

impl Writer {
    pub fn new(metadata: &Metadata) -> Self {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&metadata.rtc_unix_micros.unwrap_or(u64::MAX).to_le_bytes());
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
        Writer { buf, prev: (0, 0) }
    }

    /// Append one confirmed tick's input pair. GBA joypads are 10 bits.
    pub fn push(&mut self, keys: [u32; 2]) {
        let (p0, p1) = (keys[0] as u16, keys[1] as u16);

        // Prefer whichever default sense (zero vs previous) leaves fewer
        // sides explicit; tie-break to op=0 so the canonical idle tick
        // stays a single 0x40|0x20 byte.
        let op0 = (p0 == 0, p1 == 0);
        let op1 = (p0 == self.prev.0, p1 == self.prev.1);
        let op0_explicit = (!op0.0 as u32) + (!op0.1 as u32);
        let op1_explicit = (!op1.0 as u32) + (!op1.1 as u32);
        let (mut op_prev, p0_default, p1_default) = if op1_explicit < op0_explicit {
            (true, op1.0, op1.1)
        } else {
            (false, op0.0, op0.1)
        };
        // Keep an all-explicit tick's tag off the 0x00 sentinel.
        if !p0_default && !p1_default {
            op_prev = true;
        }

        let mut tag = 0u8;
        if op_prev {
            tag |= OP_PREV;
        }
        if p0_default {
            tag |= P0_DEFAULT;
        } else {
            tag |= ((p0 >> 8) & 0b11) as u8;
        }
        if p1_default {
            tag |= P1_DEFAULT;
        } else {
            tag |= (((p1 >> 8) & 0b11) as u8) << 2;
        }

        self.buf.push(tag);
        if !p0_default {
            self.buf.push((p0 & 0xff) as u8);
        }
        if !p1_default {
            self.buf.push((p1 & 0xff) as u8);
        }
        self.prev = (p0, p1);
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.buf.push(END_OF_REPLAY);
        self.buf
    }
}

pub struct Replay {
    pub metadata: Metadata,
    pub inputs: Vec<[u32; 2]>,
    /// Whether the stream ended on the sentinel (vs. a truncated tail).
    pub is_complete: bool,
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

        // Streaming tag decode. `0x00` ends cleanly; a truncated tail
        // (missing an explicit low byte, or no sentinel) drops the partial
        // tick and leaves is_complete = false, so a crashed recording
        // still plays back everything that was flushed.
        let mut inputs = Vec::new();
        let mut prev = (0u16, 0u16);
        let mut is_complete = false;
        while let Ok(&tag) = r.peek() {
            r.at += 1;
            if tag == END_OF_REPLAY {
                is_complete = true;
                break;
            }
            let op_prev = tag & OP_PREV != 0;
            let side = |explicit_bit: u8, default_bit: u8, prev_v: u16, r: &mut Cursor| -> Option<u16> {
                if tag & default_bit != 0 {
                    Some(if op_prev { prev_v } else { 0 })
                } else {
                    let high = ((tag >> explicit_bit) & 0b11) as u16;
                    let low = *r.take(1).ok()?.first().unwrap() as u16;
                    Some((high << 8) | low)
                }
            };
            let Some(p0) = side(0, P0_DEFAULT, prev.0, &mut r) else {
                break;
            };
            let Some(p1) = side(2, P1_DEFAULT, prev.1, &mut r) else {
                break;
            };
            prev = (p0, p1);
            inputs.push([p0 as u32, p1 as u32]);
        }

        Ok(Replay {
            metadata,
            inputs,
            is_complete,
        })
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

    fn peek(&self) -> Result<&'a u8, Error> {
        self.data.get(self.at).ok_or(Error::Truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(inputs: &[[u32; 2]]) -> Replay {
        let mut w = Writer::new(&Metadata {
            rtc_unix_micros: Some(1_752_000_000_000_000),
            sides: [
                SideMeta {
                    rom_crc32: 0xdead_beef,
                    save: Some(vec![1, 2, 3]),
                },
                SideMeta {
                    rom_crc32: 0x1234_5678,
                    save: None,
                },
            ],
        });
        for &k in inputs {
            w.push(k);
        }
        let bytes = w.finish();
        let parsed = Replay::parse(&bytes).unwrap();
        assert_eq!(parsed.inputs, inputs);
        assert!(parsed.is_complete);
        assert_eq!(parsed.metadata.sides[0].rom_crc32, 0xdead_beef);
        assert_eq!(parsed.metadata.sides[0].save.as_deref(), Some([1, 2, 3].as_slice()));
        parsed
    }

    #[test]
    fn roundtrips_representative_streams() {
        roundtrip(&[]);
        roundtrip(&[[0, 0]; 500]); // idle: 1 byte/tick
        roundtrip(&[[1, 2], [1, 2], [1, 2]]); // held: repeats take the previous default
        roundtrip(&[[0x3ff, 0x155], [0, 0x2aa], [0x100, 0]]); // full 10-bit values, high bits set
    }

    #[test]
    fn idle_run_is_one_byte_per_tick() {
        let mut w = Writer::new(&Metadata::default());
        for _ in 0..1000 {
            w.push([0, 0]);
        }
        let bytes = w.finish();
        // 12 magic + 8 rtc + 2*(4 crc + 4 no-save) = 36 header, + 1000
        // idle tag bytes + 1 sentinel. The old flat layout spent 4000.
        assert_eq!(bytes.len(), 36 + 1000 + 1);
    }

    #[test]
    fn truncated_tail_recovers_prefix() {
        let mut w = Writer::new(&Metadata::default());
        for i in 0..10u32 {
            w.push([i & 0x3ff, (i * 3) & 0x3ff]);
        }
        let mut bytes = w.finish();
        bytes.truncate(bytes.len() - 3); // eat the sentinel + last tick's tail
        let parsed = Replay::parse(&bytes).unwrap();
        assert!(!parsed.is_complete);
        assert!(parsed.inputs.len() >= 8); // most of the stream survives
    }
}
