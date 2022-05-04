use crate::input;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use std::io::Read;
use std::io::Write;
pub trait WriteSeek: std::io::Write + std::io::Seek {}
impl<T: std::io::Write + std::io::Seek> WriteSeek for T {}

pub struct Writer {
    encoder: Option<zstd::stream::write::Encoder<'static, Box<dyn WriteSeek + Send>>>,
    num_inputs: u32,
}

const HEADER: &[u8] = b"TOOT";
const VERSION: u8 = 0x0f;

pub struct Replay {
    pub metadata: Vec<u8>,
    pub local_player_index: u8,
    pub local_state: mgba::state::State,
    pub remote_state: Option<mgba::state::State>,
    pub input_pairs: Vec<input::Pair<input::Input, input::Input>>,
}

impl Replay {
    pub fn into_remote(mut self) -> Option<Self> {
        let remote_state = match self.remote_state.take() {
            Some(remote_state) => remote_state,
            None => {
                return None;
            }
        };
        self.remote_state = Some(self.local_state);
        self.local_state = remote_state;
        self.local_player_index = 1 - self.local_player_index;
        for ip in self.input_pairs.iter_mut() {
            std::mem::swap(&mut ip.local, &mut ip.remote);
        }
        Some(self)
    }

    pub fn decode(mut r: impl std::io::Read) -> std::io::Result<Self> {
        let mut header = [0u8; 4];
        r.read_exact(&mut header)?;
        if &header != HEADER {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid header",
            ));
        }

        if r.read_u8()? != VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid version",
            ));
        }

        let num_inputs = r.read_u32::<byteorder::LittleEndian>()?;
        if num_inputs == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "replay was not finished",
            ));
        }

        let metadata_len = r.read_u32::<byteorder::LittleEndian>()?;
        let mut metadata = vec![0u8; metadata_len as usize];
        r.read_exact(&mut metadata[..])?;

        let mut zr = zstd::stream::read::Decoder::new(r)?;

        let local_player_index = zr.read_u8()?;

        let mut local_state = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
        zr.read_exact(&mut local_state)?;
        let local_state = mgba::state::State::from_slice(&local_state);

        // This is unused, for now.
        let mut remote_state = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
        zr.read_exact(&mut remote_state)?;
        let remote_state = if remote_state.len() > 0 {
            Some(mgba::state::State::from_slice(&remote_state))
        } else {
            None
        };

        let mut input_pairs = vec![];

        for _ in 0..num_inputs {
            let local_tick = zr.read_u32::<byteorder::LittleEndian>()?;
            let remote_tick = zr.read_u32::<byteorder::LittleEndian>()?;

            let p1_joyflags = zr.read_u16::<byteorder::LittleEndian>()?;
            let p2_joyflags = zr.read_u16::<byteorder::LittleEndian>()?;

            let p1_custom_screen_state = zr.read_u8()?;
            let p2_custom_screen_state = zr.read_u8()?;

            let mut p1_turn = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
            zr.read_exact(&mut p1_turn)?;

            let mut p2_turn = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
            zr.read_exact(&mut p2_turn)?;

            let p1_input = input::Input {
                local_tick,
                remote_tick,
                joyflags: p1_joyflags,
                custom_screen_state: p1_custom_screen_state,
                turn: p1_turn,
            };

            let p2_input = input::Input {
                local_tick,
                remote_tick: local_tick,
                joyflags: p2_joyflags,
                custom_screen_state: p2_custom_screen_state,
                turn: p2_turn,
            };

            let (local, remote) = if local_player_index == 0 {
                (p1_input, p2_input)
            } else {
                (p2_input, p1_input)
            };

            input_pairs.push(input::Pair { local, remote });
        }

        Ok(Self {
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
        mut writer: Box<dyn WriteSeek + Send>,
        metadata: &[u8],
        local_player_index: u8,
    ) -> std::io::Result<Self> {
        writer.write_all(HEADER)?;
        writer.write_u8(VERSION)?;
        writer.write_u32::<byteorder::LittleEndian>(0)?;
        writer.write_u32::<byteorder::LittleEndian>(metadata.len() as u32)?;
        writer.write_all(metadata)?;
        let mut encoder = zstd::Encoder::new(writer, 3)?;
        encoder.write_u8(local_player_index)?;
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
        ip: &input::Pair<input::Input, input::Input>,
    ) -> std::io::Result<()> {
        let (p1, p2) = if local_player_index == 0 {
            (&ip.local, &ip.remote)
        } else {
            (&ip.remote, &ip.local)
        };
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
            .write_u16::<byteorder::LittleEndian>(p1.joyflags)?;
        self.encoder
            .as_mut()
            .unwrap()
            .write_u16::<byteorder::LittleEndian>(p2.joyflags)?;

        self.encoder
            .as_mut()
            .unwrap()
            .write_u8(p1.custom_screen_state)?;
        self.encoder
            .as_mut()
            .unwrap()
            .write_u8(p2.custom_screen_state)?;

        self.encoder
            .as_mut()
            .unwrap()
            .write_u32::<byteorder::LittleEndian>(p1.turn.len() as u32)?;
        self.encoder.as_mut().unwrap().write_all(&p1.turn)?;
        self.encoder
            .as_mut()
            .unwrap()
            .write_u32::<byteorder::LittleEndian>(p2.turn.len() as u32)?;
        self.encoder.as_mut().unwrap().write_all(&p2.turn)?;

        self.num_inputs += 1;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<Box<dyn WriteSeek + Send>> {
        let mut w = self.encoder.take().unwrap().finish()?;
        w.seek(std::io::SeekFrom::Start((HEADER.len() + 1) as u64))?;
        w.write_u32::<byteorder::LittleEndian>(self.num_inputs)?;
        Ok(w)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if let Some(encoder) = self.encoder.take() {
            log::info!("writer was not finished before drop, this replay will be incomplete!");
            encoder.finish().expect("finish");
        }
    }
}
