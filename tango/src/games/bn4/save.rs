use byteorder::ByteOrder;

use crate::games;

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
    games::compute_save_raw_checksum(&buf, shift + CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let mut buf = buf
            .get(..SRAM_SIZE)
            .map(|buf| buf.to_vec())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        games::mask_save(&mut buf[..], MASK_OFFSET);

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

            let us_checksum_remaining = byteorder::LittleEndian::read_u32(
                &buf[shift + CHECKSUM_OFFSET..shift + CHECKSUM_OFFSET + 4],
            )
            .checked_sub(compute_raw_checksum(&buf, shift));

            // I'm pretty sure the developers did not intend to exclude the first byte, but this is how the JP version detects saves, I guess.
            let jp_checksum_remaining =
                us_checksum_remaining.and_then(|v| v.checked_sub(buf[0] as u32));

            let (variant, mut region) = match jp_checksum_remaining {
                Some(RED_SUN) => (Variant::RedSun, Region::JP),
                Some(BLUE_MOON) => (Variant::BlueMoon, Region::JP),
                _ => match us_checksum_remaining {
                    Some(RED_SUN) => (Variant::RedSun, Region::US),
                    Some(BLUE_MOON) => (Variant::BlueMoon, Region::US),
                    _ => {
                        anyhow::bail!("unknown game, remaining checksum was either {:02x?} (jp) or {:02x?} (us)", jp_checksum_remaining, us_checksum_remaining);
                    }
                },
            };

            if us_checksum_remaining == jp_checksum_remaining {
                region = Region::Any;
            }

            GameInfo { variant, region }
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

impl games::Save for Save {}
