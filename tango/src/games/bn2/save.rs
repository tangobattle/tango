use byteorder::ByteOrder;

use crate::games;

const SRAM_SIZE: usize = 0x3a78;
const GAME_NAME_OFFSET: usize = 0x1198;
const CHECKSUM_OFFSET: usize = 0x114c;

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
        let n = &save.buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE2 20011016" {
            anyhow::bail!("unknown game name: {:02x?}", n);
        }

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
