pub mod export;
mod protos;

use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use prost::Message;
use std::io::Read;
use std::io::Write;
pub trait ReadWriteSeek: std::io::Read + std::io::Write + std::io::Seek {}
impl<T: std::io::Read + std::io::Write + std::io::Seek> ReadWriteSeek for T {}

pub use protos::replay11::metadata;
pub type Metadata = protos::replay11::Metadata;

pub struct Writer {
    encoder: Option<zstd::stream::write::Encoder<'static, Box<dyn ReadWriteSeek + Send>>>,
    num_inputs: u32,
}

pub const HEADER: &[u8] = b"TOOT";
pub const VERSION: u8 = 0x12;

#[derive(Clone)]
pub struct Replay {
    pub is_complete: bool,
    pub metadata: Metadata,
    pub local_player_index: u8,
    pub local_state: Box<mgba::state::State>,
    pub remote_state: Box<mgba::state::State>,
    pub input_pairs: Vec<crate::input::Pair<crate::input::Input, crate::input::Input>>,
}

pub fn decode_metadata(version: u8, raw: &[u8]) -> Result<Metadata, std::io::Error> {
    Ok(match version {
        VERSION => protos::replay11::Metadata::decode(&raw[..])?,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid version: {:02x}", version),
            ));
        }
    })
}

pub fn read_metadata(r: &mut impl std::io::Read) -> Result<(usize, Metadata), std::io::Error> {
    let mut header = [0u8; 4];
    r.read_exact(&mut header)?;
    if &header != HEADER {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid header"));
    }

    let version = r.read_u8()?;
    let num_inputs = r.read_u32::<byteorder::LittleEndian>()? as usize;
    let metadata_len = r.read_u32::<byteorder::LittleEndian>()?;
    let mut raw = vec![0u8; metadata_len as usize];
    r.read_exact(&mut raw[..])?;
    Ok((num_inputs, decode_metadata(version, &raw)?))
}

impl Replay {
    pub fn into_remote(mut self) -> Self {
        std::mem::swap(&mut self.metadata.local_side, &mut self.metadata.remote_side);
        self.local_player_index = 1 - self.local_player_index;
        std::mem::swap(&mut self.local_state, &mut self.remote_state);
        for ip in self.input_pairs.iter_mut() {
            std::mem::swap(&mut ip.local, &mut ip.remote);
        }
        self
    }

    pub fn decode(mut r: impl std::io::Read) -> std::io::Result<Self> {
        let (num_inputs, metadata) = read_metadata(&mut r)?;

        let mut zr = zstd::stream::read::Decoder::new(r)?;

        let local_player_index = zr.read_u8()?;

        let input_raw_size = zr.read_u8()? as usize;

        let mut local_state = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
        zr.read_exact(&mut local_state)?;
        let local_state = mgba::state::State::from_slice(&local_state);

        let mut remote_state = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
        zr.read_exact(&mut remote_state)?;
        let remote_state = mgba::state::State::from_slice(&remote_state);

        let mut input_pairs = vec![];

        loop {
            let local_tick = if let Ok(v) = zr.read_u32::<byteorder::LittleEndian>() {
                v
            } else {
                break;
            };
            let remote_tick = if let Ok(v) = zr.read_u32::<byteorder::LittleEndian>() {
                v
            } else {
                break;
            };
            let dt = std::time::Duration::from_millis(if let Ok(v) = zr.read_u16::<byteorder::LittleEndian>() {
                v
            } else {
                break;
            } as u64);

            let mut p1_input = crate::input::Input {
                local_tick,
                remote_tick,
                joyflags: if let Ok(v) = zr.read_u16::<byteorder::LittleEndian>() {
                    v
                } else {
                    break;
                },
                packet: vec![0u8; input_raw_size],
                dt,
            };
            if zr.read_exact(&mut p1_input.packet).is_err() {
                break;
            }

            let mut p2_input = crate::input::Input {
                local_tick,
                remote_tick: local_tick,
                joyflags: if let Ok(v) = zr.read_u16::<byteorder::LittleEndian>() {
                    v
                } else {
                    break;
                },
                packet: vec![0u8; input_raw_size],
                dt,
            };
            if zr.read_exact(&mut p2_input.packet).is_err() {
                break;
            }

            let (local, remote) = if local_player_index == 0 {
                (p1_input, p2_input)
            } else {
                (p2_input, p1_input)
            };

            input_pairs.push(crate::input::Pair { local, remote });
        }

        Ok(Self {
            is_complete: num_inputs > 0 && num_inputs as usize == input_pairs.len(),
            metadata,
            local_player_index,
            local_state,
            remote_state,
            input_pairs,
        })
    }
}

impl Writer {
    pub fn new(
        mut writer: impl ReadWriteSeek + Send + 'static,
        metadata: Metadata,
        local_player_index: u8,
        raw_input_size: u8,
    ) -> std::io::Result<Self> {
        writer.write_all(HEADER)?;
        writer.write_u8(VERSION)?;
        writer.write_u32::<byteorder::LittleEndian>(0)?;
        let raw_metadata = metadata.encode_to_vec();
        writer.write_u32::<byteorder::LittleEndian>(raw_metadata.len() as u32)?;
        writer.write_all(&raw_metadata[..])?;
        let mut encoder = zstd::Encoder::new(Box::new(writer) as Box<dyn ReadWriteSeek + Send>, 3)?;
        encoder.write_u8(local_player_index)?;
        encoder.write_u8(raw_input_size)?;
        encoder.flush()?;
        Ok(Writer {
            encoder: Some(encoder),
            num_inputs: 0,
        })
    }

    pub fn write_state(&mut self, state: &mgba::state::State) -> std::io::Result<()> {
        self.encoder
            .as_mut()
            .unwrap()
            .write_u32::<byteorder::LittleEndian>(state.as_slice().len() as u32)?;
        self.encoder.as_mut().unwrap().write_all(state.as_slice())?;
        self.encoder.as_mut().unwrap().flush()?;
        Ok(())
    }

    pub fn write_input(
        &mut self,
        local_player_index: u8,
        ip: &crate::input::Pair<crate::input::Input, crate::input::Input>,
    ) -> std::io::Result<()> {
        self.encoder
            .as_mut()
            .unwrap()
            .write_u32::<byteorder::LittleEndian>(ip.local.local_tick)?;
        self.encoder
            .as_mut()
            .unwrap()
            .write_u32::<byteorder::LittleEndian>(ip.local.remote_tick)?;

        self.encoder
            .as_mut()
            .unwrap()
            .write_u16::<byteorder::LittleEndian>(ip.local.dt.as_millis() as u16)?;

        let (p1, p2) = if local_player_index == 0 {
            (&ip.local, &ip.remote)
        } else {
            (&ip.remote, &ip.local)
        };

        self.encoder
            .as_mut()
            .unwrap()
            .write_u16::<byteorder::LittleEndian>(p1.joyflags)?;
        self.encoder.as_mut().unwrap().write_all(&p1.packet)?;
        self.encoder
            .as_mut()
            .unwrap()
            .write_u16::<byteorder::LittleEndian>(p2.joyflags)?;
        self.encoder.as_mut().unwrap().write_all(&p2.packet)?;

        self.num_inputs += 1;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<Box<dyn ReadWriteSeek + Send>> {
        let mut w = self.encoder.take().unwrap().finish()?;
        w.seek(std::io::SeekFrom::Start((HEADER.len() + 1) as u64))?;
        w.write_u32::<byteorder::LittleEndian>(self.num_inputs)?;
        Ok(w)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if let Some(encoder) = self.encoder.take() {
            encoder.finish().expect("finish");
        }
    }
}
