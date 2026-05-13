pub mod export;
mod protos;

use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use prost::Message;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;

pub use protos::replay11::metadata;
pub type Metadata = protos::replay11::Metadata;

pub const HEADER: &[u8] = b"TOOT";
pub const VERSION: u8 = 0x17;

/// Single byte written after the last round's zstd frame to mark a clean
/// shutdown. zstd frames always start with magic `0x28` (standard) or
/// `0x50..=0x5F` (skippable), so `0x00` cannot collide with a frame start.
const END_OF_REPLAY_SENTINEL: u8 = 0x00;

/// Internal state machine: between rounds the underlying writer sits in
/// `Idle`; during a round the writer is owned by the zstd encoder so frame
/// bytes flow directly to disk with no intermediate buffer.
enum WriterState {
    Idle(Box<dyn Write + Send>),
    InRound(zstd::stream::write::Encoder<'static, Box<dyn Write + Send>>),
    /// Transient — only observable while moving between the above two.
    Empty,
}

pub struct Writer {
    state: WriterState,
    raw_input_size: u8,
}

#[derive(Clone)]
pub struct Replay {
    pub is_complete: bool,
    pub metadata: Metadata,
    pub is_offerer: bool,
    pub local_player_index: u8,
    pub rng_seed: [u8; 16],
    pub local_sram: Vec<u8>,
    pub remote_sram: Vec<u8>,
    pub rounds: Vec<Vec<crate::input::Pair<crate::input::Input, crate::input::Input>>>,
}

pub fn decode_metadata(version: u8, raw: &[u8]) -> Result<Metadata, std::io::Error> {
    Ok(match version {
        VERSION => protos::replay11::Metadata::decode(raw)?,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid version: {:02x}", version),
            ));
        }
    })
}

fn read_compressed_blob(r: &mut impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let len = r.read_u32::<byteorder::LittleEndian>()? as usize;
    let mut compressed = vec![0u8; len];
    r.read_exact(&mut compressed)?;
    zstd::stream::decode_all(&compressed[..])
}

fn write_compressed_blob(w: &mut impl Write, data: &[u8]) -> std::io::Result<()> {
    let compressed = zstd::stream::encode_all(data, 3)?;
    w.write_u32::<byteorder::LittleEndian>(compressed.len() as u32)?;
    w.write_all(&compressed)
}

pub fn read_metadata(r: &mut impl std::io::Read) -> Result<Metadata, std::io::Error> {
    let mut header = [0u8; 4];
    r.read_exact(&mut header)?;
    if header != HEADER {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid header"));
    }

    let version = r.read_u8()?;
    let metadata_len = r.read_u32::<byteorder::LittleEndian>()?;
    let mut raw = vec![0u8; metadata_len as usize];
    r.read_exact(&mut raw[..])?;
    decode_metadata(version, &raw)
}

impl Replay {
    pub fn into_remote(mut self) -> Self {
        std::mem::swap(&mut self.metadata.local_side, &mut self.metadata.remote_side);
        self.local_player_index = 1 - self.local_player_index;
        self.is_offerer = !self.is_offerer;
        std::mem::swap(&mut self.local_sram, &mut self.remote_sram);
        for round in self.rounds.iter_mut() {
            for ip in round.iter_mut() {
                std::mem::swap(&mut ip.local, &mut ip.remote);
            }
        }
        self
    }

    pub fn total_input_pairs(&self) -> usize {
        self.rounds.iter().map(|r| r.len()).sum()
    }

