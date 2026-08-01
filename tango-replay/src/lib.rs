//! Tango's replay file format, in two layers kept deliberately
//! distinct:
//!
//! - the *container* (this crate's root): the file framing — magic,
//!   schema version, perspective byte, metadata proto, rng seed, SRAM
//!   frames — everything needed to boot a pair of cores into the
//!   recorded match.
//!
//! - the *stream* ([`stream`]): the per-tick input-pair codec that
//!   makes up the rest of the file. It knows nothing about the framing.
//!
//! A recording is the inputs and the state they act on, and nothing
//! else. It used to also carry round boundaries, stamped in as the
//! match played; rounds are a fact about what the games did with those
//! inputs, which the games' own telemetry reports on re-simulation, so
//! the file no longer has an opinion about them.

mod protos;
pub mod stream;

use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use prost::Message;
use std::io::{Read, Write};

pub use protos::replay11::metadata;
pub type Metadata = protos::replay11::Metadata;

pub const HEADER: &[u8] = b"TOOT";

/// The file extension recordings carry, without the dot. Named here
/// rather than spelled out at each site so a recorder and a scanner
/// can't disagree about it.
pub const EXTENSION: &str = "tangoreplay";
/// SIO-engine replays. The input stream is one continuous run of pair
/// ticks from session start, replayed by rebooting and re-priming a
/// link and feeding it the (p1, p2) stream verbatim. Trap-engine
/// recordings (schema 0x1B and older) and the perspective-ordered 0x1C
/// are not supported.
///
/// Layout: magic, version, local_player_index, metadata (u32 length +
/// proto), rng seed, two zstd SRAM frames, then a [`stream`]-encoded
/// input stream. Everything but `local_player_index` is in absolute
/// player order — sides, SRAM frames, input columns — so the recorder's
/// seat is the file's ONE perspective-dependent byte: overwriting byte 5
/// yields the other player's recording of the same match.
///
/// 0x1E widened the stream's rows to the DS's inputs (12-bit pad plus
/// the stylus); the container around them is unchanged, so its 0x1D
/// predecessor stays readable ([`stream::Stream::read_v1`]) and only
/// the current schema is written.
///
/// Dropping the round marks did NOT bump this. A mark was a tag bit
/// with no bytes of its own, so a recording made while they were live
/// decodes byte-for-byte identically now that the bit is ignored, and a
/// recording made since reads on an older build as a match with one
/// round. A bump would instead have [`read_metadata`] reject every
/// replay already on disk.
pub const VERSION: u8 = 0x1E;

/// The touchless predecessor, still accepted by [`read_metadata`] and
/// [`Replay::decode`].
const VERSION_V1: u8 = 0x1D;

pub struct Writer {
    /// Everything after the header framing is the shared stream
    /// encoding.
    stream: stream::Writer<Box<dyn Write + Send>>,
}

#[derive(Clone)]
pub struct Replay {
    pub is_complete: bool,
    pub metadata: Metadata,
    /// The recorder's player slot — the file's one perspective bit.
    /// Everything else is absolute player order.
    pub local_player_index: u8,
    pub rng_seed: [u8; 16],
    /// Each player's SRAM dump as
    /// the save's `to_sram_dump` produces it — ready
    /// to hand to `mgba::core::Core::load_save` without further
    /// conversion.
    pub srams: [Vec<u8>; 2],
    /// One continuous run of (p1, p2) input pair ticks from session
    /// start — the stream as recorded, and the whole of what a
    /// recording says happened. Where the rounds fall in it is the
    /// telemetry's answer, arrived at by re-simulating these inputs.
    pub inputs: Vec<[stream::Input; 2]>,
}

impl Metadata {
    /// The side seated in the given player slot (0 = player 1).
    pub fn side(&self, player_index: u8) -> Option<&metadata::Side> {
        match player_index {
            0 => self.p1_side.as_ref(),
            _ => self.p2_side.as_ref(),
        }
    }
}

/// Ceiling on the declared metadata length. The proto is two sides'
/// nicknames plus their game info — hundreds of bytes, not megabytes —
/// so a length past this is a corrupt header rather than a big match,
/// and reading it as one would mean allocating whatever the file says.
const MAX_METADATA_LEN: u32 = 1024 * 1024;

fn unsupported_version(version: u8) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("unsupported replay version: {version:02x}"),
    )
}

pub fn decode_metadata(version: u8, raw: &[u8]) -> Result<Metadata, std::io::Error> {
    Ok(match version {
        VERSION | VERSION_V1 => protos::replay11::Metadata::decode(raw)?,
        _ => return Err(unsupported_version(version)),
    })
}

