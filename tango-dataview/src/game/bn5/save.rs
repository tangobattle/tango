use byteorder::{ByteOrder, ReadBytesExt, WriteBytesExt};

use crate::save::{self, PatchCard56sView as _, Save as _};

const SAVE_START_OFFSET: usize = 0x0100;
const SAVE_SIZE: usize = 0x7c14;
const MASK_OFFSET: usize = 0x1a34;
const GAME_NAME_OFFSET: usize = 0x29e0;
const CHECKSUM_OFFSET: usize = 0x29dc;
const SHIFT_OFFSET: usize = 0x1A30;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Region {
    US,
    JP,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Variant {
    Protoman,
    Colonel,
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
            b"REXE5TOB 20041104 JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Protoman,
            },
            b"REXE5TOK 20041104 JP" => GameInfo {
                region: Region::JP,
                variant: Variant::Colonel,
            },
            b"REXE5TOB 20041006 US" => GameInfo {
                region: Region::US,
                variant: Variant::Protoman,
            },
            b"REXE5TOK 20041006 US" => GameInfo {
                region: Region::US,
                variant: Variant::Colonel,
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
                Variant::Protoman => 0x72,
                Variant::Colonel => 0x18,
            }
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        byteorder::LittleEndian::write_u32(
            &mut self.buf[self.shift + CHECKSUM_OFFSET..self.shift + CHECKSUM_OFFSET + 4],
            checksum,
        );
    }

    fn rebuild_materialized_auto_battle_data(&mut self, assets: &dyn crate::rom::Assets) {
        let materialized =
            crate::abd::MaterializedAutoBattleData::materialize(self.view_auto_battle_data().unwrap().as_ref(), assets);
        let mut buf = &mut self.buf[self.shift + 0x554c..];
        for v in materialized.as_slice() {
            buf.write_u16::<byteorder::LittleEndian>(v.map(|v| v as u16).unwrap_or(0xffff))
                .unwrap();
        }
    }

    fn rebuild_precomposed_navicust(&mut self, assets: &dyn crate::rom::Assets) {
        let composed = crate::navicust::compose(self.view_navicust().unwrap().as_ref(), assets);
        self.buf[self.shift + 0x4d48..self.shift + 0x4d48 + 0x24].copy_from_slice(
            &composed
                .into_iter()
                .map(|v| v.map(|v| v + 1).unwrap_or(0) as u8)
                .chain(std::iter::repeat(0))
                .take(0x24)
                .collect::<Vec<_>>(),
        )
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
        Some(save::PatchCardsView::PatchCard56s(Box::new(PatchCard56sView {
            save: self,
        })))
    }

    fn view_patch_cards_mut(&mut self) -> Option<save::PatchCardsViewMut> {
        Some(save::PatchCardsViewMut::PatchCard56s(Box::new(PatchCard56sViewMut {
            save: self,
        })))
    }

    // fn view_navi(&self) -> Option<Box<dyn save::NaviView + '_>> {
    //     Some(Box::new(NaviView { save: self }))
    // }

    fn view_auto_battle_data(&self) -> Option<Box<dyn save::AutoBattleDataView + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

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
        self.rebuild_materialized_auto_battle_data(assets);
        self.rebuild_precomposed_navicust(assets);
        self.rebuild_checksum();
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
        self.save.buf[self.save.shift + 0x52d5] as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<usize> {
        let idx = self.save.buf[self.save.shift + 0x52d6 + folder_index];
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

        let offset = self.save.shift + 0x2df4 + folder_index * (30 * 2) + chip_index * 2;
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
        self.save.buf[self.save.shift + 0x79a0] as usize
    }

    fn patch_card(&self, slot: usize) -> Option<save::PatchCard> {
        if slot >= self.count() {
            return None;
        }
        let raw = self.save.buf[self.save.shift + 0x79d0 + slot];
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
        self.save.buf[self.save.shift + 0x79a0] = count as u8;
    }

    fn set_patch_card(&mut self, slot: usize, patch_card: save::PatchCard) -> bool {
        let view = PatchCard56sView { save: self.save };
        if slot >= view.count() {
            return false;
        }
        self.save.buf[self.save.shift + 0x79d0 + slot] =
            (patch_card.id | (if patch_card.enabled { 0 } else { 1 } << 7)) as u8;
        true
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

        let buf = &self.save.buf[self.save.shift + 0x4d6c + i * 8..self.save.shift + 0x4d6c + (i + 1) * 8];
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

    fn precomposed(&self) -> Option<crate::navicust::ComposedNavicust> {
        let offset = self.save.shift + 0x4d48;

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

pub struct AutoBattleDataView<'a> {
    save: &'a Save,
}

const NUM_AUTO_BATTLE_DATA_CHIPS: usize = 368;

impl<'a> save::AutoBattleDataView<'a> for AutoBattleDataView<'a> {
    fn chip_use_count(&self, id: usize) -> Option<usize> {
        if id >= NUM_AUTO_BATTLE_DATA_CHIPS {
            return None;
        }
        let offset = 0x7340 + id * 2;
        Some(byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]) as usize)
    }

    fn secondary_chip_use_count(&self, id: usize) -> Option<usize> {
        if id >= NUM_AUTO_BATTLE_DATA_CHIPS {
            return None;
        }
        let offset = 0x2340 + id * 2;
        Some(byteorder::LittleEndian::read_u16(&self.save.buf[offset..offset + 2]) as usize)
    }

    fn materialized(&self) -> crate::abd::MaterializedAutoBattleData {
        let mut buf = &self.save.buf[self.save.shift + 0x554c..];
        crate::abd::MaterializedAutoBattleData::new(
            (0..42)
                .map(|_| {
                    let v = buf.read_u16::<byteorder::LittleEndian>().unwrap() as usize;
                    if v == 0xffff {
                        return None;
                    }
                    return Some(v);
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        )
    }
}

pub struct NaviView<'a> {
    save: &'a Save,
}

impl<'a> save::NaviView<'a> for NaviView<'a> {
    fn navi(&self) -> usize {
        self.save.buf[self.save.shift + 0x2940] as usize
    }
}
