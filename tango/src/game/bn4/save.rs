use byteorder::ByteOrder;

use crate::save;

const SRAM_SIZE: usize = 0x73d2;
const MASK_OFFSET: usize = 0x1554;
const GAME_NAME_OFFSET: usize = 0x2208;
const CHECKSUM_OFFSET: usize = 0x21e8;
const SHIFT_OFFSET: usize = 0x1550;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Variant {
    BlueMoon,
    RedSun,
}

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Region {
    Any,
    JP,
    US,
}

const fn checksum_start_for_variant(variant: Variant) -> u32 {
    match variant {
        Variant::RedSun => 0x16,
        Variant::BlueMoon => 0x22,
    }
}

#[derive(PartialEq, Debug)]
pub struct GameInfo {
    pub variant: Variant,
    pub region: Region,
}

pub struct Save {
    buf: Vec<u8>,
    shift: usize,
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8], shift: usize) -> u32 {
    save::compute_save_raw_checksum(&buf, shift + CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let mut buf = buf
            .get(..SRAM_SIZE)
            .map(|buf| buf.to_vec())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        save::mask_save(&mut buf[..], MASK_OFFSET);

        let shift =
            byteorder::LittleEndian::read_u32(&buf[SHIFT_OFFSET..SHIFT_OFFSET + 4]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            anyhow::bail!("invalid shift of {}", shift);
        }

        let n = &buf[shift + GAME_NAME_OFFSET..shift + GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE4 20031022" {
            anyhow::bail!("unknown game name: {:02x?}", n);
        }

        let game_info = {
            const RED_SUN: u32 = checksum_start_for_variant(Variant::RedSun);
            const BLUE_MOON: u32 = checksum_start_for_variant(Variant::BlueMoon);

            let (variant, region) = match byteorder::LittleEndian::read_u32(
                &buf[shift + CHECKSUM_OFFSET..shift + CHECKSUM_OFFSET + 4],
            )
            .checked_sub(compute_raw_checksum(&buf, shift))
            {
                Some(RED_SUN) => (Variant::RedSun, Region::US),
                Some(BLUE_MOON) => (Variant::BlueMoon, Region::US),
                Some(c) => match c.checked_sub(buf[0] as u32) {
                    Some(RED_SUN) => (Variant::RedSun, Region::JP),
                    Some(BLUE_MOON) => (Variant::BlueMoon, Region::JP),
                    _ => {
                        anyhow::bail!("unknown game, bad checksum");
                    }
                },
                None => {
                    anyhow::bail!("unknown game, bad checksum");
                }
            };

            GameInfo {
                variant,
                region: if buf[0] == 0 { Region::Any } else { region },
            }
        };

        let save = Self {
            buf,
            shift,
            game_info,
        };

        Ok(save)
    }

    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(
            &self.buf[self.shift + CHECKSUM_OFFSET..self.shift + CHECKSUM_OFFSET + 4],
        )
    }

    pub fn compute_checksum(&self) -> u32 {
        compute_raw_checksum(&self.buf[self.shift..], self.shift)
            + checksum_start_for_variant(self.game_info.variant)
            - if self.game_info.region == Region::JP {
                self.buf[0] as u32
            } else {
                0
            }
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }
}

impl save::Save for Save {}
