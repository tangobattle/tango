//! The per-tick input-stream encoding for rollback replays. Because a
//! link is a deterministic function of its input streams, the input
//! stream is the whole story of a match — but a *file* needs framing
//! (boot state, ROM identity, metadata), and that framing is the
//! container's domain, not this module's: the container (this crate's
//! root) writes its own header/state/metadata and hands this module the
//! sink for the input records that follow.
//!
//! One side's input for one tick is an [`Input`]: a 12-bit joypad word
//! (the GBA layout, which the DS extends with X and Y) plus the DS's
//! stylus. Keys only change on a press or release and a resting stylus
//! doesn't move, so most ticks repeat themselves — and a tick that
//! repeats costs its tag byte alone. The tag spends exactly one bit on
//! each question a record has to answer:
//!
//!   bit 7  P0_KEY_REPEAT    p0's keys are the previous tick's (no keys bytes)
//!   bit 6  P0_TOUCH_DOWN    p0's stylus is down
//!   bit 5  P0_TOUCH_REPEAT  ...at its last recorded coordinates (no coord bytes)
//!   bit 4  P1_KEY_REPEAT    ⎫
//!   bit 3  P1_TOUCH_DOWN    ⎬ likewise for p1
//!   bit 2  P1_TOUCH_REPEAT  ⎭
//!   bit 1  MARK             overlay annotation; no effect on decoding
//!   bit 0  SENTINEL         set on every record — see below
//!
//! A record's bytes after the tag come per side, p0's before p1's:
//! explicit keys (a high byte whose low nibble is keys bits 11..8, then
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
//! input records. The cost is that the tag has no reserved bits left —
//! anything more is a schema bump.
//!
//! Marks are the embedder's tick-boundary annotations: a mark flags the
//! tick it is stamped on as the start of a new span, with no meaning of
//! its own beyond that — tango stamps one on each round's first tick.
//!
//! The previous revision of this encoding (schema 0x1D containers)
//! carried bare 10-bit joyflags: per-side default flags, an op bit
//! choosing zero or previous as the default source, an explicit side's
//! high two bits packed into the tag's low nibble, and never a stylus;
//! [`Stream::read_v1`] still decodes it so older recordings keep
//! playing back.

const P0_KEY_REPEAT: u8 = 1 << 7;
const P0_TOUCH_DOWN: u8 = 1 << 6;
const P0_TOUCH_REPEAT: u8 = 1 << 5;
const P1_KEY_REPEAT: u8 = 1 << 4;
const P1_TOUCH_DOWN: u8 = 1 << 3;
const P1_TOUCH_REPEAT: u8 = 1 << 2;
const MARK: u8 = 1 << 1;
/// Set on every record tag so none collides with [`END_OF_STREAM`].
const SENTINEL: u8 = 1 << 0;
const END_OF_STREAM: u8 = 0x00;

/// Explicit keys high byte: the keys' high nibble. Bits 7..=4 are
/// reserved and written zero.
const KEYS_HI: u8 = 0b0000_1111;

/// The 0x1D-era tag: default source (zero vs previous), per-side
/// default flags, and its own MARK position.
const V1_OP_PREV: u8 = 0b1000_0000;
const V1_P0_DEFAULT: u8 = 0b0100_0000;
const V1_P1_DEFAULT: u8 = 0b0010_0000;
const V1_MARK: u8 = 0b0001_0000;

/// One side's input for one tick, as a replay records it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct Input {
    /// Held joypad bits, 12 wide (higher bits are not representable and
    /// are masked off on write).
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
    /// True once [`mark`](Writer::mark) was called; the next
    /// [`push`](Writer::push) sets the MARK bit on its tag byte and
    /// clears this.
    next_is_marked: bool,
    /// What the repeat flags may refer to, per side.
    state: [SideState; 2],
}

impl<W: std::io::Write> Writer<W> {
    /// Wrap `w`, which the embedder has already written its framing
    /// into; everything from here on is input records.
    pub fn new(w: W) -> Self {
        Writer {
            w,
            next_is_marked: false,
            state: [SideState::default(); 2],
        }
    }

    /// Stamp the MARK flag on the next pushed tick. Nothing is emitted
    /// here — a mark with no tick after it (e.g. a crash right at a
    /// boundary) simply never reaches the stream.
    pub fn mark(&mut self) {
        self.next_is_marked = true;
    }