/// The cheap header read for listings: everything before the SRAM
/// frames. Returns (version, local_player_index, metadata).
pub fn read_metadata(r: &mut impl std::io::Read) -> Result<(u8, u8, Metadata), std::io::Error> {
    let mut header = [0u8; 4];
    r.read_exact(&mut header)?;
    if header != HEADER {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid header"));
    }

    let version = r.read_u8()?;
    // Everything past this byte is laid out per the schema, so it has
    // to be settled before any of it is read — not after, on the
    // decoded metadata. Pre-0x1D files carry no perspective byte, which
    // slides the length field by one: read anyway, it picks up the
    // proto's leading field tag as its high byte and asks for ~128 MiB
    // per file, which is what made scanning a library carried across
    // the 0x1D bump take minutes.
    if version != VERSION && version != VERSION_V1 {
        return Err(unsupported_version(version));
    }
    let local_player_index = r.read_u8()?;
    let metadata_len = r.read_u32::<byteorder::LittleEndian>()?;
    if metadata_len > MAX_METADATA_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("metadata length {metadata_len} exceeds the {MAX_METADATA_LEN}-byte limit"),
        ));
    }
    let mut raw = vec![0u8; metadata_len as usize];
    r.read_exact(&mut raw[..])?;
    Ok((version, local_player_index, decode_metadata(version, &raw)?))
}

// The two SRAM dumps are stored as two zstd frames concatenated
// directly in the stream — no length prefixes. `single_frame` +
// BufRead's exact-consumption semantics leave the reader positioned
// right after the frame's end marker, so the next zstd frame (and the
// joyflag records that follow it) are read straight from the same
// reader.
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

    /// The recorder's side of the metadata.
    pub fn local_side(&self) -> Option<&metadata::Side> {
        self.metadata.side(self.local_player_index)
    }

    /// The recorder's opponent's side of the metadata.
    pub fn remote_side(&self) -> Option<&metadata::Side> {
        self.metadata.side(1 - self.local_player_index)
    }

    pub fn decode(r: impl std::io::Read) -> std::io::Result<Self> {
        let mut r = std::io::BufReader::new(r);
        // Rejects anything but the readable schemas.
        let (version, local_player_index, metadata) = read_metadata(&mut r)?;

        let mut rng_seed = [0u8; 16];
        r.read_exact(&mut rng_seed)?;

        let srams = [read_zstd_frame(&mut r)?, read_zstd_frame(&mut r)?];

        // The rest of the file is the shared stream encoding; a
        // truncated tail comes back as is_complete = false with the
        // partial record dropped.
        let stream = if version == VERSION_V1 {
            stream::Stream::read_v1(&mut r)?
        } else {
            stream::Stream::read(&mut r)?
        };
        Ok(Self {
            is_complete: stream.is_complete,
            metadata,
            local_player_index,
            rng_seed,
            srams,
            inputs: stream.inputs,
        })
    }
}

impl Writer {
    /// `version` is the container schema to stamp — [`VERSION`] is
    /// the only one readers accept. Arguments follow the file layout;
    /// `metadata` sides and `srams` are in absolute player order.
    pub fn new(
        mut writer: impl Write + Send + 'static,
        version: u8,
        local_player_index: u8,
        metadata: Metadata,
        rng_seed: [u8; 16],
        srams: [&[u8]; 2],
    ) -> std::io::Result<Self> {
        writer.write_all(HEADER)?;
        writer.write_u8(version)?;
        writer.write_u8(local_player_index)?;
        let raw_metadata = metadata.encode_to_vec();
        writer.write_u32::<byteorder::LittleEndian>(raw_metadata.len() as u32)?;
        writer.write_all(&raw_metadata[..])?;

        let mut writer = Box::new(writer) as Box<dyn Write + Send>;
        writer.write_all(&rng_seed)?;
        for sram in srams {
            write_zstd_frame(&mut *writer, sram)?;
        }
        writer.flush()?;
        Ok(Writer {
            stream: stream::Writer::new(writer),
        })
    }

    /// Append one confirmed tick's (p1, p2) input pair — absolute
    /// player order, same as [`Replay::inputs`] comes back.
    pub fn write_input(&mut self, inputs: [stream::Input; 2]) -> std::io::Result<()> {
        self.stream.push(inputs)
    }

