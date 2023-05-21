use byteorder::{ByteOrder, WriteBytesExt};

use crate::save::{self, NavicustView as _};

const SAVE_SIZE: usize = 0x73d2;
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
pub struct Region {
    pub jp: bool,
    pub us: bool,
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
    buf: [u8; SAVE_SIZE],
    shift: usize,
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8], shift: usize) -> u32 {
    save::compute_save_raw_checksum(&buf, shift + CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, save::Error> {
        let mut buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(save::Error::InvalidSize(buf.len()))?;

        save::mask_save(&mut buf[..], MASK_OFFSET);

        let shift = byteorder::LittleEndian::read_u32(&buf[SHIFT_OFFSET..SHIFT_OFFSET + 4]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(save::Error::InvalidShift(shift));
        }

        let n = &buf[shift + GAME_NAME_OFFSET..shift + GAME_NAME_OFFSET + 20];
        if n != b"ROCKMANEXE4 20031022" {
            return Err(save::Error::InvalidGameName(n.to_vec()));
        }

        let game_info = {
            const RED_SUN: u32 = checksum_start_for_variant(Variant::RedSun);
            const BLUE_MOON: u32 = checksum_start_for_variant(Variant::BlueMoon);

            let expected_checksum =
                byteorder::LittleEndian::read_u32(&buf[shift + CHECKSUM_OFFSET..shift + CHECKSUM_OFFSET + 4]);
            let raw_checksum = compute_raw_checksum(&buf, shift);

            let (variant, region) = match expected_checksum.checked_sub(raw_checksum) {
                Some(RED_SUN) => (Variant::RedSun, Region { us: true, jp: false }),
                Some(BLUE_MOON) => (Variant::BlueMoon, Region { us: true, jp: false }),
                None => match expected_checksum.checked_sub(raw_checksum - buf[0] as u32) {
                    Some(RED_SUN) => (Variant::RedSun, Region { us: false, jp: true }),
                    Some(BLUE_MOON) => (Variant::BlueMoon, Region { us: false, jp: true }),
                    _ => {
                        return Err(save::Error::ChecksumMismatch {
                            expected: vec![expected_checksum],
                            actual: raw_checksum,
                            shift,
                            attempt: 1,
                        });
                    }
                },
                _ => {
                    return Err(save::Error::ChecksumMismatch {
                        expected: vec![expected_checksum],
                        actual: raw_checksum,
                        shift,
                        attempt: 0,
                    });
                }
            };

            GameInfo {
                variant,
                region: if buf[0] == 0 {
                    Region { us: true, jp: true }
                } else {
                    region
                },
            }
        };

        let save = Self { buf, shift, game_info };

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, save::Error> {
        let buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(save::Error::InvalidSize(buf.len()))?;

        let shift = byteorder::LittleEndian::read_u32(&buf[SHIFT_OFFSET..SHIFT_OFFSET + 4]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(save::Error::InvalidShift(shift));
        }

        Ok(Self { buf, game_info, shift })
    }

    #[allow(dead_code)]
    pub fn checksum(&self) -> u32 {
        byteorder::LittleEndian::read_u32(&self.buf[self.shift + CHECKSUM_OFFSET..self.shift + CHECKSUM_OFFSET + 4])
    }

    pub fn compute_checksum(&self) -> u32 {
        compute_raw_checksum(&self.buf, self.shift) + checksum_start_for_variant(self.game_info.variant)
            - if self.game_info.region == (Region { us: false, jp: true }) {
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

    fn view_navicust_mut(&mut self) -> Option<Box<dyn save::NavicustViewMut + '_>> {
        Some(Box::new(NavicustViewMut { save: self }))
    }

    fn view_patch_cards(&self) -> Option<save::PatchCardsView> {
        Some(save::PatchCardsView::PatchCard4s(Box::new(PatchCard4sView {
            save: self,
        })))
    }

    fn view_patch_cards_mut(&mut self) -> Option<save::PatchCardsViewMut> {
        Some(save::PatchCardsViewMut::PatchCard4s(Box::new(PatchCard4sViewMut {
            save: self,
        })))
    }

    fn view_auto_battle_data(&self) -> Option<Box<dyn save::AutoBattleDataView + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn view_auto_battle_data_mut(&mut self) -> Option<Box<dyn save::AutoBattleDataViewMut + '_>> {
        Some(Box::new(AutoBattleDataViewMut { save: self }))
    }

    fn as_raw_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SAVE_SIZE].copy_from_slice(&self.buf);
        save::mask_save(&mut buf[..SAVE_SIZE], MASK_OFFSET);
        buf
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        byteorder::LittleEndian::write_u32(
            &mut self.buf[self.shift + CHECKSUM_OFFSET..self.shift + CHECKSUM_OFFSET + 4],
            checksum,
        );
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
        self.save.buf[self.save.shift + 0x2132] as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        let idx = self.save.buf[self.save.shift + 0x214d + folder_index];
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

        let offset = self.save.shift + 0x262c + folder_index * (30 * 2) + chip_index * 2;
        let raw = byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]);

        Some(save::Chip {
            id: (raw & 0x1ff) as usize,
            code: b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[(raw >> 9) as usize] as char,
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

    fn navicust_part(&self, i: usize) -> Option<save::NavicustPart> {
        if i >= self.count() {
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

    fn materialized(&self) -> Option<crate::navicust::MaterializedNavicust> {
        let offset = self.save.shift + 0x4540;

        Some(crate::navicust::materialized_from_wram(
            &self.save.buf[offset..offset + (self.height() * self.width())],
            self.height(),
            self.width(),
        ))
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

        let buf = &mut self.save.buf[self.save.shift + 0x4564 + i * 8..self.save.shift + 0x4564 + (i + 1) * 8];
        buf[0x0] = (part.id * 4 + part.variant) as u8;
        buf[0x2] = part.col as u8;
        buf[0x3] = part.row as u8;
        buf[0x4] = part.rot as u8;
        buf[0x5] = if part.compressed { 1 } else { 0 };
        true
    }

    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets) {
        let materialized = crate::navicust::materialize(&NavicustView { save: self.save }, assets);
        self.save.buf[self.save.shift + 0x4540..self.save.shift + 0x4540 + 0x24].copy_from_slice(
            &materialized
                .into_iter()
                .map(|v| v.map(|v| v + 1).unwrap_or(0) as u8)
                .chain(std::iter::repeat(0))
                .take(0x24)
                .collect::<Vec<_>>(),
        )
    }
}
pub struct PatchCard4sView<'a> {
    save: &'a Save,
}

impl<'a> save::PatchCard4sView<'a> for PatchCard4sView<'a> {
    fn patch_card(&self, slot: usize) -> Option<save::PatchCard> {
        if slot > 6 {
            return None;
        }

        let mut id = self.save.buf[self.save.shift + 0x464c + slot] as usize;

        let enabled = if id < super::NUM_PATCH_CARD4S {
            true
        } else {
            id = self.save.buf[self.save.shift + 0x464c + 7 + slot] as usize;
            if id >= super::NUM_PATCH_CARD4S {
                return None;
            }
            false
        };
        Some(save::PatchCard { id, enabled })
    }
}

