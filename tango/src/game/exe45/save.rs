use byteorder::ByteOrder;

use crate::save;

const SRAM_SIZE: usize = 0xc7a8;
const MASK_OFFSET: usize = 0x3c84;
const GAME_NAME_OFFSET: usize = 0x4ba8;
const CHECKSUM_OFFSET: usize = 0x4b88;

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
        save::mask_save(&mut buf[..], MASK_OFFSET);

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

    pub fn from_wram(buf: &[u8]) -> Result<Self, anyhow::Error> {
        Ok(Self {
            buf: buf
                .get(..SRAM_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(anyhow::anyhow!("save is wrong size"))?,
        })
    }

    pub fn current_navi(&self) -> u8 {
        self.buf[0x4ad1]
    }
}

impl save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navi(&self) -> Option<Box<dyn save::NaviView + '_>> {
        Some(Box::new(NaviView { save: self }))
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
        1
    }

    fn equipped_folder_index(&self) -> usize {
        0
    }

    fn regular_chip_is_in_place(&self) -> bool {
        false
    }

    fn regular_chip_index(&self, _folder_index: usize) -> Option<usize> {
        None
    }

    fn tag_chip_indexes(&self, _folder_index: usize) -> Option<[usize; 2]> {
        None
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<save::Chip> {
        if folder_index >= 1 || chip_index >= 30 {
            return None;
        }

        let offset = 0x7500 + self.save.current_navi() as usize * (30 * 2) + chip_index * 2;
        let raw = byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]);

        Some(save::Chip {
            id: (raw & 0x1ff) as usize,
            code: (raw >> 9) as usize,
        })
    }
}

pub struct NaviView<'a> {
    save: &'a Save,
}

impl<'a> save::NaviView<'a> for NaviView<'a> {
    fn navi(&self) -> usize {
        self.save.buf[0x4ad1] as usize
    }
}