    pub fn finish(self) -> std::io::Result<()> {
        self.stream.finish()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Writer::new` wants ownership of an `impl Write + Send + 'static`,
    /// so a plain `Vec` can't be inspected afterwards; share the buffer.
    struct SharedVec(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for SharedVec {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn side(nickname: &str) -> Option<metadata::Side> {
        Some(metadata::Side {
            nickname: nickname.to_owned(),
            game_info: Some(metadata::GameInfo {
                rom_family: "bn6".to_owned(),
                sim_version: 7,
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    fn write_replay(local_player_index: u8) -> Vec<u8> {
        // Keys with high bits set exercise the explicit form, a stylus
        // sample the touch bytes, the repeated pair the previous-tick
        // default.
        let keys = stream::Input::keys;
        let ticks: Vec<[stream::Input; 2]> = vec![
            [keys(0), keys(0)],
            [keys(0x041), keys(0x082)],
            [keys(0x041), keys(0x082)],
            [
                keys(0xfff),
                stream::Input {
                    keys: 0x155,
                    touch: Some((128, 96)),
                },
            ],
            [keys(0), keys(0x300)],
        ];
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut w = Writer::new(
            SharedVec(buf.clone()),
            VERSION,
            local_player_index,
            Metadata {
                ts: 1_752_000_000_000,
                p1_side: side("alice"),
                p2_side: side("bob"),
                ..Default::default()
            },
            [7u8; 16],
            [&[1, 2, 3], &[4, 5]],
        )
        .unwrap();
        for &keys in ticks.iter() {
            w.write_input(keys).unwrap();
        }
        w.finish().unwrap();
        let bytes = buf.lock().unwrap().clone();
        bytes
    }

    #[test]
    fn roundtrips_inputs_and_header() {
        let replay = Replay::decode(&write_replay(1)[..]).unwrap();
        assert!(replay.is_complete);
        assert_eq!(replay.local_player_index, 1);
        assert_eq!(replay.rng_seed, [7u8; 16]);
        assert_eq!(replay.srams, [vec![1, 2, 3], vec![4, 5]]);
        assert_eq!(replay.metadata.ts, 1_752_000_000_000);
        let keys = stream::Input::keys;
        assert_eq!(
            replay.inputs,
            vec![
                [keys(0), keys(0)],
                [keys(0x041), keys(0x082)],
                [keys(0x041), keys(0x082)],
                [
                    keys(0xfff),
                    stream::Input {
                        keys: 0x155,
                        touch: Some((128, 96)),
                    },
                ],
                [keys(0), keys(0x300)],
            ]
        );
        assert_eq!(replay.local_side().unwrap().nickname, "bob");
        assert_eq!(replay.remote_side().unwrap().nickname, "alice");
        assert_eq!(replay.local_side().unwrap().game_info.as_ref().unwrap().sim_version, 7);
    }

    /// A 0x1D container — the touchless predecessor — still decodes:
    /// same framing, v1 stream body.
    #[test]
    fn v1_replay_still_decodes() {
        // Reuse the current writer for the framing (it is byte-identical
        // through the SRAM frames), then splice in the old version byte
        // and a hand-laid v1 stream.
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let w = Writer::new(
            SharedVec(buf.clone()),
            VERSION,
            0,
            Metadata {
                ts: 1_752_000_000_000,
                p1_side: side("alice"),
                p2_side: side("bob"),
                ..Default::default()
            },
            [7u8; 16],
            [&[1, 2, 3], &[4, 5]],
        )
        .unwrap();
        // Framing only — no v2 records; drop the writer without its
        // sentinel by taking the bytes as they stand.
        drop(w);
        let mut bytes = buf.lock().unwrap().clone();
        bytes[4] = 0x1D;
        // v1 records: an idle tick, then explicit both sides with the
        // high bits in the tag's low nibble (and the retired mark bit
        // set, which decode ignores), then the sentinel.
        bytes.extend_from_slice(&[0x40 | 0x20, 0x80 | 0x10 | 0b01 | (0b10 << 2), 0x55, 0xaa, 0x00]);

        let replay = Replay::decode(&bytes[..]).unwrap();
        assert!(replay.is_complete);
        let keys = stream::Input::keys;
        assert_eq!(replay.inputs, vec![[keys(0), keys(0)], [keys(0x155), keys(0x2aa)]]);
        assert_eq!(replay.srams, [vec![1, 2, 3], vec![4, 5]]);
    }

    #[test]
    fn perspective_flips_with_one_byte() {
        // The design invariant the layout exists for: byte 5 is the
        // file's only perspective-dependent content, so overwriting it
        // yields the other player's recording of the same match.
        let mut bytes = write_replay(0);
        let a = Replay::decode(&bytes[..]).unwrap();
        assert_eq!(bytes[5], 0);
        bytes[5] = 1;
        let b = Replay::decode(&bytes[..]).unwrap();

        assert_eq!(b.local_player_index, 1);
        assert_eq!(a.metadata, b.metadata);
        assert_eq!(a.srams, b.srams);
        assert_eq!(a.inputs, b.inputs);
        assert_eq!(a.local_side(), b.remote_side());
        assert_eq!(a.remote_side(), b.local_side());
    }

    /// A pre-0x1D file has no perspective byte, so its metadata length
    /// sits where ours reads `local_player_index` + the low three bytes
    /// of the length. Reading the length before checking the version
    /// picks up the proto's leading tag as its high byte — here 0x08,
    /// `ts`'s field tag, for a demand of 128 MiB out of a 20-byte file.
    /// The version check has to come first, and the whole header has to
    /// be rejected from the bytes alone.
    #[test]
    fn old_schema_is_rejected_before_the_length_is_trusted() {
        let metadata = Metadata {
            ts: 1_752_000_000_000,
            ..Default::default()
        }
        .encode_to_vec();
        assert_eq!(metadata[0], 0x08, "test rests on ts being the leading field");

        let mut old = Vec::new();
        old.extend_from_slice(HEADER);
        old.push(0x1C); // trap-era schema: version, then straight to the length
        old.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        old.extend_from_slice(&metadata);

        // The length the unchecked read would have believed.
        let would_have_read = u32::from_le_bytes(old[6..10].try_into().unwrap());
        assert!(would_have_read > 100 * 1024 * 1024, "{would_have_read}");

        let err = read_metadata(&mut &old[..]).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("unsupported replay version: 1c"), "{err}");
    }
}