pub struct PatchCard4sViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> save::PatchCard4sViewMut<'a> for PatchCard4sViewMut<'a> {
    fn set_patch_card(&mut self, slot: usize, patch_card: Option<save::PatchCard>) -> bool {
        if slot > 6 {
            return false;
        }

        if patch_card
            .as_ref()
            .map(|p| p.id == 0 || p.id >= super::NUM_PATCH_CARD4S)
            .unwrap_or(false)
        {
            return false;
        }

        self.save.buf[self.save.shift + 0x464c + slot] = 0xff;
        self.save.buf[self.save.shift + 0x464c + 7 + slot] = 0xff;

        if let Some(patch_card) = patch_card {
            self.save.buf[self.save.shift + 0x464c + if patch_card.enabled { 0 } else { 7 } + slot] =
                patch_card.id as u8;
        }

        true
    }
}

pub struct AutoBattleDataView<'a> {
    save: &'a Save,
}

impl<'a> save::AutoBattleDataView<'a> for AutoBattleDataView<'a> {
    fn chip_use_count(&self, id: usize) -> Option<usize> {
        if id >= super::NUM_CHIPS {
            return None;
        }
        let offset = 0x6f50 + id * 2;
        Some(byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]) as usize)
    }

    fn secondary_chip_use_count(&self, id: usize) -> Option<usize> {
        if id >= super::NUM_CHIPS {
            return None;
        }
        let offset = 0x1bb0 + id * 2;
        Some(byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]) as usize)
    }

    fn materialized(&self) -> crate::abd::MaterializedAutoBattleData {
        crate::abd::MaterializedAutoBattleData::from_wram(
            &self.save.buf[self.save.shift + 0x5064..self.save.shift + 0x5064 + 42 * 2],
        )
    }
}

pub struct AutoBattleDataViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> save::AutoBattleDataViewMut<'a> for AutoBattleDataViewMut<'a> {
    fn set_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        if id >= super::NUM_CHIPS {
            return false;
        }
        let offset = 0x7340 + id * 2;
        byteorder::LittleEndian::write_u16(&mut self.save.buf[offset..offset + 2], count as u16);
        true
    }

    fn set_secondary_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        if id >= super::NUM_CHIPS {
            return false;
        }
        let offset: usize = 0x2340 + id * 2;
        byteorder::LittleEndian::write_u16(&mut self.save.buf[offset..offset + 2], count as u16);
        true
    }

    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets) {
        let materialized =
            crate::abd::MaterializedAutoBattleData::materialize(&AutoBattleDataView { save: self.save }, assets);
        let mut buf = &mut self.save.buf[self.save.shift + 0x5064..];
        for v in materialized.as_slice() {
            buf.write_u16::<byteorder::LittleEndian>(v.map(|v| v as u16).unwrap_or(0xffff))
                .unwrap();
        }
    }
}
