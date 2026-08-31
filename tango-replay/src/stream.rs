//! The per-tick input-stream encoding for rollback replays. Because a
//! link is a deterministic function of its input streams, the input
//! stream is the whole story of a match — but a *file* needs framing
//! (boot state, ROM identity, metadata), and that framing is the
//! container's domain, not this module's: the container (this crate's
//! root) writes its own header/state/metadata and hands this module the
//! sink for the input records that follow.
//!
//! One side's input for one tick is an [`Input`]: a 13-bit input word
//! (the GBA pad layout, which the DS extends with X, Y and its mic bit)
//! plus the DS's stylus. Keys only change on a press or release and a
//! resting stylus doesn't move, so most ticks repeat themselves — and a
//! tick that repeats costs its tag byte alone. The tag spends one bit on
//! each question a record has to answer:
//!
//!   bit 7  P0_KEY_REPEAT    p0's keys are the previous tick's (no keys bytes)
//!   bit 6  P0_TOUCH_DOWN    p0's stylus is down
//!   bit 5  P0_TOUCH_REPEAT  ...at its last recorded coordinates (no coord bytes)
//!   bit 4  P1_KEY_REPEAT    ⎫
//!   bit 3  P1_TOUCH_DOWN    ⎬ likewise for p1
//!   bit 2  P1_TOUCH_REPEAT  ⎭
//!   bit 1  (reserved)       never written; ignored on read — see below
//!   bit 0  SENTINEL         set on every record — see below
//!
//! A record's bytes after the tag come per side, p0's before p1's:
//! explicit keys (a high byte whose low 5 bits are keys bits 12..8, then
//! the low byte) when KEY_REPEAT is clear, then the stylus's (x, y)
//! when it is down without TOUCH_REPEAT.
//!
//! TOUCH_REPEAT references the side's last *recorded* coordinates,
//! which persist across lifts — tapping the same spot twice spells the
//! coordinates out once. It is meaningful only while TOUCH_DOWN is set
//! (the encoder never emits it alone; the decoder ignores it then), so
//! the down flag alone says whether the side is touching and the record
//! stays self-delimiting from the tag. Coordinates are never guessed:
//! they are either restated, or exactly the last ones recorded.
//!
//! `0x00` is the end-of-stream sentinel, and the SENTINEL bit is what
//! keeps it unmistakable: every record sets it, so no tag can encode as
//! `0x00`. This also means a zero-filled tail (a crash on some
//! filesystems) reads as a clean end rather than as fabricated all-zero
//! input records.
//!
//! Bit 1 used to be a MARK flag — a tick-boundary annotation the
//! container read as a round start. Rounds are a fact about the match
//! the games' telemetry reports, not a fact about the input stream, so
//! nothing stamps them into recordings any more: a replay is inputs and
//! nothing else. The bit is never written and always ignored, which is
//! what keeps recordings made while it was live decoding byte-for-byte
//! identically (a mark carried no bytes of its own) — it is a free
//! reserved bit now, not a schema break in either direction.
//!
const P0_KEY_REPEAT: u8 = 1 << 7;
const P0_TOUCH_DOWN: u8 = 1 << 6;
const P0_TOUCH_REPEAT: u8 = 1 << 5;
const P1_KEY_REPEAT: u8 = 1 << 4;
const P1_TOUCH_DOWN: u8 = 1 << 3;
const P1_TOUCH_REPEAT: u8 = 1 << 2;
/// Set on every record tag so none collides with [`END_OF_STREAM`].
const SENTINEL: u8 = 1 << 0;
const END_OF_STREAM: u8 = 0x00;

/// Explicit keys high byte: the keys' top 5 bits — the pad's high
/// nibble and, above it, the DS's mic bit, which was one of this byte's
/// reserved zeros until the mic became an input. Bits 7..=5 are still
/// reserved and written zero.
const KEYS_HI: u8 = 0b0001_1111;

/// One side's input for one tick, as a replay records it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct Input {
    /// Held input bits, 13 wide — the pad, plus the DS's mic (higher
    /// bits are not representable and are masked off on write).
    pub keys: u16,
    /// Stylus position on the touch screen in that screen's own pixels
    /// (the DS's fits a byte per axis), or `None` for a lifted stylus.
    pub touch: Option<(u8, u8)>,
}

impl Input {
    /// An input with nothing but the joypad held.
    pub fn keys(keys: u16) -> Self {
        Input { keys, touch: None }
    }
}

/// The codec's memory of one side: what its repeat flags can refer to.
/// The writer and the reader each track one per side, identically, so a
/// repeat always resolves to what was actually recorded.
#[derive(Clone, Copy, Default)]
struct SideState {
    /// The previous tick's keys — KEY_REPEAT's referent.
    keys: u16,
    /// The last coordinates recorded, persisting across lifts —
    /// TOUCH_REPEAT's referent. Starts at the origin.
    touch: (u8, u8),
}

impl SideState {
    fn advance(&mut self, input: Input) {
        self.keys = input.keys;
        if let Some(touch) = input.touch {
            self.touch = touch;
        }
    }
}