    /// Append one confirmed tick's input pair.
    pub fn push(&mut self, inputs: [Input; 2]) -> std::io::Result<()> {
        let inputs = inputs.map(|input| Input {
            keys: input.keys & 0xfff,
            touch: input.touch,
        });

        let mut tag = SENTINEL;
        if self.next_is_marked {
            tag |= MARK;
            self.next_is_marked = false;
        }
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
    pub fn read(r: impl std::io::Read) -> std::io::Result<Self> {
        let mut state = [SideState::default(); 2];
        Self::read_with(r, MARK, move |r, tag| {
            let mut pair = [Input::default(); 2];
            for (which, (input, state)) in pair.iter_mut().zip(state.iter_mut()).enumerate() {
                let (key_repeat, touch_down, touch_repeat) = [
                    (P0_KEY_REPEAT, P0_TOUCH_DOWN, P0_TOUCH_REPEAT),
                    (P1_KEY_REPEAT, P1_TOUCH_DOWN, P1_TOUCH_REPEAT),
                ][which];
                input.keys = if tag & key_repeat != 0 {
                    state.keys
                } else {
                    let Some(hi) = read_u8_opt(r)? else { return Ok(None) };
                    let Some(lo) = read_u8_opt(r)? else { return Ok(None) };
                    (((hi & KEYS_HI) as u16) << 8) | lo as u16
                };
                input.touch = if tag & touch_down == 0 {
                    None
                } else if tag & touch_repeat != 0 {
                    Some(state.touch)
                } else {
                    let Some(x) = read_u8_opt(r)? else { return Ok(None) };
                    let Some(y) = read_u8_opt(r)? else { return Ok(None) };
                    Some((x, y))
                };
                state.advance(*input);
            }
            Ok(Some(pair))
        })
    }

    /// Decode the previous (schema 0x1D) revision: bare 10-bit
    /// joyflags, per-side defaults, an explicit side's high two bits in
    /// the tag's low nibble, never a stylus.
    pub fn read_v1(r: impl std::io::Read) -> std::io::Result<Self> {
        let mut prev = [0u16; 2];
        Self::read_with(r, V1_MARK, move |r, tag| {
            for (which, side) in prev.iter_mut().enumerate() {
                let default_bit = [V1_P0_DEFAULT, V1_P1_DEFAULT][which];
                *side = if tag & default_bit != 0 {
                    if tag & V1_OP_PREV != 0 { *side } else { 0 }
                } else {
                    let high = ((tag >> (which * 2)) & 0b11) as u16;
                    let Some(low) = read_u8_opt(r)? else { return Ok(None) };
                    (high << 8) | low as u16
                };
            }
            Ok(Some(prev.map(Input::keys)))
        })
    }

    /// The shared record loop: the revisions differ in their MARK bit's
    /// position and in how a tag and its bytes come back as a pair,
    /// which is what `record` decodes (carrying its own repeat state).
    /// Both use the `0x00` sentinel.
    fn read_with(
        mut r: impl std::io::Read,
        mark_bit: u8,
        mut record: impl FnMut(&mut dyn std::io::Read, u8) -> std::io::Result<Option<[Input; 2]>>,
    ) -> std::io::Result<Self> {
        let mut inputs: Vec<[Input; 2]> = Vec::new();
        let mut marks: Vec<usize> = Vec::new();
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

            if tag & mark_bit != 0 {
                marks.push(inputs.len());
            }

            let Some(pair) = record(&mut r, tag)? else {
                break;
            };
            inputs.push(pair);
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

    fn keys(keys: u16) -> Input {
        Input::keys(keys)
    }

    fn touched(k: u16, x: u8, y: u8) -> Input {
        Input {
            keys: k,
            touch: Some((x, y)),
        }
    }

    fn roundtrip(ticks: &[(bool, [Input; 2])]) -> Vec<u8> {
        let mut w = Writer::new(Vec::new());
        for &(marked, inputs) in ticks {
            if marked {
                w.mark();
            }
            w.push(inputs).unwrap();
        }
        let bytes = w.finish().unwrap();
        let s = Stream::read(&bytes[..]).unwrap();
        assert!(s.is_complete);
        assert_eq!(s.inputs, ticks.iter().map(|&(_, inputs)| inputs).collect::<Vec<_>>());
        assert_eq!(
            s.marks,
            ticks
                .iter()
                .enumerate()
                .filter(|(_, &(marked, _))| marked)
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        );
        bytes
    }

    #[test]
    fn roundtrips_representative_streams() {
        roundtrip(&[]);
        roundtrip(&vec![(false, [keys(0), keys(0)]); 500]); // idle: 1 byte/tick
        roundtrip(&[
            (false, [keys(1), keys(2)]),
            (false, [keys(1), keys(2)]),
            (false, [keys(1), keys(2)]),
        ]); // held
        roundtrip(&[
            (false, [keys(0xfff), keys(0x155)]),
            (false, [keys(0), keys(0xaaa)]),
            (false, [keys(0x100), keys(0)]),
        ]); // 12-bit
    }

    #[test]
    fn roundtrips_touch() {
        roundtrip(&[
            (false, [touched(0, 128, 96), keys(0)]),
            (false, [touched(0, 128, 96), keys(0)]), // resting stylus
            (false, [touched(0, 129, 97), keys(0)]), // dragging
            (false, [touched(0x41, 130, 98), keys(0x82)]), // dragging while pressing
            (false, [keys(0), touched(0xfff, 255, 191)]),
            (false, [keys(0), keys(0)]), // both lifted
            // Touch at the origin is still a touch, distinct from none.
            (false, [touched(0, 0, 0), keys(0)]),
        ]);
    }

    #[test]
    fn roundtrips_marks() {
        // Marks on the first tick, mid-stream, and across a held run —
        // the held keys straddling a mark lean on KEY_REPEAT across the
        // boundary.
        roundtrip(&[
            (true, [keys(0x041), keys(0x082)]),
            (false, [keys(0x041), keys(0x082)]),
            (true, [keys(0x041), keys(0x082)]),
            (false, [keys(0), keys(0)]),
            (true, [keys(0x3ff), keys(0)]),
        ]);
    }

    #[test]
    fn resting_touch_is_one_byte_per_tick() {
        // A stylus resting in place repeats its coordinates through the
        // tag, so holding it costs what holding a button does.
        let bytes = roundtrip(&{
            let mut ticks = vec![(false, [touched(0, 100, 50), keys(0)])];
            ticks.extend(vec![(false, [touched(0, 100, 50), keys(0)]); 99]);
            ticks
        });
        // Explicit first touch (tag + x + y), 99 repeats, sentinel.
        assert_eq!(bytes.len(), 3 + 99 + 1);
    }

    #[test]
    fn dragging_restates_coordinates() {
        // A moving stylus spells its coordinates out each tick: tag +
        // (x, y), with the pad still riding its repeat.
        let bytes = roundtrip(&(0..100u8).map(|i| (false, [touched(0, 100 + (i % 2), 50), keys(0)])).collect::<Vec<_>>());
        assert_eq!(bytes.len(), 100 * 3 + 1);
    }

    #[test]
    fn same_spot_retap_repeats_across_the_lift() {
        // TOUCH_REPEAT's referent is the last *recorded* coordinates,
        // which survive a lift — the second tap of a double-tap costs
        // nothing to place.
        let bytes = roundtrip(&[
            (false, [touched(0, 50, 60), keys(0)]), // tap: tag + coords
            (false, [keys(0), keys(0)]),            // lift: tag
            (false, [touched(0, 50, 60), keys(0)]), // same spot: tag
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
            (false, [keys(0), keys(0)]),     // repeat of the initial zeros: 1
            (false, [keys(1), keys(0)]),     // p0 edges: 3
            (false, [keys(1), keys(0)]),     // held: 1
            (false, [keys(1), keys(0x200)]), // p1 edges: 3
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

    #[test]
    fn v1_streams_still_decode() {
        // A hand-laid 0x1D-era stream: an idle tick, an explicit pair
        // with high bits in the tag's low nibble, a previous-tick
        // repeat, then the sentinel.
        let bytes = [
            V1_P0_DEFAULT | V1_P1_DEFAULT,             // [0, 0]
            V1_OP_PREV | 0b01 | (0b10 << 2) | V1_MARK, // explicit both, marked: p0 = 0x155, p1 = 0x2aa
            0x55,
            0xaa,
            V1_OP_PREV | V1_P0_DEFAULT | V1_P1_DEFAULT, // repeat
            END_OF_STREAM,
        ];
        let s = Stream::read_v1(&bytes[..]).unwrap();
        assert!(s.is_complete);
        assert_eq!(
            s.inputs,
            vec![
                [keys(0), keys(0)],
                [keys(0x155), keys(0x2aa)],
                [keys(0x155), keys(0x2aa)],
            ]
        );
        assert_eq!(s.marks, vec![1]);
    }
}
