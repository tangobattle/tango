use byteorder::ByteOrder;

use crate::games;

const SRAM_SIZE: usize = 0x2308;
const GAME_NAME_OFFSET: usize = 0x03fc;
const CHECKSUM_OFFSET: usize = 0x03f0;

#[derive(PartialEq, Debug)]
pub enum Region {
    US,
    JP,
}

#[derive(PartialEq, Debug)]
pub struct GameInfo {
    pub region: Region,
}

pub struct Save {
    buf: Vec<u8>,
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let buf = buf
            .get(..SRAM_SIZE)
            .map(|buf| buf.to_vec())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        let save = Self { buf };
        save.game_info()?;

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            anyhow::bail!(
                "checksum mismatch: expected {:08x}, got {:08x}",
                save.checksum(),
                computed_checksum
            );
        }

        Ok(save)
    }

    pub fn game_info(&self) -> Result<GameInfo, anyhow::Error> {
        Ok(match &self.buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20] {
            b"ROCKMAN EXE 20010120" => GameInfo { region: Region::JP },
            b"ROCKMAN EXE 20010727" => GameInfo { region: Region::US },
            n => {
                anyhow::bail!("unknown game name: {:02x?}", n);
            }
        })
    }

    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
    }

    pub fn compute_checksum(&self) -> u32 {
        let mut checksum = 0x16;

        for (i, b) in self.buf.iter().enumerate() {
            if i >= CHECKSUM_OFFSET && i < CHECKSUM_OFFSET + 4 {
                continue;
            }
            checksum += *b as u32;
        }
        checksum
    }
}

impl games::Save for Save {}
