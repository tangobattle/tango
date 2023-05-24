use byteorder::ByteOrder;

use crate::save::{self, ChipsView as _, NaviView as _, NavicustView as _, PatchCard56sView as _, Save as _};

pub const SAVE_START_OFFSET: usize = 0x0100;
pub const SAVE_SIZE: usize = 0x6710;
pub const MASK_OFFSET: usize = 0x1064;
pub const GAME_NAME_OFFSET: usize = 0x1c70;
pub const CHECKSUM_OFFSET: usize = 0x1c6c;
pub const SHIFT_OFFSET: usize = 0x1060;

pub const EREADER_DESCRIPTION_OFFSET: usize = 0x07d6;
pub const EREADER_DESCRIPTION_SIZE: usize = 0x64;

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

fn convert_jp_to_us(shift: usize, buf: &mut [u8; SAVE_SIZE]) {
    // Extend the shop data section.
    let jp_start = shift + 0x410c;
    let jp_end = shift + 0x50fc;
    buf.copy_within(jp_start..jp_end, jp_start + 0x40);
    for p in &mut buf[jp_start..jp_start + 0x40] {
        *p = 0;
    }
}

fn convert_us_to_jp(shift: usize, buf: &mut [u8; SAVE_SIZE]) {
    // Truncate the shop data section.
    let jp_start = shift + 0x410c;
    let jp_end = shift + 0x50fc;
    buf.copy_within(jp_start + 0x40..jp_end + 0x40, jp_start);
    for p in &mut buf[jp_end..jp_end + 0x40] {
        *p = 0;
    }
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, save::Error> {
        let mut buf: [u8; SAVE_SIZE] = buf
            .get(SAVE_START_OFFSET..)
            .and_then(|buf| buf.get(..SAVE_SIZE))
            .and_then(|buf| buf.try_into().ok())
            .ok_or(save::Error::InvalidSize(buf.len()))?;

        save::mask_save(&mut buf[..], MASK_OFFSET);

        let shift = *bytemuck::from_bytes::<u32>(&buf[SHIFT_OFFSET..][..4]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(save::Error::InvalidShift(shift));
        }

        let game_info = match &buf[GAME_NAME_OFFSET..][..20] {
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

        let mut save = Self { buf, shift, game_info };

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            return Err(save::Error::ChecksumMismatch {
                actual: save.checksum(),
                expected: vec![computed_checksum],
                shift,
                attempt: 0,
            });
        }

        // Saves are canonicalized into US format. This will also cause a checksum rebuild, unfortunately.
        if save.game_info.region == Region::JP {
            convert_jp_to_us(shift, &mut save.buf);
            save.rebuild_checksum();
        }

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, save::Error> {
        let shift = *bytemuck::from_bytes::<u32>(&buf[SHIFT_OFFSET..][..4]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(save::Error::InvalidShift(shift));
        }

        let buf = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(save::Error::InvalidSize(buf.len()))?;

        let mut save = Self { buf, shift, game_info };

        // Saves are canonicalized into US format. This will also cause a checksum rebuild, unfortunately.
        if save.game_info.region == Region::JP {
            convert_jp_to_us(shift, &mut save.buf);
            save.rebuild_checksum();
        }

        Ok(save)
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    pub fn checksum(&self) -> u32 {
        *bytemuck::from_bytes::<u32>(&self.buf[self.shift + CHECKSUM_OFFSET..][..4])
    }

    pub fn shift(&self) -> usize {
        self.shift
    }

    pub fn as_us_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    pub fn as_jp_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]> {
        let mut buf = self.buf.clone();
        convert_us_to_jp(self.shift, &mut buf);
        std::borrow::Cow::Owned(buf.to_vec())
    }

    pub fn compute_checksum(&self) -> u32 {
        save::compute_save_raw_checksum(&self.buf, self.shift + CHECKSUM_OFFSET)
            + match self.game_info.variant {
                Variant::Gregar => 0x72,
                Variant::Falzar => 0x18,
            }
    }

    fn navi_stats_offset(&self, id: usize) -> usize {
        self.shift + 0x47cc + 0x64 * if id == 0 { 0 } else { 1 }
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

    fn as_raw_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]> {
        match self.game_info.region {
            Region::US => self.as_us_wram(),
            Region::JP => self.as_jp_wram(),
        }
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[SAVE_START_OFFSET..][..SAVE_SIZE].copy_from_slice(&self.as_raw_wram());
        save::mask_save(&mut buf[SAVE_START_OFFSET..][..SAVE_SIZE], MASK_OFFSET);
        buf
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        byteorder::LittleEndian::write_u32(&mut self.buf[self.shift + CHECKSUM_OFFSET..][..4], checksum);
    }

    fn bugfrags(&self) -> Option<u32> {
        Some(*bytemuck::from_bytes::<u32>(&self.buf[self.shift + 0x1be0..][..4]))
    }

    fn set_bugfrags(&mut self, count: u32) -> bool {
        if count > 9999 {
            return false;
        }

        byteorder::LittleEndian::write_u32(&mut self.buf[self.shift + 0x1be0..][..4], count);

        // Anticheat...
        let mask = *bytemuck::from_bytes::<u32>(&self.buf[self.shift + 0x18b8..][..4]);
        byteorder::LittleEndian::write_u32(&mut self.buf[self.shift + 0x5030..][..4], mask ^ count);

        true
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
        let tag_chips_offset = navi_stats_offset + 0x56 + folder_index * 2;
        let idx1 = self.save.buf[tag_chips_offset + 0x00];
        let idx2 = self.save.buf[tag_chips_offset + 0x01];
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
        let raw = byteorder::LittleEndian::read_u16(&self.save.buf[offset..][..2]);

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
        self.save.buf[self.save.shift + 0x65F0] as usize
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
        self.save.buf[self.save.shift + 0x65F0] = count as u8;
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

    fn rebuild_anticheat(&mut self) {
        let mask = match self.save.game_info.variant {
            Variant::Gregar => 0x43,
            Variant::Falzar => 0x8d,
        };
        for id in 0..super::NUM_PATCH_CARD56S {
            self.save.buf[self.save.shift + 0x5088 + id] = self.save.buf[self.save.shift + 0x06c0 + id] ^ mask;
        }
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
            &mut self.save.buf[offset..][..2],
            chip.id as u16 | ((variant as u16) << 9),
        );
        true
    }

    fn set_tag_chip_indexes(&mut self, folder_index: usize, chip_indexes: Option<[usize; 2]>) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() {
            return false;
        }

        let (idx1, idx2) = if let Some([idx1, idx2]) = chip_indexes {
            if idx1 >= 30 || idx2 >= 30 {
                return false;
            }
            (idx1, idx2)
        } else {
            (0xff, 0xff)
        };

        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        let tag_chips_offset = navi_stats_offset + 0x56 + folder_index * 2;

        self.save.buf[tag_chips_offset + 0x00] = idx1 as u8;
        self.save.buf[tag_chips_offset + 0x01] = idx2 as u8;
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

    fn rebuild_anticheat(&mut self) {
        let mask = match self.save.game_info.variant {
            Variant::Gregar => 0x17,
            Variant::Falzar => 0x81,
        };

        let base_offset = self.save.shift + 0x4c20;

        for id in 0..super::NUM_CHIPS {
            self.save.buf[base_offset + id] = self.save.buf[self.save.shift + 0x08a0 + id] ^ mask;
        }
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

        let buf = &self.save.buf[self.save.shift + 0x4190 + i * 8..][..8];
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

    fn materialized(&self) -> Option<crate::navicust::MaterializedNavicust> {
        let offset = self.save.shift + 0x414c;

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

        let buf = &mut self.save.buf[self.save.shift + 0x4190 + i * 8..][..8];
        buf[0x0] = (part.id * 4 + part.variant) as u8;
        buf[0x3] = part.col as u8;
        buf[0x4] = part.row as u8;
        buf[0x5] = part.rot as u8;
        buf[0x6] = if part.compressed { 1 } else { 0 };
        true
    }

    fn clear_materialized(&mut self) {
        self.save.buf[self.save.shift + 0x4d48..][..0x44].copy_from_slice(&[0; 0x44]);
    }

    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets) {
        let materialized = crate::navicust::materialize(&NavicustView { save: self.save }, assets);
        self.save.buf[self.save.shift + 0x4d48..][..0x44].copy_from_slice(
            &materialized
                .into_iter()
                .map(|v| v.map(|v| v + 1).unwrap_or(0) as u8)
                .chain(std::iter::repeat(0))
                .take(0x44)
                .collect::<Vec<_>>(),
        )
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
