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

#[derive(PartialEq, Debug, Clone)]
pub struct GameInfo {
    pub variant: Variant,
    pub region: Region,
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SRAM_SIZE],
    shift: usize,
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8], shift: usize) -> u32 {
    save::compute_save_raw_checksum(&buf, shift + CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let mut buf: [u8; SRAM_SIZE] = buf
            .get(..SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
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

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, anyhow::Error> {
        let buf: [u8; SRAM_SIZE] = buf
            .get(..SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        let shift =
            byteorder::LittleEndian::read_u32(&buf[SHIFT_OFFSET..SHIFT_OFFSET + 4]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            anyhow::bail!("invalid shift of {}", shift);
        }

        Ok(Self {
            buf,
            game_info,
            shift,
        })
    }

    #[allow(dead_code)]
    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(
            &self.buf[self.shift + CHECKSUM_OFFSET..self.shift + CHECKSUM_OFFSET + 4],
        )
    }

    #[allow(dead_code)]
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

impl save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navicust(&self) -> Option<Box<dyn save::NavicustView + '_>> {
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_modcards(&self) -> Option<save::ModcardsView> {
        Some(save::ModcardsView::Modcards4(Box::new(Modcards4View {
            save: self,
        })))
    }

    fn view_dark_ai(&self) -> Option<Box<dyn save::DarkAIView + '_>> {
        Some(Box::new(DarkAIView { save: self }))
    }

    fn as_raw_wram(&self) -> &[u8] {
        &self.buf
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SRAM_SIZE].copy_from_slice(&self.buf);
        save::mask_save(&mut buf[..SRAM_SIZE], MASK_OFFSET);
        buf
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

impl<'a> save::ChipsView<'a> for ChipsView<'a> {
    fn chip_codes(&self) -> &'static [u8] {
        &b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[..]
    }

    fn num_folders(&self) -> usize {
        3 // TODO
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[self.save.shift + 0x2132] as usize
    }

    fn regular_chip_is_in_place(&self) -> bool {
        false
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        let idx = self.save.buf[self.save.shift + 0x214d + folder_index];
        if idx == 0xff {
            None
        } else {
            Some(idx as usize)
        }
    }

    fn tag_chip_indexes(&self, _folder_index: usize) -> Option<[usize; 2]> {
        None
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let offset = self.save.shift + 0x262c + folder_index * (30 * 2) + chip_index * 2;
        let raw = byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]);

        Some(save::Chip {
            id: (raw & 0x1ff) as usize,
            code: (raw >> 9) as usize,
        })
    }
}

pub struct NavicustView<'a> {
    save: &'a Save,
}

impl<'a> save::NavicustView<'a> for NavicustView<'a> {
    fn width(&self) -> usize {
        5
    }

    fn height(&self) -> usize {
        5
    }

    fn command_line(&self) -> usize {
        2
    }

    fn has_out_of_bounds(&self) -> bool {
        false
    }

    fn navicust_part(&self, i: usize) -> Option<save::NavicustPart> {
        if i >= 25 {
            return None;
        }

        let offset = self.save.shift + 0x4564;
        let buf = &self.save.buf[offset + i * 8..offset + (i + 1) * 8];
        let raw = buf[0];
        if raw == 0 {
            return None;
        }

        Some(save::NavicustPart {
            id: (raw / 4) as usize,
            variant: (raw % 4) as usize,
            col: buf[0x2],
            row: buf[0x3],
            rot: buf[0x4],
            compressed: buf[0x5] != 0,
        })
    }
}

pub struct Modcards4View<'a> {
    save: &'a Save,
}

impl<'a> save::Modcards4View<'a> for Modcards4View<'a> {
    fn modcard(&self, slot: usize) -> Option<save::Modcard> {
        let mut id = self.save.buf[self.save.shift + 0x464c + slot] as usize;
        let enabled = if id < 0x85 {
            true
        } else {
            id = self.save.buf[self.save.shift + 0x464c + 7 + slot] as usize;
            if id >= 0x85 {
                return None;
            }
            false
        };
        Some(save::Modcard { id, enabled })
    }
}

pub struct DarkAIView<'a> {
    save: &'a Save,
}

impl<'a> save::DarkAIView<'a> for DarkAIView<'a> {
    fn chip_use_count(&self, id: usize) -> Option<u16> {
        if id >= 350 {
            return None;
        }
        let offset = 0x6f50 + id * 2;
        Some(byteorder::LittleEndian::read_u16(
            &self.save.buf[offset..offset + 2],
        ))
    }

    fn secondary_chip_use_count(&self, id: usize) -> Option<u16> {
        if id >= 350 {
            return None;
        }
        let offset = 0x1bb0 + id * 2;
        Some(byteorder::LittleEndian::read_u16(
            &self.save.buf[offset..offset + 2],
        ))
    }
}
