use byteorder::ByteOrder;

use crate::save::{self, ChipsView as _, NaviView as _, PatchCard56sView as _};

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

#[derive(PartialEq, Debug, Clone)]
pub struct GameInfo {
    pub region: Region,
    pub variant: Variant,
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SRAM_SIZE],
    game_info: GameInfo,
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, anyhow::Error> {
        let mut buf: [u8; SRAM_SIZE] = buf
            .get(SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(anyhow::anyhow!("save is wrong size"))?;
        save::mask_save(&mut buf[..], MASK_OFFSET);

        let game_info = match &buf[GAME_NAME_OFFSET..GAME_NAME_OFFSET + 20] {
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
        save::compute_save_raw_checksum(&self.buf, CHECKSUM_OFFSET)
            + match self.game_info.variant {
                Variant::Gregar => 0x72,
                Variant::Falzar => 0x18,
            }
    }

    fn navi_stats_offset(&self, id: usize) -> usize {
        (if self.game_info.region == Region::JP {
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

    fn view_navicust(&self) -> Option<Box<dyn save::NavicustView + '_>> {
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_patch_cards(&self) -> Option<save::PatchCardsView> {
        if self.game_info.region != Region::JP {
            return None;
        }
        Some(save::PatchCardsView::PatchCard56s(Box::new(PatchCard56sView {
            save: self,
        })))
    }

    fn view_patch_cards_mut(&mut self) -> Option<save::PatchCardsViewMut> {
        if self.game_info.region != Region::JP {
            return None;
        }
        Some(save::PatchCardsViewMut::PatchCard56s(Box::new(PatchCard56sViewMut {
            save: self,
        })))
    }

    // fn view_navi(&self) -> Option<Box<dyn save::NaviView + '_>> {
    //     Some(Box::new(NaviView { save: self }))
    // }

    fn as_raw_wram(&self) -> &[u8] {
        &self.buf
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE].copy_from_slice(&self.buf);
        save::mask_save(&mut buf[SRAM_START_OFFSET..SRAM_START_OFFSET + SRAM_SIZE], MASK_OFFSET);
        buf
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

impl<'a> save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        self.save.buf[0x1c09] as usize
    }

    fn equipped_folder_index(&self) -> usize {
        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        self.save.buf[navi_stats_offset + 0x2d] as usize
    }

    fn regular_chip_is_in_place(&self) -> bool {
        true
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        if folder_index >= self.num_folders() {
            return None;
        }

        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        let idx = self.save.buf[navi_stats_offset + 0x2e + folder_index];
        if idx >= 30 {
            None
        } else {
            Some(idx as usize)
        }
    }

    fn tag_chip_indexes(&self, folder_index: usize) -> Option<[usize; 2]> {
        if folder_index >= self.num_folders() {
            return None;
        }

        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        let idx1 = self.save.buf[navi_stats_offset + 0x56 + folder_index * 2 + 0x00];
        let idx2 = self.save.buf[navi_stats_offset + 0x56 + folder_index * 2 + 0x01];
        if idx1 == 0xff || idx2 == 0xff {
            None
        } else {
            Some([idx1 as usize, idx2 as usize])
        }
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let offset = 0x2178 + folder_index * (30 * 2) + chip_index * 2;
        let raw = byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]);

        Some(save::Chip {
            id: (raw & 0x1ff) as usize,
            code: b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[(raw >> 9) as usize] as char,
        })
    }
}

pub struct PatchCard56sView<'a> {
    save: &'a Save,
}

impl<'a> save::PatchCard56sView<'a> for PatchCard56sView<'a> {
    fn count(&self) -> usize {
        self.save.buf[0x65f0] as usize
    }

    fn patch_card(&self, slot: usize) -> Option<save::PatchCard> {
        if slot >= self.count() {
            return None;
        }
        let raw = self.save.buf[0x6620 + slot];
        Some(save::PatchCard {
            id: (raw & 0x7f) as usize,
            enabled: raw >> 7 == 0,
        })
    }
}

pub struct PatchCard56sViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> save::PatchCard56sViewMut<'a> for PatchCard56sViewMut<'a> {
    fn set_count(&mut self, count: usize) {
        self.save.buf[0x65f0] = count as u8;
        self.rebuild();
    }

    fn set_patch_card(&mut self, slot: usize, patch_card: save::PatchCard) -> bool {
        let view = PatchCard56sView { save: self.save };
        if slot >= view.count() {
            return false;
        }
        self.save.buf[0x6620 + slot] = (patch_card.id | (if patch_card.enabled { 0 } else { 1 } << 7)) as u8;
        self.rebuild();
        true
    }
}

