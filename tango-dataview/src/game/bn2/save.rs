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
    pub fn new(buf: &[u8]) -> Result<Self, save::Error> {
        let save = Save::from_wram(buf)?;
        let n = &save.buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE2 20011016" {
            return Err(save::Error::InvalidGameName(n.to_vec()));
        }

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            return Err(save::Error::ChecksumMismatch {
                actual: save.checksum(),
                expected: vec![computed_checksum],
                attempt: 0,
                shift: 0,
            });
        }

        Ok(save)
    }

    pub fn from_wram(buf: &[u8]) -> Result<Self, save::Error> {
        Ok(Self {
            buf: buf
                .get(..SRAM_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(save::Error::InvalidSize(buf.len()))?,
        })
    }

    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
    }

    pub fn compute_checksum(&self) -> u32 {
        save::compute_save_raw_checksum(&self.buf, CHECKSUM_OFFSET) + 0x16
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        byteorder::LittleEndian::write_u32(&mut self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4], checksum);
    }
}

impl save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn as_raw_wram(&self) -> &[u8] {
        &self.buf
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SRAM_SIZE].copy_from_slice(&self.buf);
        buf
    }

    fn rebuild(&mut self, _assets: &dyn crate::rom::Assets) {
        self.rebuild_checksum();
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

impl<'a> save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        3
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[0x0dc2] as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        let idx = self.save.buf[0x0ddd + folder_index];
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

        let offset = 0x0ab0 + folder_index * (30 * 4) + chip_index * 4;

        Some(save::Chip {
            id: byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]) as usize,
            code: b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"
                [byteorder::LittleEndian::read_u16(&self.save.buf[offset + 2..offset + 4]) as usize]
                as char,
        })
    }
}
