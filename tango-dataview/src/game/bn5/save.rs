use crate::save::{self, NavicustView as _, PatchCard56sView as _};

pub const SAVE_START_OFFSET: usize = 0x0100;
pub const SAVE_SIZE: usize = 0x7c14;
pub const MASK_OFFSET: usize = 0x1a34;
pub const GAME_NAME_OFFSET: usize = 0x29e0;
pub const CHECKSUM_OFFSET: usize = 0x29dc;
pub const SHIFT_OFFSET: usize = 0x1A30;

pub const EREADER_NAME_OFFSET: usize = 0x1d16;
pub const EREADER_NAME_SIZE: usize = 0x18;
pub const EREADER_DESCRIPTION_OFFSET: usize = 0x1376;
pub const EREADER_DESCRIPTION_SIZE: usize = 0x64;

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
            .get(SAVE_START_OFFSET..)
            .and_then(|buf| buf.get(..SAVE_SIZE))
            .and_then(|buf| buf.try_into().ok())
            .ok_or(save::Error::InvalidSize(buf.len()))?;

        save::mask_save(&mut buf[..], MASK_OFFSET);

        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..4]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(save::Error::InvalidShift(shift));
        }

        let game_info = match &buf[GAME_NAME_OFFSET..][..20] {
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
        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..4]) as usize;
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
        bytemuck::pod_read_unaligned::<u32>(&self.buf[self.shift + CHECKSUM_OFFSET..][..4])
    }

    pub fn shift(&self) -> usize {
        self.shift
    }

    pub fn compute_checksum(&self) -> u32 {
        save::compute_save_raw_checksum(&self.buf, self.shift + CHECKSUM_OFFSET)
            + match self.game_info.variant {
                Variant::Protoman => 0x72,
                Variant::Colonel => 0x18,
            }
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

    fn view_auto_battle_data_mut(&mut self) -> Option<Box<dyn save::AutoBattleDataViewMut + '_>> {
        Some(Box::new(AutoBattleDataViewMut { save: self }))
    }

    fn as_raw_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[SAVE_START_OFFSET..][..SAVE_SIZE].copy_from_slice(&self.buf);
        save::mask_save(&mut buf[SAVE_START_OFFSET..][..SAVE_SIZE], MASK_OFFSET);
        buf
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        self.buf[CHECKSUM_OFFSET..][..4].copy_from_slice(&bytemuck::cast::<_, [u8; 4]>(checksum));
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

        let raw = bytemuck::pod_read_unaligned::<u16>(
            &self.save.buf
                [self.save.shift + 0x2df4 + folder_index * (30 * 2) + chip_index * std::mem::size_of::<u16>()..]
                [..std::mem::size_of::<u16>()],
        );

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

    fn rebuild_anticheat(&mut self) {
        let mask = match self.save.game_info.variant {
            Variant::Protoman => 0x43,
            Variant::Colonel => 0x8d,
        };
        for id in 0..super::NUM_PATCH_CARD56S {
            self.save.buf[self.save.shift + 0x60dc + id] = self.save.buf[self.save.shift + 0x1220 + id] ^ mask;
        }
    }
}

pub struct NavicustView<'a> {
    save: &'a Save,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawNavicustPart {
    id_and_variant: u8,
    _unk_01: u8,
    col: u8,
    row: u8,
    rot: u8,
    compressed: u8,
    _unk_06: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x8);

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

        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.buf[self.save.shift + 0x4d6c + i * std::mem::size_of::<RawNavicustPart>()..]
                [..std::mem::size_of::<RawNavicustPart>()],
        );

        Some(save::NavicustPart {
            id: (raw.id_and_variant / 4) as usize,
            variant: (raw.id_and_variant % 4) as usize,
            col: raw.col,
            row: raw.row,
            rot: raw.rot,
            compressed: raw.compressed != 0,
        })
    }

    fn materialized(&self) -> Option<crate::navicust::MaterializedNavicust> {
        let offset = self.save.shift + 0x4d48;

        Some(crate::navicust::materialized_from_wram(
            &self.save.buf[offset..][..(self.height() * self.width())],
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

        self.save.buf[self.save.shift + 0x4d6c + i * std::mem::size_of::<RawNavicustPart>()..]
            [..std::mem::size_of::<RawNavicustPart>()]
            .copy_from_slice(bytemuck::bytes_of(&RawNavicustPart {
                id_and_variant: (part.id * 4 + part.variant) as u8,
                col: part.col,
                row: part.row,
                rot: part.rot,
                compressed: if part.compressed { 1 } else { 0 },
                ..Default::default()
            }));

        true
    }

    fn clear_materialized(&mut self) {
        self.save.buf[self.save.shift + 0x4d48..][..0x24].copy_from_slice(&[0; 0x24]);
    }

    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets) {
        let materialized = crate::navicust::materialize(&NavicustView { save: self.save }, assets);
        self.save.buf[self.save.shift + 0x4d48..][..0x24].copy_from_slice(
            &materialized
                .into_iter()
                .map(|v| v.map(|v| v + 1).unwrap_or(0) as u8)
                .chain(std::iter::repeat(0))
                .take(0x24)
                .collect::<Vec<_>>(),
        )
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
        Some(bytemuck::pod_read_unaligned::<u16>(
            &self.save.buf[0x7340 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()],
        ) as usize)
    }

    fn secondary_chip_use_count(&self, id: usize) -> Option<usize> {
        if id >= super::NUM_CHIPS {
            return None;
        }
        Some(bytemuck::pod_read_unaligned::<u16>(
            &self.save.buf[0x2340 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()],
        ) as usize)
    }

    fn materialized(&self) -> crate::abd::MaterializedAutoBattleData {
        crate::abd::MaterializedAutoBattleData::from_wram(&self.save.buf[self.save.shift + 0x554c..][..42 * 2])
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
        self.save.buf[0x7340 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()]
            .copy_from_slice(&bytemuck::cast::<_, [u8; 2]>(count));
        true
    }

    fn set_secondary_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        if id >= super::NUM_CHIPS {
            return false;
        }
        self.save.buf[0x2340 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()]
            .copy_from_slice(&bytemuck::cast::<_, [u8; 2]>(count));
        true
    }

    fn clear_materialized(&mut self) {
        for raw in bytemuck::cast_slice_mut::<_, u16>(&mut self.save.buf[self.save.shift + 0x554c..][..42 * 2]) {
            *raw = 0xffff;
        }
    }

    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets) {
        let materialized =
            crate::abd::MaterializedAutoBattleData::materialize(&AutoBattleDataView { save: self.save }, assets);
        for (raw, v) in std::iter::zip(
            bytemuck::cast_slice_mut::<_, u16>(&mut self.save.buf[self.save.shift + 0x554c..][..42 * 2]),
            materialized.as_slice(),
        ) {
            *raw = v.unwrap_or(0xffff) as u16;
        }
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
