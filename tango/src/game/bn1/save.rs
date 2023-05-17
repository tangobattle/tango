use byteorder::ByteOrder;

use crate::save;

const SRAM_SIZE: usize = 0x2308;
const GAME_NAME_OFFSET: usize = 0x03fc;
const CHECKSUM_OFFSET: usize = 0x03f0;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Region {
    US,
    JP,
}

#[derive(PartialEq, Debug, Clone)]
pub struct GameInfo {
    pub region: Region,
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SRAM_SIZE],
    game_info: GameInfo,
}

impl Save {
    pub fn from_raw(buf: [u8; SRAM_SIZE], game_info: GameInfo) -> Self {
        Self { buf, game_info }
    }

    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let buf: [u8; SRAM_SIZE] = buf
            .get(..SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;

        let game_info = match &buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20] {
            b"ROCKMAN EXE 20010120" => GameInfo { region: Region::JP },
            b"ROCKMAN EXE 20010727" => GameInfo { region: Region::US },
            n => {
                anyhow::bail!("unknown game name: {:02x?}", n);
            }
        };

        let save = Self { buf, game_info };

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

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, anyhow::Error> {
        Ok(Self {
            buf: buf
                .get(..SRAM_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(anyhow::anyhow!("save is wrong size"))?,
            game_info,
        })
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
    }

    pub fn compute_checksum(&self) -> u32 {
        save::compute_save_raw_checksum(&self.buf, CHECKSUM_OFFSET) + 0x16
    }

    #[allow(dead_code)]
    pub fn armor(&self) -> usize {
        self.buf[0x0227] as usize
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
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

impl<'a> save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        1
    }

    fn equipped_folder_index(&self) -> usize {
        0
    }

    fn regular_chip_is_in_place(&self) -> bool {
        false
    }

    fn chips_have_mb(&self) -> bool {
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

        Some(save::Chip {
            id: self.save.buf[0x01c0 + chip_index * 2] as usize,
            code: b"ABCDEFGHIJKLMNOPQRSTUVWXYZ"[self.save.buf[0x01c0 + chip_index * 2 + 1] as usize] as char,
        })
    }
}
