use byteorder::ByteOrder;

use crate::save;

const SRAM_START_OFFSET: usize = 0x0100;
const SRAM_SIZE: usize = 0x6710;
const MASK_OFFSET: usize = 0x1064;
const GAME_NAME_OFFSET: usize = 0x1c70;
const CHECKSUM_OFFSET: usize = 0x1c6c;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Region {
    US,
    JP,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Variant {
    Gregar,
    Falzar,
}

#[derive(PartialEq, Debug)]
pub struct GameInfo {
    pub region: Region,
    pub variant: Variant,
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SRAM_SIZE],
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let mut buf: [u8; SRAM_SIZE] = buf
            .get(SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;
        save::mask_save(&mut buf[..], MASK_OFFSET);

        let save = Self { buf };
        save.game_info()?;

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

    pub fn game_info(&self) -> Result<GameInfo, anyhow::Error> {
        Ok(match &self.buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20] {
            b"REXE6 G 20050924a JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Gregar,
            },
            b"REXE6 F 20050924a JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Falzar,
            },
            b"REXE6 G 20060110a US" => GameInfo {
                region: Region::US,
                variant: Variant::Gregar,
            },
            b"REXE6 F 20060110a US" => GameInfo {
                region: Region::US,
                variant: Variant::Falzar,
            },
            n => {
                anyhow::bail!("unknown game name: {:02x?}", n);
            }
        })
    }

    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4])
    }

    pub fn compute_checksum(&self) -> u32 {
        save::compute_save_raw_checksum(&self.buf, CHECKSUM_OFFSET)
            + match self.game_info().unwrap().variant {
                Variant::Gregar => 0x72,
                Variant::Falzar => 0x18,
            }
    }

    fn current_navi(&self) -> u8 {
        self.buf[0x1b81]
    }

    fn navi_stats_offset(&self, id: u8) -> usize {
        (if self.game_info().unwrap().region == Region::JP {
            0x478c
        } else {
            0x47cc
        }) + 0x64 * if id == 0 { 0 } else { 1 }
    }
}

impl save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_modcards56(&self) -> Option<Box<dyn save::Modcards56View + '_>> {
        if self.game_info().unwrap().region == Region::JP {
            Some(Box::new(Modcards56View { save: self }))
        } else {
            None
        }
    }

    fn as_raw_wram(&self) -> &[u8] {
        &self.buf
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE].copy_from_slice(&self.buf);
        save::mask_save(
            &mut buf[SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE],
            MASK_OFFSET,
        );
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
        self.save.buf[0x1c09] as usize
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[self.save.navi_stats_offset(self.save.current_navi()) + 0x2d] as usize
    }

    fn regular_chip_is_in_place(&self) -> bool {
        true
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        let idx = self.save.buf
            [self.save.navi_stats_offset(self.save.current_navi()) + 0x2e + folder_index];
        if idx == 0 {
            None
        } else {
            Some(idx as usize)
        }
    }

    fn tag_chip_indexes(&self, folder_index: usize) -> Option<[usize; 2]> {
        let idx1 = self.save.buf[self.save.navi_stats_offset(self.save.current_navi())
            + 0x56
            + folder_index * 2
            + 0x00];
        let idx2 = self.save.buf[self.save.navi_stats_offset(self.save.current_navi())
            + 0x56
            + folder_index * 2
            + 0x01];
        if idx1 == 0xff || idx2 == 0xff {
            None
        } else {
            Some([idx1 as usize, idx2 as usize])
        }
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<save::Chip> {
        if folder_index > 3 || chip_index > 30 {
            return None;
        }

        let offset = 0x2178 + folder_index * (30 * 2) + chip_index * 2;
        let raw = byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]);

        Some(save::Chip {
            id: (raw & 0x1ff) as usize,
            code: (raw >> 9) as usize,
        })
    }
}

pub struct Modcards56View<'a> {
    save: &'a Save,
}

impl<'a> save::Modcards56View<'a> for Modcards56View<'a> {
    fn count(&self) -> usize {
        self.save.buf[0x65f0] as usize
    }

    fn modcard(&self, slot: usize) -> Option<save::Modcard56> {
        if slot > self.count() {
            return None;
        }
        let raw = self.save.buf[0x6620 + slot];
        Some(save::Modcard56 {
            id: (raw & 0x7f) as usize,
            enabled: raw >> 7 == 0,
        })
    }
}
