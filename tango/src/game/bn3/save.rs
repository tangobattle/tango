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
                variant: match byteorder::LittleEndian::read_u32(&buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
                    .checked_sub(save::compute_save_raw_checksum(&buf, CHECKSUM_OFFSET))
                {
                    Some(WHITE) => Variant::White,
                    Some(BLUE) => Variant::Blue,
                    n => {
                        anyhow::bail!("unknown checksum start: {:02x?}", n)
                    }
                },
            }
        };

        Ok(Self { buf, game_info })
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, anyhow::Error> {
        Ok(Self {
            buf: buf
                .get(..SRAM_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(anyhow::anyhow!("save is wrong size"))?,
            game_info,
        })
    }

    #[allow(dead_code)]
    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
    }

    #[allow(dead_code)]
    pub fn compute_checksum(&self) -> u32 {
        save::compute_save_raw_checksum(&self.buf, CHECKSUM_OFFSET) + checksum_start_for_variant(self.game_info.variant)
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }
}

impl save::Save for Save {
    fn as_raw_wram(&self) -> &[u8] {
        &self.buf
    }

    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navicust(&self) -> Option<Box<dyn save::NavicustView + '_>> {
        Some(Box::new(NavicustView { save: self }))
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SRAM_SIZE].copy_from_slice(&self.buf);
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
        self.save.buf[0x1882] as usize
    }

    fn regular_chip_is_in_place(&self) -> bool {
        true
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        let idx = self.save.buf[0x189d + folder_index];
        if idx >= 30 {
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

        let offset = 0x1410 + folder_index * (30 * 4) + chip_index * 4;

        Some(save::Chip {
            id: byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]) as usize,
            code: byteorder::LittleEndian::read_u16(&self.save.buf[offset + 2..offset + 4]) as usize,
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

    fn style(&self) -> Option<usize> {
        Some((self.save.buf[0x1881] & 0x3f) as usize)
    }

    fn navicust_part(&self, i: usize) -> Option<save::NavicustPart> {
        if i >= 25 {
            return None;
        }

        let buf = &self.save.buf[0x1300 + i * 8..0x1300 + (i + 1) * 8];
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
            compressed: (self.save.buf[0x0310 + (raw >> 3) as usize] & (0x80 >> (raw >> 7))) != 0,
        })
    }
}