impl<'a> PatchCard56sViewMut<'a> {
    fn rebuild(&mut self) {
        for i in 0..118 {
            self.set_patch_card_loaded(i, false);
        }

        for i in 0..(PatchCard56sView { save: self.save }).count() {
            let patch_card = if let Some(patch_card) = (PatchCard56sView { save: self.save }).patch_card(i) {
                patch_card
            } else {
                continue;
            };
            self.set_patch_card_loaded(patch_card.id, true);
        }
    }

    fn set_patch_card_loaded(&mut self, id: usize, loaded: bool) {
        let mask = match self.save.game_info.variant {
            Variant::Gregar => 0x43,
            Variant::Falzar => 0x8d,
        };
        self.save.buf[0x5047 + id] = self.save.buf[0x06bf + id] ^ if loaded { mask } else { 0xff };
    }
}

pub struct ChipsViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> save::ChipsViewMut<'a> for ChipsViewMut<'a> {
    fn set_equipped_folder(&mut self, folder_index: usize) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() {
            return false;
        }
        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        self.save.buf[navi_stats_offset + 0x2d] = folder_index as u8;
        true
    }

    fn set_chip(&mut self, folder_index: usize, chip_index: usize, chip: save::Chip) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() || chip_index >= 30 {
            return false;
        }

        let offset = 0x2178 + folder_index * (30 * 2) + chip_index * 2;
        let variant = if let Some(variant) = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"
            .iter()
            .position(|c| *c == chip.code as u8)
        {
            variant
        } else {
            return false;
        };
        byteorder::LittleEndian::write_u16(
            &mut self.save.buf[offset..offset + 2],
            chip.id as u16 | ((variant as u16) << 9),
        );
        self.rebuild();
        true
    }

    fn set_tag_chip_indexes(&mut self, folder_index: usize, chip_indexes: Option<[usize; 2]>) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() {
            return false;
        }

        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        let (idx1, idx2) = if let Some([idx1, idx2]) = chip_indexes {
            if idx1 >= 30 || idx2 >= 30 {
                return false;
            }
            (idx1, idx2)
        } else {
            (0xff, 0xff)
        };

        self.save.buf[navi_stats_offset + 0x56 + folder_index * 2 + 0x00] = idx1 as u8;
        self.save.buf[navi_stats_offset + 0x56 + folder_index * 2 + 0x01] = idx2 as u8;
        true
    }

    fn set_regular_chip_index(&mut self, folder_index: usize, chip_index: usize) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() || chip_index >= 30 {
            return false;
        }

        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        self.save.buf[navi_stats_offset + 0x2e + folder_index] = chip_index as u8;
        true
    }

    fn can_set_regular_chip(&self) -> bool {
        true
    }

    fn can_set_tag_chips(&self) -> bool {
        true
    }
}

impl<'a> ChipsViewMut<'a> {
    fn rebuild(&mut self) {
        // Kind of goody, but it works.
        for id in 0..411 {
            for variant in 0..4 {
                self.set_pack_count(id, variant, 99);
            }
        }
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: u8) {
        self.save.buf[0x2230 + id * 0xc + variant] = count;
    }
}

pub struct NavicustView<'a> {
    save: &'a Save,
}

impl<'a> save::NavicustView<'a> for NavicustView<'a> {
    fn width(&self) -> usize {
        7
    }

    fn height(&self) -> usize {
        7
    }

    fn command_line(&self) -> usize {
        3
    }

    fn has_out_of_bounds(&self) -> bool {
        true
    }

    fn navicust_part(&self, i: usize) -> Option<save::NavicustPart> {
        if i >= self.count() {
            return None;
        }

        let ncp_offset = if self.save.game_info.region == Region::JP {
            0x4150
        } else {
            0x4190
        };

        let buf = &self.save.buf[ncp_offset + i * 8..ncp_offset + (i + 1) * 8];
        let raw = buf[0];
        if raw == 0 {
            return None;
        }

        Some(save::NavicustPart {
            id: (raw / 4) as usize,
            variant: (raw % 4) as usize,
            col: buf[0x3],
            row: buf[0x4],
            rot: buf[0x5],
            compressed: buf[0x6] != 0,
        })
    }
}
pub struct NaviView<'a> {
    save: &'a Save,
}

impl<'a> save::NaviView<'a> for NaviView<'a> {
    fn navi(&self) -> usize {
        self.save.buf[0x1b81] as usize
    }
}
