use byteorder::ByteOrder;

use crate::save;

const SRAM_SIZE: usize = 0x3a78;
const GAME_NAME_OFFSET: usize = 0x1198;
const CHECKSUM_OFFSET: usize = 0x114c;

fn compute_checksum(buf: &[u8]) -> u32 {
    save::compute_save_raw_checksum(buf, CHECKSUM_OFFSET) + 0x16
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SRAM_SIZE],
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let buf: [u8; SRAM_SIZE] = buf
            .get(..SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        let n = &buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE2 20011016" {
            anyhow::bail!("unknown game name: {:02x?}", n);
        }

        let expected_checksum = byteorder::LittleEndian::read_u32(&buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4]);
        let computed_checksum = compute_checksum(&buf);
        if expected_checksum != computed_checksum {
            anyhow::bail!(
                "checksum mismatch: expected {:08x}, got {:08x}",
                expected_checksum,
                computed_checksum
            );
        }

        Ok(Save { buf })
    }

    pub fn from_wram(buf: &[u8]) -> Result<Self, anyhow::Error> {
        Ok(Self {
            buf: buf
                .get(..SRAM_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(anyhow::anyhow!("save is wrong size"))?,
        })
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
        buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20].copy_from_slice(b"ROCKMANEXE2 20011016");
        let checksum = compute_checksum(&buf);
        byteorder::LittleEndian::write_u32(&mut buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4], checksum);
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
        3
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[0x0dc2] as usize
    }

    fn regular_chip_is_in_place(&self) -> bool {
        true
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
            code: byteorder::LittleEndian::read_u16(&self.save.buf[offset + 2..offset + 4]) as usize,
        })
    }
}
