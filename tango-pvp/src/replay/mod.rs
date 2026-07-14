pub mod export;
mod protos;

use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use prost::Message;
use std::io::{Read, Write};

pub use protos::replay11::metadata;
pub type Metadata = protos::replay11::Metadata;

pub const HEADER: &[u8] = b"TOOT";
/// SIO-engine replays — the only readable schema. The input stream is
/// one continuous run of pair ticks from session start (a ROUND_START
/// on the first record and on each later round's first record),
/// replayed by rebooting and re-priming an [`mgba_siolink::Pair`] and
/// feeding it the (p1, p2) stream verbatim. Trap-engine recordings
/// (schema 0x1B and older) are not supported — the engine that played
/// them is gone, and [`decode_metadata`] rejects them.
pub const VERSION: u8 = 0x1C;

/// Per-input record encoding. GBA joyflags use 10 bits, so each side packs
/// into 2 high bits in the tag's payload nibble plus 1 low byte that
/// follows. Most frames are idle or repeat the previous frame, so the tag
/// byte alone usually suffices.
///
/// Tag byte layout:
///   bit 7 (op): 0 = "default value is zero", 1 = "default value is previous record's value"
///   bit 6:      1 = p1 takes the default (no p1 byte follows), 0 = p1 explicit
///   bit 5:      1 = p2 takes the default (no p2 byte follows), 0 = p2 explicit
///   bit 4:      ROUND_START flag (overlay; resets `prev` to (0,0) for this record)
///   bits 0..=1: high 2 bits of explicit p1
///   bits 2..=3: high 2 bits of explicit p2
///
/// The all-zero tag byte (0x00) is the end-of-replay sentinel. The op bit
/// is informationally redundant when both sides are explicit, so the
/// encoder always sets it (op=1) in that case to keep the tag byte clear
/// of the EOR sentinel.
const OP_PREV: u8 = 0b1000_0000;
const P1_DEFAULT: u8 = 0b0100_0000;
const P2_DEFAULT: u8 = 0b0010_0000;
const ROUND_START_FLAG: u8 = 0b0001_0000;
const END_OF_REPLAY: u8 = 0x00;

pub struct Writer {
    writer: Box<dyn Write + Send>,
    /// True once a round is open. The next [`write_input`] sets the
    /// `ROUND_START_FLAG` bit on its tag byte and clears this.
    next_input_is_round_start: bool,
    /// Last (p1, p2) joyflags emitted, used by the "default = previous"
    /// tag form. Reset to (0, 0) on every [`start_round`].
    prev: (u16, u16),
}

#[derive(Clone)]
pub struct Replay {
    pub is_complete: bool,
    pub metadata: Metadata,
    pub is_offerer: bool,
    pub local_player_index: u8,
    pub rng_seed: [u8; 16],
    /// Each side's SRAM dump as
    /// [`tango_dataview::save::Save::to_sram_dump`] produces it — ready
    /// to hand to `mgba::core::Core::load_save` without further
    /// conversion. Replays prior to schema version 0x1B stored raw
    /// WRAM here and reassembled SRAM on read.
    pub local_sram: Vec<u8>,
    pub remote_sram: Vec<u8>,
    pub rounds: Vec<Vec<(crate::input::PartialInput, crate::input::PartialInput)>>,
}

pub fn decode_metadata(version: u8, raw: &[u8]) -> Result<Metadata, std::io::Error> {
    Ok(match version {
        VERSION => protos::replay11::Metadata::decode(raw)?,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported replay version: {:02x}", version),
            ));
        }
    })
}

pub fn read_metadata(r: &mut impl std::io::Read) -> Result<(u8, Metadata), std::io::Error> {
    let mut header = [0u8; 4];
    r.read_exact(&mut header)?;
    if header != HEADER {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid header"));
    }

    let version = r.read_u8()?;
    let metadata_len = r.read_u32::<byteorder::LittleEndian>()?;
    let mut raw = vec![0u8; metadata_len as usize];
    r.read_exact(&mut raw[..])?;
    Ok((version, decode_metadata(version, &raw)?))
}

