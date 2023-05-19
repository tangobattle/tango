use byteorder::ByteOrder;

use crate::save::{self, ChipsView as _, NaviView as _, NavicustView as _, PatchCard56sView as _, Save as _};

const SAVE_START_OFFSET: usize = 0x0100;
const SAVE_SIZE: usize = 0x6710;
const MASK_OFFSET: usize = 0x1064;
const GAME_NAME_OFFSET: usize = 0x1c70;
const CHECKSUM_OFFSET: usize = 0x1c6c;
const SHIFT_OFFSET: usize = 0x1060;

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
    buf: [u8; SAVE_SIZE],
    shift: usize,
    game_info: GameInfo,
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, save::Error> {
        let mut buf: [u8; SAVE_SIZE] = buf
            .get(SAVE_START_OFFSET..SAVE_START_OFFSET + SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(save::Error::InvalidSize(buf.len()))?;

        save::mask_save(&mut buf[..], MASK_OFFSET);

        let shift = byteorder::LittleEndian::read_u32(&buf[SHIFT_OFFSET..SHIFT_OFFSET + 4]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(save::Error::InvalidShift(shift));
        }

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
                return Err(save::Error::InvalidGameName(n.to_vec()));
            }
        };

        let save = Self { buf, shift, game_info };

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            return Err(save::Error::ChecksumMismatch {
                actual: save.checksum(),
                expected: vec![computed_checksum],
                shift,
                attempt: 0,
            });
        }

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, save::Error> {
        let shift = byteorder::LittleEndian::read_u32(&buf[SHIFT_OFFSET..SHIFT_OFFSET + 4]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(save::Error::InvalidShift(shift));
        }

        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(save::Error::InvalidSize(buf.len()))?,
            shift,
            game_info,
        })
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[self.shift + CHECKSUM_OFFSET..self.shift + CHECKSUM_OFFSET + 4])
    }

    pub fn compute_checksum(&self) -> u32 {
        save::compute_save_raw_checksum(&self.buf, self.shift + CHECKSUM_OFFSET)
            + match self.game_info.variant {
                Variant::Gregar => 0x72,
                Variant::Falzar => 0x18,
            }
    }

    fn navi_stats_offset(&self, id: usize) -> usize {
        self.shift
            + (if self.game_info.region == Region::JP {
                0x478c
            } else {
                0x47cc
            })
            + 0x64 * if id == 0 { 0 } else { 1 }
    }

    fn rebuild_patch_cards_anticheat(&mut self) {
        for id in 0..super::NUM_PATCH_CARD56S {
            self.buf[self.shift + 0x5047 + id] = self.buf[self.shift + 0x06bf + id]
                ^ match self.game_info.variant {
                    Variant::Gregar => 0x43,
                    Variant::Falzar => 0x8d,
                };
        }
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        byteorder::LittleEndian::write_u32(
            &mut self.buf[self.shift + CHECKSUM_OFFSET..self.shift + CHECKSUM_OFFSET + 4],
            checksum,
        );
    }

    fn rebuild_precomposed_navicust(&mut self, assets: &dyn crate::rom::Assets) {
        let composed = crate::navicust::compose(self.view_navicust().unwrap().as_ref(), assets);
        self.buf[self.shift + 0x4d48..self.shift + 0x4d48 + 0x44].copy_from_slice(
            &composed
                .into_iter()
                .map(|v| v.map(|v| v + 1).unwrap_or(0) as u8)
                .chain(std::iter::repeat(0))
                .take(0x44)
                .collect::<Vec<_>>(),
        )
    }

    fn rebuild_anticheat(&mut self) {
        self.rebuild_patch_cards_anticheat();
    }
}

