use byteorder::ByteOrder;

use crate::save;

pub const SAVE_SIZE: usize = 0x57b0;
pub const GAME_NAME_OFFSET: usize = 0x1e00;
pub const CHECKSUM_OFFSET: usize = 0x1dd8;

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
    buf: [u8; SAVE_SIZE],
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8]) -> u32 {
    save::compute_save_raw_checksum(buf, CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, save::Error> {
        let buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(save::Error::InvalidSize(buf.len()))?;

        let n = &buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE3 20021002" && n != b"BBN3 v0.5.0 20021002" {
            return Err(save::Error::InvalidGameName(n.to_vec()));
        }

        let save_checksum = byteorder::LittleEndian::read_u32(&buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4]);
        let raw_checksum = compute_raw_checksum(&buf);
        let game_info = {
            const WHITE: u32 = checksum_start_for_variant(Variant::White);
            const BLUE: u32 = checksum_start_for_variant(Variant::Blue);
            GameInfo {
                variant: match save_checksum.checked_sub(raw_checksum) {
                    Some(WHITE) => Variant::White,
                    Some(BLUE) => Variant::Blue,
                    _ => {
                        return Err(save::Error::ChecksumMismatch {
                            actual: save_checksum,
                            expected: vec![raw_checksum + WHITE, raw_checksum + BLUE],
                            attempt: 0,
                            shift: 0,
                        });
                    }
                },
            }
        };

        let save = Self { buf, game_info };

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(save::Error::InvalidSize(buf.len()))?,
            game_info,
        })
    }

    #[allow(dead_code)]
    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
    }

    #[allow(dead_code)]
    pub fn compute_checksum(&self) -> u32 {
        compute_raw_checksum(&self.buf) + checksum_start_for_variant(self.game_info.variant)
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }
}

impl save::Save for Save {
    fn as_raw_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navicust(&self) -> Option<Box<dyn save::NavicustView + '_>> {
        Some(Box::new(NavicustView { save: self }))
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SAVE_SIZE].copy_from_slice(&self.buf);
        buf
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        byteorder::LittleEndian::write_u32(&mut self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4], checksum);
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

impl<'a> save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        3 // TODO
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[0x1882] as usize
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
            code: b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"
                [byteorder::LittleEndian::read_u16(&self.save.buf[offset + 2..offset + 4]) as usize]
                as char,
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

    fn style(&self) -> Option<usize> {
        Some((self.save.buf[0x1881] & 0x3f) as usize)
    }

    fn navicust_part(&self, i: usize) -> Option<save::NavicustPart> {
        if i >= self.count() {
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

    fn materialized(&self) -> Option<crate::navicust::MaterializedNavicust> {
        None
    }
}
