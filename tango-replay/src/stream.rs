//! The per-tick input-stream encoding for rollback replays. Because a
//! link is a deterministic function of its joypad streams, the input
//! stream is the whole story of a match — but a *file* needs framing
//! (boot state, ROM identity, metadata), and that framing is the
//! container's domain, not this module's: the container (this crate's
//! root) writes its own header/state/metadata and hands this module the
//! sink for the input records that follow.
//!
//! A GBA joypad is only 10 bits, and most ticks are idle or repeat the
//! previous tick, so one tag byte usually stands in for a whole
//! `[p0, p1]` pair, with an explicit low byte appended only for a side
//! that changed. A whole-session recording is therefore dominated by
//! long idle runs that cost a byte each, not the flat 4 bytes/tick a
//! naive layout would spend.
//!
//! Tag byte layout:
//!   bit 7 (op): 0 = "default value is zero", 1 = "default value is the previous tick"
//!   bit 6:      p0 takes the default (no p0 low byte follows)
//!   bit 5:      p1 takes the default (no p1 low byte follows)
//!   bit 4:      MARK flag (overlay annotation; no effect on decoding)
//!   bits 0..=1: high 2 bits of an explicit p0
//!   bits 2..=3: high 2 bits of an explicit p1
//!
//! `0x00` is the end-of-stream sentinel. The op bit is informationally
//! redundant when both sides are explicit, so the encoder always sets it
//! (op=1) in that case to keep the tag byte clear of the sentinel.
//!
//! Marks are the embedder's tick-boundary annotations: a mark flags the
//! tick it is stamped on as the start of a new span, with no meaning of
//! its own beyond that — tango stamps one on each round's first tick.

const OP_PREV: u8 = 0b1000_0000;
const P0_DEFAULT: u8 = 0b0100_0000;
const P1_DEFAULT: u8 = 0b0010_0000;
const MARK: u8 = 0b0001_0000;
const END_OF_STREAM: u8 = 0x00;

fn read_u8(r: &mut impl std::io::Read) -> std::io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

/// Streams input records into `w` as they come; nothing is held back for
/// the end but the one-byte sentinel, so a recording that dies mid-match
/// still parses up to its last flushed tick.
pub struct Writer<W: std::io::Write> {
    w: W,
    /// True once [`mark`](Writer::mark) was called; the next
    /// [`push`](Writer::push) sets the MARK bit on its tag byte and
    /// clears this.
    next_is_marked: bool,
    /// Last `[p0, p1]` emitted, for the "default = previous" tag form.
    prev: [u16; 2],
}

impl<W: std::io::Write> Writer<W> {
    /// Wrap `w`, which the embedder has already written its framing
    /// into; everything from here on is input records.
    pub fn new(w: W) -> Self {
        Writer {
            w,
            next_is_marked: false,
            prev: [0, 0],
        }
    }

    /// Stamp the MARK flag on the next pushed tick. Nothing is emitted
    /// here — a mark with no tick after it (e.g. a crash right at a
    /// boundary) simply never reaches the stream.
    pub fn mark(&mut self) {
        self.next_is_marked = true;
    }

    /// Append one confirmed tick's input pair. GBA joypads are 10 bits.
    pub fn push(&mut self, keys: [u16; 2]) -> std::io::Result<()> {
        let [p0, p1] = keys;

        // Pick whichever op (default = zero vs default = previous) lets
        // more sides "take the default" — fewer explicit sides = smaller
        // record. Tie-break toward op=0 so the canonical idle tick stays
        // a single 0x40|0x20 byte.
        let op0 = (p0 == 0, p1 == 0);
        let op1 = (p0 == self.prev[0], p1 == self.prev[1]);
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
        if self.next_is_marked {
            tag |= MARK;
            self.next_is_marked = false;
        }

        let mut record = [tag, 0, 0];
        let mut len = 1;
        if !p0_default {
            record[len] = (p0 & 0xff) as u8;
            len += 1;
        }
        if !p1_default {
            record[len] = (p1 & 0xff) as u8;
            len += 1;
        }
        self.w.write_all(&record[..len])?;
        self.prev = [p0, p1];
        Ok(())
    }

    /// Write the end-of-stream sentinel, flush, and hand back the sink.
    pub fn finish(mut self) -> std::io::Result<W> {
        self.w.write_all(&[END_OF_STREAM])?;
        self.w.flush()?;
        Ok(self.w)
    }
}

/// A decoded input stream.
pub struct Stream {
    pub inputs: Vec<[u16; 2]>,
    /// Indices into `inputs` of the ticks whose MARK flag was set, in
    /// stream order — exactly as recorded, no normalization. A record
    /// that was marked but then truncated mid-parse leaves its mark
    /// dangling at `inputs.len()`.
    pub marks: Vec<usize>,
    /// Whether the stream ended on the sentinel (vs. a truncated tail).
    pub is_complete: bool,
}

