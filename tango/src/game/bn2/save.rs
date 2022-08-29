use byteorder::ByteOrder;

use crate::save;

const SRAM_SIZE: usize = 0x3a78;
const GAME_NAME_OFFSET: usize = 0x1198;
const CHECKSUM_OFFSET: usize = 0x114c;

#[derive(Clone)]
pub struct Save {
    buf: [u8; SRAM_SIZE],
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let buf: [u8; SRAM_SIZE] = buf
            .get(..SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        let n = &buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE2 20011016" {
            anyhow::bail!("unknown game name: {:02x?}", n);
        }

        let save = Self { buf };

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
        save::compute_save_raw_checksum(&self.buf, CHECKSUM_OFFSET) + 0x16
    }
}

impl save::Save for Save {
    fn to_vec(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SRAM_SIZE].copy_from_slice(&self.buf);
        buf
    }
}
