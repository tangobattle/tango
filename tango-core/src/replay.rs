use crate::input;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use std::io::Read;
use std::io::Write;

pub struct Writer {
    encoder: zstd::stream::write::AutoFinishEncoder<'static, Box<dyn std::io::Write + Send>>,
}

const HEADER: &[u8] = b"TOOT";
const VERSION: u8 = 0x0b;

pub struct Replay {
    pub metadata: Vec<u8>,
    pub local_player_index: u8,
    pub state: mgba::state::State,
    pub input_pairs: Vec<input::Pair<input::Input>>,
}

impl Replay {
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

        let metadata_len = r.read_u32::<byteorder::LittleEndian>()?;
        let mut metadata = vec![0u8; metadata_len as usize];
        r.read_exact(&mut metadata[..])?;

        let mut zr = zstd::stream::read::Decoder::new(r)?;

        let local_player_index = zr.read_u8()?;

        let mut state = vec![0u8; zr.read_u32::<byteorder::LittleEndian>()? as usize];
        zr.read_exact(&mut state)?;
        let state = mgba::state::State::from_slice(&state);

        let mut input_pairs = vec![];

        loop {
            let local_tick = match zr.read_u32::<byteorder::LittleEndian>() {
                Ok(local_tick) => local_tick,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        break;
                    }
                    return Err(e);
                }
            };
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
            state,
            input_pairs,
        })
    }
}

impl Writer {
    pub fn new(
        mut writer: Box<dyn std::io::Write + Send>,
        metadata: &[u8],
        local_player_index: u8,
    ) -> std::io::Result<Self> {
        writer.write_all(HEADER)?;
        writer.write_u8(VERSION)?;
        writer.write_u32::<byteorder::LittleEndian>(metadata.len() as u32)?;
        writer.write_all(metadata)?;
        let mut encoder = zstd::Encoder::new(writer, 3)?.auto_finish();
        encoder.write_u8(local_player_index)?;
        encoder.flush()?;
        Ok(Writer { encoder })
    }

    pub fn write_state(&mut self, state: &mgba::state::State) -> std::io::Result<()> {
        self.encoder
            .write_u32::<byteorder::LittleEndian>(state.as_slice().len() as u32)?;
        self.encoder.write_all(state.as_slice())?;
        self.encoder.flush()?;
        Ok(())
    }

    pub fn write_input(
        &mut self,
        local_player_index: u8,
        ip: &input::Pair<input::Input>,
    ) -> std::io::Result<()> {
        let (p1, p2) = if local_player_index == 0 {
            (&ip.local, &ip.remote)
        } else {
            (&ip.remote, &ip.local)
        };
        self.encoder
            .write_u32::<byteorder::LittleEndian>(ip.local.local_tick)?;
        self.encoder
            .write_u32::<byteorder::LittleEndian>(ip.local.remote_tick)?;

        self.encoder
            .write_u16::<byteorder::LittleEndian>(p1.joyflags)?;
        self.encoder
            .write_u16::<byteorder::LittleEndian>(p2.joyflags)?;

        self.encoder.write_u8(p1.custom_screen_state)?;
        self.encoder.write_u8(p2.custom_screen_state)?;

        self.encoder
            .write_u32::<byteorder::LittleEndian>(p1.turn.len() as u32)?;
        self.encoder.write_all(&p1.turn)?;
        self.encoder
            .write_u32::<byteorder::LittleEndian>(p2.turn.len() as u32)?;
        self.encoder.write_all(&p2.turn)?;

        Ok(())
    }
}