impl Stream {
    /// Streaming decode from `r`, positioned at the first tag byte; a
    /// clean end leaves `r` positioned just past the sentinel. EOF
    /// mid-stream drops the partial record and yields
    /// `is_complete = false` (so a crashed recording still plays back
    /// everything that was flushed); any other I/O error propagates.
    pub fn read(mut r: impl std::io::Read) -> std::io::Result<Self> {
        // One side's value, or None on EOF mid-record.
        fn side(
            r: &mut impl std::io::Read,
            tag: u8,
            shift: u32,
            default_bit: u8,
            prev: u16,
        ) -> std::io::Result<Option<u16>> {
            Ok(if tag & default_bit != 0 {
                Some(if tag & OP_PREV != 0 { prev } else { 0 })
            } else {
                let high = ((tag >> shift) & 0b11) as u16;
                match read_u8(r) {
                    Ok(low) => Some((high << 8) | low as u16),
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => None,
                    Err(e) => return Err(e),
                }
            })
        }

        let mut inputs: Vec<[u16; 2]> = Vec::new();
        let mut marks: Vec<usize> = Vec::new();
        let mut prev: [u16; 2] = [0, 0];
        let mut is_complete = false;

        loop {
            let tag = match read_u8(&mut r) {
                Ok(tag) => tag,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            };
            if tag == END_OF_STREAM {
                is_complete = true;
                break;
            }

            if tag & MARK != 0 {
                marks.push(inputs.len());
            }

            let Some(p0) = side(&mut r, tag, 0, P0_DEFAULT, prev[0])? else {
                break;
            };
            let Some(p1) = side(&mut r, tag, 2, P1_DEFAULT, prev[1])? else {
                break;
            };
            prev = [p0, p1];
            inputs.push([p0, p1]);
        }

        Ok(Stream {
            inputs,
            marks,
            is_complete,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(ticks: &[(bool, [u16; 2])]) -> Stream {
        let mut w = Writer::new(Vec::new());
        for &(marked, keys) in ticks {
            if marked {
                w.mark();
            }
            w.push(keys).unwrap();
        }
        let bytes = w.finish().unwrap();
        let s = Stream::read(&bytes[..]).unwrap();
        assert!(s.is_complete);
        assert_eq!(s.inputs, ticks.iter().map(|&(_, keys)| keys).collect::<Vec<_>>());
        assert_eq!(
            s.marks,
            ticks
                .iter()
                .enumerate()
                .filter(|(_, &(marked, _))| marked)
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        );
        s
    }

    #[test]
    fn roundtrips_representative_streams() {
        roundtrip(&[]);
        roundtrip(&vec![(false, [0, 0]); 500]); // idle: 1 byte/tick
        roundtrip(&[(false, [1, 2]), (false, [1, 2]), (false, [1, 2])]); // held
        roundtrip(&[(false, [0x3ff, 0x155]), (false, [0, 0x2aa]), (false, [0x100, 0])]); // 10-bit
    }

    #[test]
    fn roundtrips_marks() {
        // Marks on the first tick, mid-stream, and across a held run —
        // the held value straddling a mark leans on the previous-tick
        // default across the boundary.
        roundtrip(&[
            (true, [0x041, 0x082]),
            (false, [0x041, 0x082]),
            (true, [0x041, 0x082]),
            (false, [0, 0]),
            (true, [0x3ff, 0]),
        ]);
    }

    #[test]
    fn marked_tick_keeps_the_previous_tick_default() {
        // A mark annotates its tick, it doesn't touch the codec: a held
        // pair straddling a mark still costs one tag byte.
        let mut w = Writer::new(Vec::new());
        w.push([0x155, 0x2aa]).unwrap();
        w.mark();
        w.push([0x155, 0x2aa]).unwrap();
        let bytes = w.finish().unwrap();
        // Explicit first tick (tag + 2 low bytes), 1-byte marked tick,
        // sentinel.
        assert_eq!(bytes.len(), 3 + 1 + 1);
        let s = Stream::read(&bytes[..]).unwrap();
        assert_eq!(s.inputs, vec![[0x155, 0x2aa], [0x155, 0x2aa]]);
        assert_eq!(s.marks, vec![1]);
    }

    #[test]
    fn unmarked_stream_matches_the_premark_encoding() {
        // Streams without marks are byte-identical to the format as it
        // existed before the MARK bit — old recordings stay readable and
        // markless writers stay compatible with old readers.
        let mut w = Writer::new(Vec::new());
        for _ in 0..100 {
            w.push([0, 0]).unwrap();
        }
        let bytes = w.finish().unwrap();
        assert_eq!(bytes, [vec![P0_DEFAULT | P1_DEFAULT; 100], vec![END_OF_STREAM]].concat());
    }

    #[test]
    fn idle_run_is_one_byte_per_tick() {
        let mut w = Writer::new(Vec::new());
        for _ in 0..1000 {
            w.push([0, 0]).unwrap();
        }
        let bytes = w.finish().unwrap();
        // 1000 idle tag bytes + 1 sentinel. The old flat layout spent
        // 4000 on the ticks alone.
        assert_eq!(bytes.len(), 1000 + 1);
    }

    #[test]
    fn truncated_tail_recovers_prefix() {
        let mut w = Writer::new(Vec::new());
        for i in 0..10u16 {
            w.push([i & 0x3ff, (i * 3) & 0x3ff]).unwrap();
        }
        let mut bytes = w.finish().unwrap();
        bytes.truncate(bytes.len() - 3); // eat the sentinel + last tick's tail
        let s = Stream::read(&bytes[..]).unwrap();
        assert!(!s.is_complete);
        assert!(s.inputs.len() >= 8);
    }
}