fn read_u8(r: &mut (impl std::io::Read + ?Sized)) -> std::io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

/// One byte, or `None` at EOF — the mid-record truncation case the
/// decoders turn into a dropped partial record.
fn read_u8_opt(r: &mut (impl std::io::Read + ?Sized)) -> std::io::Result<Option<u8>> {
    match read_u8(r) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

/// Streams input records into `w` as they come; nothing is held back for
/// the end but the one-byte sentinel, so a recording that dies mid-match
/// still parses up to its last flushed tick.
pub struct Writer<W: std::io::Write> {
    w: W,
    /// What the repeat flags may refer to, per side.
    state: [SideState; 2],
}

impl<W: std::io::Write> Writer<W> {
    /// Wrap `w`, which the embedder has already written its framing
    /// into; everything from here on is input records.
    pub fn new(w: W) -> Self {
        Writer {
            w,
            state: [SideState::default(); 2],
        }
    }

    /// Append one confirmed tick's input pair.
    pub fn push(&mut self, inputs: [Input; 2]) -> std::io::Result<()> {
        let inputs = inputs.map(|input| Input {
            keys: input.keys & 0x1fff,
            touch: input.touch,
        });

        let mut tag = SENTINEL;
        // Tag + two sides of up to 4 bytes each.
        let mut record = [0u8; 9];
        let mut len = 1;
        for (which, (input, state)) in inputs.iter().zip(self.state.iter_mut()).enumerate() {
            let (key_repeat, touch_down, touch_repeat) = [
                (P0_KEY_REPEAT, P0_TOUCH_DOWN, P0_TOUCH_REPEAT),
                (P1_KEY_REPEAT, P1_TOUCH_DOWN, P1_TOUCH_REPEAT),
            ][which];
            if input.keys == state.keys {
                tag |= key_repeat;
            } else {
                record[len] = ((input.keys >> 8) as u8) & KEYS_HI;
                record[len + 1] = input.keys as u8;
                len += 2;
            }
            if let Some((x, y)) = input.touch {
                tag |= touch_down;
                if (x, y) == state.touch {
                    tag |= touch_repeat;
                } else {
                    record[len] = x;
                    record[len + 1] = y;
                    len += 2;
                }
            }
            state.advance(*input);
        }
        record[0] = tag;
        self.w.write_all(&record[..len])
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
    pub inputs: Vec<[Input; 2]>,
    /// Whether the stream ended on the sentinel (vs. a truncated tail).
    pub is_complete: bool,
}

impl Stream {
    /// Streaming decode from `r`, positioned at the first tag byte; a
    /// clean end leaves `r` positioned just past the sentinel. EOF
    /// mid-stream drops the partial record and yields
    /// `is_complete = false` (so a crashed recording still plays back
    /// everything that was flushed); any other I/O error propagates.
    pub fn read(r: impl std::io::Read) -> std::io::Result<Self> {
        let mut r = r;
        let mut state = [SideState::default(); 2];
        let mut inputs: Vec<[Input; 2]> = Vec::new();
        let mut is_complete = false;

        'stream: loop {
            let tag = match read_u8(&mut r) {
                Ok(tag) => tag,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            };
            if tag == END_OF_STREAM {
                is_complete = true;
                break;
            }

            let mut pair = [Input::default(); 2];
            for (which, (input, state)) in pair.iter_mut().zip(state.iter_mut()).enumerate() {
                let (key_repeat, touch_down, touch_repeat) = [
                    (P0_KEY_REPEAT, P0_TOUCH_DOWN, P0_TOUCH_REPEAT),
                    (P1_KEY_REPEAT, P1_TOUCH_DOWN, P1_TOUCH_REPEAT),
                ][which];
                input.keys = if tag & key_repeat != 0 {
                    state.keys
                } else {
                    let Some(hi) = read_u8_opt(&mut r)? else {
                        break 'stream;
                    };
                    let Some(lo) = read_u8_opt(&mut r)? else {
                        break 'stream;
                    };
                    (((hi & KEYS_HI) as u16) << 8) | lo as u16
                };
                input.touch = if tag & touch_down == 0 {
                    None
                } else if tag & touch_repeat != 0 {
                    Some(state.touch)
                } else {
                    let Some(x) = read_u8_opt(&mut r)? else {
                        break 'stream;
                    };
                    let Some(y) = read_u8_opt(&mut r)? else {
                        break 'stream;
                    };
                    Some((x, y))
                };
                state.advance(*input);
            }
            inputs.push(pair);
        }

        Ok(Stream { inputs, is_complete })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(keys: u16) -> Input {
        Input::keys(keys)
    }

    fn touched(k: u16, x: u8, y: u8) -> Input {
        Input {
            keys: k,
            touch: Some((x, y)),
        }
    }

    fn roundtrip(ticks: &[[Input; 2]]) -> Vec<u8> {
        let mut w = Writer::new(Vec::new());
        for &inputs in ticks {
            w.push(inputs).unwrap();
        }
        let bytes = w.finish().unwrap();
        let s = Stream::read(&bytes[..]).unwrap();
        assert!(s.is_complete);
        assert_eq!(s.inputs, ticks);
        bytes
    }

    #[test]
    fn roundtrips_representative_streams() {
        roundtrip(&[]);
        roundtrip(&vec![[keys(0), keys(0)]; 500]); // idle: 1 byte/tick
        roundtrip(&[[keys(1), keys(2)], [keys(1), keys(2)], [keys(1), keys(2)]]); // held
        roundtrip(&[
            [keys(0x1fff), keys(0x155)],
            [keys(0), keys(0xaaa)],
            [keys(0x100), keys(0)],
            // The mic bit alone: it sits above the pad in the same word,
            // so it has to survive the high byte on its own.
            [keys(0x1000), keys(0x1001)],
        ]); // 13-bit
    }

    #[test]
    fn roundtrips_touch() {
        roundtrip(&[
            [touched(0, 128, 96), keys(0)],
            [touched(0, 128, 96), keys(0)],       // resting stylus
            [touched(0, 129, 97), keys(0)],       // dragging
            [touched(0x41, 130, 98), keys(0x82)], // dragging while pressing
            [keys(0), touched(0xfff, 255, 191)],
            [keys(0), keys(0)], // both lifted
            // Touch at the origin is still a touch, distinct from none.
            [touched(0, 0, 0), keys(0)],
        ]);
    }

    #[test]
    fn resting_touch_is_one_byte_per_tick() {
        // A stylus resting in place repeats its coordinates through the
        // tag, so holding it costs what holding a button does.
        let bytes = roundtrip(&{
            let mut ticks = vec![[touched(0, 100, 50), keys(0)]];
            ticks.extend(vec![[touched(0, 100, 50), keys(0)]; 99]);
            ticks
        });
        // Explicit first touch (tag + x + y), 99 repeats, sentinel.
        assert_eq!(bytes.len(), 3 + 99 + 1);
    }

    #[test]
    fn dragging_restates_coordinates() {
        // A moving stylus spells its coordinates out each tick: tag +
        // (x, y), with the pad still riding its repeat.
        let bytes = roundtrip(
            &(0..100u8)
                .map(|i| [touched(0, 100 + (i % 2), 50), keys(0)])
                .collect::<Vec<_>>(),
        );
        assert_eq!(bytes.len(), 100 * 3 + 1);
    }

    #[test]
    fn same_spot_retap_repeats_across_the_lift() {
        // TOUCH_REPEAT's referent is the last *recorded* coordinates,
        // which survive a lift — the second tap of a double-tap costs
        // nothing to place.
        let bytes = roundtrip(&[
            [touched(0, 50, 60), keys(0)], // tap: tag + coords
            [keys(0), keys(0)],            // lift: tag
            [touched(0, 50, 60), keys(0)], // same spot: tag
        ]);
        assert_eq!(bytes.len(), 3 + 1 + 1 + 1);
    }

    #[test]
    fn idle_run_is_one_byte_per_tick() {
        let mut w = Writer::new(Vec::new());
        for _ in 0..1000 {
            w.push([keys(0), keys(0)]).unwrap();
        }
        let bytes = w.finish().unwrap();
        assert_eq!(bytes.len(), 1000 + 1);
    }

    #[test]
    fn keys_edge_costs_only_its_side() {
        let bytes = roundtrip(&[
            [keys(0), keys(0)],     // repeat of the initial zeros: 1
            [keys(1), keys(0)],     // p0 edges: 3
            [keys(1), keys(0)],     // held: 1
            [keys(1), keys(0x200)], // p1 edges: 3
        ]);
        assert_eq!(bytes.len(), 1 + 3 + 1 + 3 + 1);
    }

    #[test]
    fn zero_fill_reads_as_a_clean_end() {
        // Record tags always carry the SENTINEL bit, so a zero-filled
        // tail (a crash on some filesystems) terminates decode instead
        // of fabricating input records.
        let mut w = Writer::new(Vec::new());
        w.push([keys(0x155), keys(0x2aa)]).unwrap();
        let mut bytes = w.finish().unwrap();
        bytes.extend_from_slice(&[0u8; 64]);
        let s = Stream::read(&bytes[..]).unwrap();
        assert!(s.is_complete);
        assert_eq!(s.inputs, vec![[keys(0x155), keys(0x2aa)]]);
    }

    #[test]
    fn truncated_tail_recovers_prefix() {
        let mut w = Writer::new(Vec::new());
        for i in 0..10u16 {
            w.push([touched(i & 0xfff, i as u8, 3), keys((i * 3) & 0xfff)]).unwrap();
        }
        let mut bytes = w.finish().unwrap();
        bytes.truncate(bytes.len() - 3); // eat the sentinel + last tick's tail
        let s = Stream::read(&bytes[..]).unwrap();
        assert!(!s.is_complete);
        assert!(s.inputs.len() >= 8);
    }
}
