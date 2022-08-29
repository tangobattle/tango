use byteorder::ByteOrder;

use crate::save;

const SRAM_START_OFFSET: usize = 0x0100;
const SRAM_SIZE: usize = 0x7c14;
const MASK_OFFSET: usize = 0x1a34;
const GAME_NAME_OFFSET: usize = 0x29e0;
const CHECKSUM_OFFSET: usize = 0x29dc;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Region {
    US,
    JP,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Variant {
    Protoman,
    Colonel,
}

#[derive(PartialEq, Debug)]
pub struct GameInfo {
    pub region: Region,
    pub variant: Variant,
}

pub struct Save {
    buf: Vec<u8>,
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let mut buf = buf
            .get(SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE)
            .map(|buf| buf.to_vec())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;
        save::mask_save(&mut buf[..], MASK_OFFSET);

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
            b"REXE5TOB 20041104 JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Protoman,
            },
            b"REXE5TOK 20041104 JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Colonel,
            },
            b"REXE5TOB 20041006 US" => GameInfo {
                region: Region::US,
                variant: Variant::Protoman,
            },
            b"REXE5TOK 20041006 US" => GameInfo {
                region: Region::US,
                variant: Variant::Colonel,
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
        save::compute_save_raw_checksum(&self.buf, CHECKSUM_OFFSET)
            + match self.game_info().unwrap().variant {
                Variant::Protoman => 0x72,
                Variant::Colonel => 0x18,
            }
    }
}

impl save::Save for Save {}
