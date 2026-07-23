mod protos;

use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use prost::Message;
use std::io::{Read, Write};

pub use protos::replay11::metadata;
pub type Metadata = protos::replay11::Metadata;

pub const HEADER: &[u8] = b"TOOT";
/// SIO-engine replays — the only readable schema. The input stream is
/// one continuous run of pair ticks from session start, replayed by
/// rebooting and re-priming an [`mgba_rollback::Link`] and feeding it
/// the (p1, p2) stream verbatim. Trap-engine recordings (schema 0x1B
/// and older) are not supported — the engine that played them is gone,
/// and [`decode_metadata`] rejects them.
///
/// After the header (magic, version, metadata, offerer flag, player
/// index, rng seed, two zstd SRAM frames), the rest of the file is an
/// [`mgba_rollback::replay`] input stream: this container only frames
/// it. Round boundaries are the stream's marks — one stamped on each
/// round's first record.
pub const VERSION: u8 = 0x1C;

pub struct Writer {
    /// Everything after the header framing is the shared stream
    /// encoding; rounds are its marks.
    stream: mgba_rollback::replay::Writer<Box<dyn Write + Send>>,
}

#[derive(Clone)]
pub struct Replay {
    pub is_complete: bool,
    pub metadata: Metadata,
    pub is_offerer: bool,
    pub local_player_index: u8,
    pub rng_seed: [u8; 16],
    /// Each side's SRAM dump as
    /// `tango_dataview::save::Save::to_sram_dump` produces it — ready
    /// to hand to `mgba::core::Core::load_save` without further
    /// conversion. Replays prior to schema version 0x1B stored raw
    /// WRAM here and reassembled SRAM on read.
    pub local_sram: Vec<u8>,
    pub remote_sram: Vec<u8>,
    /// One continuous run of (local, remote) pair ticks from session
    /// start — the stream as recorded, not segmented.
    pub inputs: Vec<(crate::input::Input, crate::input::Input)>,
    /// Indices into `inputs` where a round starts (records carrying a
    /// stream mark). The first entry is always 0 when `inputs` is
    /// non-empty — recordings that predate the markers decode as one
    /// round — so consecutive entries (and the stream end) delimit the
    /// rounds.
    pub round_starts: Vec<usize>,
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
        for (local, remote) in self.inputs.iter_mut() {
            std::mem::swap(local, remote);
        }
        self
    }

    /// The rounds as spans of `inputs`: each round runs from its
    /// round-start mark to the next (the last to the end of the stream).
    pub fn round_ranges(&self) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
        let ends = self
            .round_starts
            .iter()
            .skip(1)
            .copied()
            .chain(std::iter::once(self.inputs.len()));
        self.round_starts
            .iter()
            .copied()
            .zip(ends)
            .map(|(start, end)| start..end)
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

        // The rest of the file is the shared stream encoding; a
        // truncated tail comes back as is_complete = false with the
        // partial record dropped.
        let stream = mgba_rollback::replay::Stream::read(&mut r)?;

        let inputs: Vec<(crate::input::Input, crate::input::Input)> = stream
            .inputs
            .into_iter()
            .map(|[p1, p2]| {
                let p1_input = crate::input::Input { joyflags: p1 };
                let p2_input = crate::input::Input { joyflags: p2 };
                if local_player_index == 0 {
                    (p1_input, p2_input)
                } else {
                    (p2_input, p1_input)
                }
            })
            .collect();

        // A leading unmarked run — recordings that predate the markers,
        // or a crash-recovered partial round — still counts as a round.
        let mut round_starts = stream.marks;
        if !inputs.is_empty() && round_starts.first() != Some(&0) {
            round_starts.insert(0, 0);
        }

        Ok(Self {
            is_complete: stream.is_complete,
            metadata,
            is_offerer,
            local_player_index,
            rng_seed,
            local_sram,
            remote_sram,
            inputs,
            round_starts,
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
            stream: mgba_rollback::replay::Writer::new(writer),
        })
    }

    pub fn start_round(&mut self) -> std::io::Result<()> {
        // The mark is stamped on the next [`write_input`]'s tag byte; we
        // don't emit anything here, so a crash mid-round just leaves the
        // partial inputs on disk and the recovery path will treat the next
        // mark as starting a fresh round.
        self.stream.mark();
        Ok(())
    }

    pub fn write_input(
        &mut self,
        local_player_index: u8,
        ip: &(crate::input::Input, crate::input::Input),
    ) -> std::io::Result<()> {
        let (local, remote) = ip;
        let (p1, p2) = if local_player_index == 0 {
            (local.joyflags, remote.joyflags)
        } else {
            (remote.joyflags, local.joyflags)
        };
        self.stream.push([p1, p2])
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

    #[test]
    fn roundtrips_rounds_and_both_sides() {
        // joyflags with high bits set exercise the explicit tag form;
        // the repeated pair the previous-tick default.
        let ticks: Vec<(u16, u16)> = vec![(0, 0), (0x041, 0x082), (0x041, 0x082), (0x3ff, 0x155), (0, 0x300)];
        let round_starts = [0usize, 3];

        for local_player_index in [0u8, 1] {
            let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let mut w = Writer::new(
                SharedVec(buf.clone()),
                VERSION,
                Metadata {
                    ts: 1_752_000_000_000,
                    ..Default::default()
                },
                true,
                local_player_index,
                [7u8; 16],
                &[1, 2, 3],
                &[4, 5],
            )
            .unwrap();
            for (i, &(p1, p2)) in ticks.iter().enumerate() {
                if round_starts.contains(&i) {
                    w.start_round().unwrap();
                }
                let (local, remote) = if local_player_index == 0 { (p1, p2) } else { (p2, p1) };
                w.write_input(
                    local_player_index,
                    &(
                        crate::input::Input { joyflags: local },
                        crate::input::Input { joyflags: remote },
                    ),
                )
                .unwrap();
            }
            w.finish().unwrap();

            let bytes = buf.lock().unwrap().clone();
            let replay = Replay::decode(&bytes[..]).unwrap();
            assert!(replay.is_complete);
            assert!(replay.is_offerer);
            assert_eq!(replay.local_player_index, local_player_index);
            assert_eq!(replay.rng_seed, [7u8; 16]);
            assert_eq!(replay.local_sram, vec![1, 2, 3]);
            assert_eq!(replay.remote_sram, vec![4, 5]);
            assert_eq!(replay.metadata.ts, 1_752_000_000_000);
            assert_eq!(replay.round_starts, round_starts);
            let expected: Vec<(u16, u16)> = ticks
                .iter()
                .map(|&(p1, p2)| if local_player_index == 0 { (p1, p2) } else { (p2, p1) })
                .collect();
            assert_eq!(
                replay
                    .inputs
                    .iter()
                    .map(|(l, r)| (l.joyflags, r.joyflags))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}
