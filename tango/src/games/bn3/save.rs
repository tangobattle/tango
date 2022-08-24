use byteorder::ByteOrder;

use crate::games;

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

#[derive(PartialEq, Debug)]
pub struct GameInfo {
    pub variant: Variant,
}

pub struct Save {
    buf: Vec<u8>,
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8]) -> u32 {
    let mut checksum = 0;

    for (i, b) in buf.iter().enumerate() {
        if i >= CHECKSUM_OFFSET && i < CHECKSUM_OFFSET + 4 {
            continue;
        }
        checksum += *b as u32;
    }
    checksum
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let buf = buf
            .get(..SRAM_SIZE)
            .map(|buf| buf.to_vec())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        let n = &buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE3 20021002" {
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

impl games::Save for Save {}