// The local and remote SRAM dumps are stored as two zstd frames
// concatenated directly in the stream — no length prefixes.
// `single_frame` + BufRead's exact-consumption semantics leave the reader
// positioned right after the frame's end marker, so the next zstd frame
// (and the joyflag records that follow it) are read straight from the
// same reader.
fn read_zstd_frame(r: &mut impl std::io::BufRead) -> std::io::Result<Vec<u8>> {
    let mut decoder = zstd::stream::read::Decoder::with_buffer(r)?.single_frame();
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn write_zstd_frame(w: impl Write, data: &[u8]) -> std::io::Result<()> {
    let mut encoder = zstd::stream::write::Encoder::new(w, 3)?;
    encoder.write_all(data)?;
    encoder.finish()?;
    Ok(())
}

impl Replay {
    /// The cart-RTC time playback cores must be pinned to (via
    /// `Core::set_rtc_fixed`, before `reset()`): the match clock in
    /// `metadata.ts`, milliseconds since the unix epoch. Live PvP pins every
    /// core to the negotiated match clock and records that same value as
    /// `metadata.ts`, so playback reproduces the live match's RTC reads
    /// exactly — without the pin, RTC-reading games (exe45) diverge.
    pub fn rtc_time(&self) -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(self.metadata.ts)
    }

    pub fn into_remote(mut self) -> Self {
        std::mem::swap(&mut self.metadata.local_side, &mut self.metadata.remote_side);
        self.local_player_index = 1 - self.local_player_index;
        self.is_offerer = !self.is_offerer;
        std::mem::swap(&mut self.local_sram, &mut self.remote_sram);
        for round in self.rounds.iter_mut() {
            for (local, remote) in round.iter_mut() {
                std::mem::swap(local, remote);
            }
        }
        self
    }

    pub fn total_input_pairs(&self) -> usize {
        self.rounds.iter().map(|r| r.len()).sum()
    }

    pub fn decode(r: impl std::io::Read) -> std::io::Result<Self> {
        let mut r = std::io::BufReader::new(r);
        // Rejects anything but VERSION.
        let (_version, metadata) = read_metadata(&mut r)?;

        let is_offerer = r.read_u8()? != 0;
        let local_player_index = r.read_u8()?;

        let mut rng_seed = [0u8; 16];
        r.read_exact(&mut rng_seed)?;

        let local_sram = read_zstd_frame(&mut r)?;
        let remote_sram = read_zstd_frame(&mut r)?;

        // Streaming round decode: see the tag-byte layout doc near
        // `OP_PREV` for the per-record encoding. `0x00` ends the stream
        // cleanly; any unexpected EOF mid-record drops the partial record
        // and leaves is_complete=false.
        let mut rounds: Vec<Vec<(crate::input::PartialInput, crate::input::PartialInput)>> = Vec::new();
        let mut current_round: Vec<(crate::input::PartialInput, crate::input::PartialInput)> = Vec::new();
        let mut is_complete = false;
        let mut prev: (u16, u16) = (0, 0);

        while let Ok(tag) = r.read_u8() {
            if tag == END_OF_REPLAY {
                is_complete = true;
                break;
            }

            if tag & ROUND_START_FLAG != 0 {
                if !current_round.is_empty() {
                    rounds.push(std::mem::take(&mut current_round));
                }
                prev = (0, 0);
            }

            let op_prev = tag & OP_PREV != 0;
            let p1_default = tag & P1_DEFAULT != 0;
            let p2_default = tag & P2_DEFAULT != 0;

            let p1 = if p1_default {
                if op_prev {
                    prev.0
                } else {
                    0
                }
            } else {
                let high = (tag & 0b11) as u16;
                let Ok(low) = r.read_u8() else { break };
                (high << 8) | low as u16
            };
            let p2 = if p2_default {
                if op_prev {
                    prev.1
                } else {
                    0
                }
            } else {
                let high = ((tag >> 2) & 0b11) as u16;
                let Ok(low) = r.read_u8() else { break };
                (high << 8) | low as u16
            };

            prev = (p1, p2);

            let p1_input = crate::input::PartialInput { joyflags: p1 };
            let p2_input = crate::input::PartialInput { joyflags: p2 };
            let (local, remote) = if local_player_index == 0 {
                (p1_input, p2_input)
            } else {
                (p2_input, p1_input)
            };
            current_round.push((local, remote));
        }

        if !current_round.is_empty() {
            rounds.push(current_round);
        }

        Ok(Self {
            is_complete,
            metadata,
            is_offerer,
            local_player_index,
            rng_seed,
            local_sram,
            remote_sram,
            rounds,
        })
    }
}

