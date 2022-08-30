use byteorder::ByteOrder;

use crate::save;

const SRAM_SIZE: usize = 0x57b0;
const GAME_NAME_OFFSET: usize = 0x1e00;
const CHECKSUM_OFFSET: usize = 0x1dd8;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Variant {
    White,
    Blue,
}

const fn checksum_start_for_variant(variant: Variant) -> u32 {
    match variant {
        Variant::White => 0x16,
        Variant::Blue => 0x22,
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct GameInfo {
    pub variant: Variant,
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SRAM_SIZE],
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8]) -> u32 {
    save::compute_save_raw_checksum(buf, CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let buf: [u8; SRAM_SIZE] = buf
            .get(..SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        let n = &buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE3 20021002" && n != b"BBN3 v0.5.0 20021002" {
            anyhow::bail!("unknown game name: {:02x?}", n);
        }

        let game_info = {
            const WHITE: u32 = checksum_start_for_variant(Variant::White);
            const BLUE: u32 = checksum_start_for_variant(Variant::Blue);
            GameInfo {
                variant: match byteorder::LittleEndian::read_u32(
                    &buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4],
                )
                .checked_sub(compute_raw_checksum(&buf))
                {
                    Some(WHITE) => Variant::White,
                    Some(BLUE) => Variant::Blue,
                    n => {
                        anyhow::bail!("unknown checksum start: {:02x?}", n)
                    }
                },
            }
        };

        let save = Self { buf, game_info };

        Ok(save)
    }

    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
    }

    pub fn compute_checksum(&self) -> u32 {
        compute_raw_checksum(&self.buf) + checksum_start_for_variant(self.game_info.variant)
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }
}

impl save::Save for Save {
    fn as_raw_wram(&self) -> &[u8] {
        &self.buf
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SRAM_SIZE].copy_from_slice(&self.buf);
        buf
    }
}
