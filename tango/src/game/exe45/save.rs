use byteorder::ByteOrder;

use crate::save;

const SRAM_SIZE: usize = 0xc7a8;
const MASK_OFFSET: usize = 0x3c84;
const GAME_NAME_OFFSET: usize = 0x4ba8;
const CHECKSUM_OFFSET: usize = 0x4b88;

fn mask(buf: &mut [u8]) {
    let mask = byteorder::LittleEndian::read_u32(&buf[MASK_OFFSET..MASK_OFFSET + 4]);
    for b in buf.iter_mut() {
        *b = *b ^ (mask as u8);
    }
    byteorder::LittleEndian::write_u32(&mut buf[MASK_OFFSET..MASK_OFFSET + 4], mask);
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SRAM_SIZE],
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let mut buf: [u8; SRAM_SIZE] = buf
            .get(..SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        mask(&mut buf[..]);

        let n = &buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE4RO 040607" && n != b"ROCKMANEXE4RO 041217" {
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
        self.buf
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if i < CHECKSUM_OFFSET || i >= CHECKSUM_OFFSET + 4 {
                    *b as u32
                } else {
                    0
                }
            })
            .sum::<u32>()
            + 0x38
    }
}

impl save::Save for Save {
    fn to_vec(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SRAM_SIZE].copy_from_slice(&self.buf);
        save::mask_save(&mut buf[..SRAM_SIZE], MASK_OFFSET);
        buf
    }
}