impl Writer {
    /// `version` is the container schema to stamp — [`VERSION`] is
    /// the only one readers accept.
    pub fn new(
        mut writer: impl Write + Send + 'static,
        version: u8,
        metadata: Metadata,
        is_offerer: bool,
        local_player_index: u8,
        rng_seed: [u8; 16],
        local_sram: &[u8],
        remote_sram: &[u8],
    ) -> std::io::Result<Self> {
        writer.write_all(HEADER)?;
        writer.write_u8(version)?;
        let raw_metadata = metadata.encode_to_vec();
        writer.write_u32::<byteorder::LittleEndian>(raw_metadata.len() as u32)?;
        writer.write_all(&raw_metadata[..])?;

        let mut writer = Box::new(writer) as Box<dyn Write + Send>;
        writer.write_u8(if is_offerer { 1 } else { 0 })?;
        writer.write_u8(local_player_index)?;
        writer.write_all(&rng_seed)?;
        write_zstd_frame(&mut *writer, local_sram)?;
        write_zstd_frame(&mut *writer, remote_sram)?;
        writer.flush()?;
        Ok(Writer {
            writer,
            next_input_is_round_start: false,
            prev: (0, 0),
        })
    }

    pub fn start_round(&mut self) -> std::io::Result<()> {
        // The marker is stamped on the next [`write_input`]'s tag byte; we
        // don't emit anything here, so a crash mid-round just leaves the
        // partial inputs on disk and the recovery path will treat the next
        // ROUND_START as starting a fresh round.
        self.next_input_is_round_start = true;
        self.prev = (0, 0);
        Ok(())
    }

    pub fn write_input(
        &mut self,
        local_player_index: u8,
        ip: &(crate::input::PartialInput, crate::input::PartialInput),
    ) -> std::io::Result<()> {
        let (local, remote) = ip;
        let (p1, p2) = if local_player_index == 0 {
            (local.joyflags, remote.joyflags)
        } else {
            (remote.joyflags, local.joyflags)
        };

        // Pick whichever op (default = zero vs default = previous) lets
        // more sides "take the default" — fewer explicit sides = smaller
        // record. Tie-break toward op=0 so the canonical idle frame stays
        // compact and matches new readers' expectations.
        let op0_p1_default = p1 == 0;
        let op0_p2_default = p2 == 0;
        let op1_p1_default = p1 == self.prev.0;
        let op1_p2_default = p2 == self.prev.1;
        let op0_explicit_count = (!op0_p1_default as u32) + (!op0_p2_default as u32);
        let op1_explicit_count = (!op1_p1_default as u32) + (!op1_p2_default as u32);

        let (mut op_prev, p1_default, p2_default) = if op1_explicit_count < op0_explicit_count {
            (true, op1_p1_default, op1_p2_default)
        } else {
            (false, op0_p1_default, op0_p2_default)
        };

        // The op bit is informationally redundant when both sides are
        // explicit; force it high so the tag byte never collides with the
        // 0x00 EOR sentinel (which it could if both high-bit pairs and the
        // round-start bit are all zero).
        if !p1_default && !p2_default {
            op_prev = true;
        }

        let mut tag: u8 = 0;
        if op_prev {
            tag |= OP_PREV;
        }
        if p1_default {
            tag |= P1_DEFAULT;
        } else {
            tag |= ((p1 >> 8) & 0b11) as u8;
        }
        if p2_default {
            tag |= P2_DEFAULT;
        } else {
            tag |= (((p2 >> 8) & 0b11) as u8) << 2;
        }
        if self.next_input_is_round_start {
            tag |= ROUND_START_FLAG;
            self.next_input_is_round_start = false;
        }

        self.writer.write_u8(tag)?;
        if !p1_default {
            self.writer.write_u8((p1 & 0xff) as u8)?;
        }
        if !p2_default {
            self.writer.write_u8((p2 & 0xff) as u8)?;
        }

        self.prev = (p1, p2);
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        self.writer.write_u8(END_OF_REPLAY)?;
        self.writer.flush()?;
        Ok(())
    }
}