    pub fn decode(mut r: impl std::io::Read) -> std::io::Result<Self> {
        let metadata = read_metadata(&mut r)?;

        let is_offerer = r.read_u8()? != 0;
        let local_player_index = r.read_u8()?;
        let raw_input_size = r.read_u8()? as usize;

        let mut rng_seed = [0u8; 16];
        r.read_exact(&mut rng_seed)?;

        let local_sram = read_compressed_blob(&mut r)?;
        let remote_sram = read_compressed_blob(&mut r)?;

        // Each round is a self-delimiting zstd frame written directly to
        // the file; rounds are concatenated back-to-back. After the last
        // frame, a single END_OF_REPLAY_SENTINEL byte marks clean shutdown.
        // Mid-frame EOF or a missing sentinel means the writer crashed —
        // the in-progress round is dropped, completed rounds are kept.
        let record_size = 4 + 2 * raw_input_size;
        let mut rounds: Vec<Vec<crate::input::Pair<crate::input::Input, crate::input::Input>>> = Vec::new();
        let mut is_complete = false;

        let mut br = std::io::BufReader::new(r);
        loop {
            let buf = br.fill_buf()?;
            let Some(&first) = buf.first() else {
                break;
            };
            if first == END_OF_REPLAY_SENTINEL {
                br.consume(1);
                is_complete = true;
                break;
            }

            let mut decoder = zstd::stream::read::Decoder::with_buffer(br)?.single_frame();
            let mut decompressed = Vec::new();
            if decoder.read_to_end(&mut decompressed).is_err() {
                break;
            }
            br = decoder.finish();

            if decompressed.len() % record_size != 0 {
                break;
            }
            let mut round = Vec::with_capacity(decompressed.len() / record_size);
            for chunk in decompressed.chunks_exact(record_size) {
                let p1_jf = u16::from_le_bytes([chunk[0], chunk[1]]);
                let p1_pkt = chunk[2..2 + raw_input_size].to_vec();
                let p2_jf_off = 2 + raw_input_size;
                let p2_jf = u16::from_le_bytes([chunk[p2_jf_off], chunk[p2_jf_off + 1]]);
                let p2_pkt = chunk[p2_jf_off + 2..].to_vec();
                let p1_input = crate::input::Input {
                    joyflags: p1_jf,
                    packet: p1_pkt,
                };
                let p2_input = crate::input::Input {
                    joyflags: p2_jf,
                    packet: p2_pkt,
                };
                let (local, remote) = if local_player_index == 0 {
                    (p1_input, p2_input)
                } else {
                    (p2_input, p1_input)
                };
                round.push(crate::input::Pair { local, remote });
            }
            rounds.push(round);
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
    pub fn new(
        mut writer: impl Write + Send + 'static,
        metadata: Metadata,
        is_offerer: bool,
        local_player_index: u8,
        raw_input_size: u8,
        rng_seed: [u8; 16],
        local_sram: &[u8],
        remote_sram: &[u8],
    ) -> std::io::Result<Self> {
        writer.write_all(HEADER)?;
        writer.write_u8(VERSION)?;
        let raw_metadata = metadata.encode_to_vec();
        writer.write_u32::<byteorder::LittleEndian>(raw_metadata.len() as u32)?;
        writer.write_all(&raw_metadata[..])?;

        let mut writer = Box::new(writer) as Box<dyn Write + Send>;
        writer.write_u8(if is_offerer { 1 } else { 0 })?;
        writer.write_u8(local_player_index)?;
        writer.write_u8(raw_input_size)?;
        writer.write_all(&rng_seed)?;
        write_compressed_blob(&mut writer, local_sram)?;
        write_compressed_blob(&mut writer, remote_sram)?;
        writer.flush()?;
        Ok(Writer {
            state: WriterState::Idle(writer),
            raw_input_size,
        })
    }

    pub fn start_round(&mut self) -> std::io::Result<()> {
        let mut writer = match std::mem::replace(&mut self.state, WriterState::Empty) {
            WriterState::InRound(enc) => enc.finish()?,
            WriterState::Idle(w) => w,
            WriterState::Empty => unreachable!(),
        };
        writer.flush()?;
        self.state = WriterState::InRound(zstd::Encoder::new(writer, 3)?);
        Ok(())
    }

    pub fn write_input(
        &mut self,
        local_player_index: u8,
        ip: &crate::input::Pair<crate::input::Input, crate::input::Input>,
    ) -> std::io::Result<()> {
        let WriterState::InRound(enc) = &mut self.state else {
            panic!("write_input called before start_round");
        };

        let (p1, p2) = if local_player_index == 0 {
            (&ip.local, &ip.remote)
        } else {
            (&ip.remote, &ip.local)
        };

        if p1.packet.len() != self.raw_input_size as usize || p2.packet.len() != self.raw_input_size as usize {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "packet size mismatch",
            ));
        }

        enc.write_u16::<byteorder::LittleEndian>(p1.joyflags)?;
        enc.write_all(&p1.packet)?;
        enc.write_u16::<byteorder::LittleEndian>(p2.joyflags)?;
        enc.write_all(&p2.packet)?;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        let mut writer = match std::mem::replace(&mut self.state, WriterState::Empty) {
            WriterState::InRound(enc) => enc.finish()?,
            WriterState::Idle(w) => w,
            WriterState::Empty => unreachable!(),
        };
        writer.write_u8(END_OF_REPLAY_SENTINEL)?;
        writer.flush()?;
        Ok(())
    }
}