impl save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn save::ChipsViewMut + '_>> {
        Some(Box::new(ChipsViewMut { save: self }))
    }

    fn view_navicust(&self) -> Option<Box<dyn save::NavicustView + '_>> {
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_navicust_mut(&mut self) -> Option<Box<dyn save::NavicustViewMut + '_>> {
        Some(Box::new(NavicustViewMut { save: self }))
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
        buf[SAVE_START_OFFSET..SAVE_START_OFFSET + SAVE_SIZE].copy_from_slice(&self.buf);
        save::mask_save(&mut buf[SAVE_START_OFFSET..SAVE_START_OFFSET + SAVE_SIZE], MASK_OFFSET);
        buf
    }

    fn rebuild(&mut self, assets: &dyn crate::rom::Assets) {
        self.rebuild_precomposed_navicust(assets);
        self.rebuild_anticheat();
        self.rebuild_checksum();
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

impl<'a> save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        self.save.buf[self.save.shift + 0x1c09] as usize
    }

    fn equipped_folder_index(&self) -> usize {
        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        self.save.buf[navi_stats_offset + 0x2d] as usize
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

        let offset = self.save.shift + 0x2178 + folder_index * (30 * 2) + chip_index * 2;
        let raw = byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]);

        Some(save::Chip {
            id: (raw & 0x1ff) as usize,
            code: b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[(raw >> 9) as usize] as char,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        Some(self.save.buf[self.save.shift + 0x2230 + id * 0xc + variant] as usize)
    }
}

pub struct PatchCard56sView<'a> {
    save: &'a Save,
}

impl<'a> save::PatchCard56sView<'a> for PatchCard56sView<'a> {
    fn count(&self) -> usize {
        self.save.buf[self.save.shift + 0x65f0] as usize
    }

    fn patch_card(&self, slot: usize) -> Option<save::PatchCard> {
        if slot >= self.count() {
            return None;
        }
        let raw = self.save.buf[self.save.shift + 0x6620 + slot];
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
        self.save.buf[self.save.shift + 0x65f0] = count as u8;
    }

    fn set_patch_card(&mut self, slot: usize, patch_card: save::PatchCard) -> bool {
        let view = PatchCard56sView { save: self.save };
        if slot >= view.count() {
            return false;
        }
        self.save.buf[self.save.shift + 0x6620 + slot] =
            (patch_card.id | (if patch_card.enabled { 0 } else { 1 } << 7)) as u8;
        true
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

        let offset = self.save.shift + 0x2178 + folder_index * (30 * 2) + chip_index * 2;
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

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        self.save.buf[self.save.shift + 0x2230 + id * 0xc + variant] = count as u8;
        true
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

    fn navicust_part(&self, i: usize) -> Option<save::NavicustPart> {
        if i >= self.count() {
            return None;
        }

        let ncp_offset = self.save.shift
            + if self.save.game_info.region == Region::JP {
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

    fn precomposed(&self) -> Option<crate::navicust::ComposedNavicust> {
        let offset = self.save.shift
            + if self.save.game_info.region == Region::JP {
                0x410C
            } else {
                0x414C
            };

        Some(
            ndarray::Array2::from_shape_vec(
                (self.height(), self.width()),
                self.save.buf[offset..offset + (self.height() * self.width())]
                    .iter()
                    .map(|v| v.checked_sub(1).map(|v| v as usize))
                    .collect(),
            )
            .unwrap(),
        )
    }
}

pub struct NavicustViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> save::NavicustViewMut<'a> for NavicustViewMut<'a> {
    fn set_navicust_part(&mut self, i: usize, part: save::NavicustPart) -> bool {
        if part.id >= super::NUM_NAVICUST_PARTS.0 || part.variant >= super::NUM_NAVICUST_PARTS.1 {
            return false;
        }
        if i >= (NavicustView { save: self.save }).count() {
            return false;
        }

        let ncp_offset = self.save.shift
            + if self.save.game_info.region == Region::JP {
                0x4150
            } else {
                0x4190
            };

        let buf = &mut self.save.buf[ncp_offset + i * 8..ncp_offset + (i + 1) * 8];
        buf[0x0] = (part.id * 4 + part.variant) as u8;
        buf[0x3] = part.col as u8;
        buf[0x4] = part.row as u8;
        buf[0x5] = part.rot as u8;
        buf[0x6] = if part.compressed { 1 } else { 0 };
        true
    }
}

pub struct NaviView<'a> {
    save: &'a Save,
}

impl<'a> save::NaviView<'a> for NaviView<'a> {
    fn navi(&self) -> usize {
        self.save.buf[self.save.shift + 0x1b81] as usize
    }
}
