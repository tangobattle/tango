use byteorder::ByteOrder;

use crate::games;

const SRAM_START_OFFSET: usize = 0x0100;
const SRAM_SIZE: usize = 0x6710;
const MASK_OFFSET: usize = 0x1064;
const GAME_NAME_OFFSET: usize = 0x1c70;
const CHECKSUM_OFFSET: usize = 0x1c6c;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Region {
    US,
    JP,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Variant {
    Gregar,
    Falzar,
}

#[derive(PartialEq, Debug)]
pub struct GameInfo {
    pub region: Region,
    pub variant: Variant,
}

pub struct Save {
    buf: Vec<u8>,
}

fn mask(buf: &mut [u8]) {
    let mask = byteorder::LittleEndian::read_u32(&buf[MASK_OFFSET..MASK_OFFSET + 4]);
    for b in buf.iter_mut() {
        *b = *b ^ (mask as u8);
    }
    byteorder::LittleEndian::write_u32(&mut buf[MASK_OFFSET..MASK_OFFSET + 4], mask);
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let mut buf = buf
            .get(SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE)
            .map(|buf| buf.to_vec())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;
        mask(&mut buf);

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
            b"REXE6 G 20050924a JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Gregar,
            },
            b"REXE6 F 20050924a JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Falzar,
            },
            b"REXE6 G 20060110a US" => GameInfo {
                region: Region::US,
                variant: Variant::Gregar,
            },
            b"REXE6 F 20060110a US" => GameInfo {
                region: Region::US,
                variant: Variant::Falzar,
            },
            n => {
                anyhow::bail!("unknown game name: {:02x?}", n);
            }
        })
    }

    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
    }

    pub fn compute_checksum(&self) -> u32 {
        let mut checksum = match self.game_info().unwrap().variant {
            Variant::Gregar => 0x72,
            Variant::Falzar => 0x18,
        };

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
