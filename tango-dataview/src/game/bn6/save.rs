use crate::save::{ChipsView as _, NaviView as _, NavicustView as _, PatchCard56sView as _, Save as _};

pub const SAVE_START_OFFSET: usize = 0x0100;
pub const SAVE_SIZE: usize = 0x6710;
pub const MASK_OFFSET: usize = 0x1064;
pub const GAME_NAME_OFFSET: usize = 0x1c70;
pub const CHECKSUM_OFFSET: usize = 0x1c6c;
pub const SHIFT_OFFSET: usize = 0x1060;

pub const EREADER_NAME_OFFSET: usize = 0x1186;
pub const EREADER_NAME_SIZE: usize = 0x18;
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
    pub fn new(buf: &[u8]) -> Result<Self, crate::save::Error> {
        let mut buf: [u8; SAVE_SIZE] = buf
            .get(SAVE_START_OFFSET..)
            .and_then(|buf| buf.get(..SAVE_SIZE))
            .and_then(|buf| buf.try_into().ok())
            .ok_or(crate::save::Error::InvalidSize(buf.len()))?;

        crate::save::mask_save(&mut buf[..], MASK_OFFSET);

        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()]) as usize;
        if shift != 0 {
            return Err(crate::save::Error::InvalidShift(shift));
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
                return Err(crate::save::Error::InvalidGameName(n.to_vec()));
            }
        };

        let mut save = Self { buf, shift, game_info };

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            return Err(crate::save::Error::ChecksumMismatch {
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

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, crate::save::Error> {
        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()]) as usize;
        if shift != 0 {
            return Err(crate::save::Error::InvalidShift(shift));
        }

        let buf = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(crate::save::Error::InvalidSize(buf.len()))?;

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
        bytemuck::pod_read_unaligned::<u32>(&self.buf[self.shift + CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
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
        crate::save::compute_save_raw_checksum(&self.buf, self.shift + CHECKSUM_OFFSET)
            + match self.game_info.variant {
                Variant::Gregar => 0x72,
                Variant::Falzar => 0x18,
            }
    }

    fn navi_stats_offset(&self, id: usize) -> usize {
        self.shift + 0x47cc + 0x64 * if id == 0 { 0 } else { 1 }
    }
}

impl crate::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn crate::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn crate::save::ChipsViewMut + '_>> {
        Some(Box::new(ChipsViewMut { save: self }))
    }

    fn view_navicust(&self) -> Option<Box<dyn crate::save::NavicustView + '_>> {
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_navicust_mut(&mut self) -> Option<Box<dyn crate::save::NavicustViewMut + '_>> {
        Some(Box::new(NavicustViewMut { save: self }))
    }

    fn view_patch_cards(&self) -> Option<crate::save::PatchCardsView> {
        if self.game_info.region != Region::JP {
            return None;
        }
        Some(crate::save::PatchCardsView::PatchCard56s(Box::new(PatchCard56sView {
            save: self,
        })))
    }

    fn view_patch_cards_mut(&mut self) -> Option<crate::save::PatchCardsViewMut> {
        if self.game_info.region != Region::JP {
            return None;
        }
        Some(crate::save::PatchCardsViewMut::PatchCard56s(Box::new(
            PatchCard56sViewMut { save: self },
        )))
    }

    // fn view_navi(&self) -> Option<Box<dyn crate::save::NaviView + '_>> {
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
        crate::save::mask_save(&mut buf[SAVE_START_OFFSET..][..SAVE_SIZE], MASK_OFFSET);
        buf
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&checksum));
    }

    fn bugfrags(&self) -> Option<u32> {
        Some(bytemuck::pod_read_unaligned::<u32>(
            &self.buf[self.shift + 0x1be0..][..std::mem::size_of::<u32>()],
        ))
    }

    fn set_bugfrags(&mut self, count: u32) -> bool {
        if count > 9999 {
            return false;
        }

        self.buf[self.shift + 0x1be0..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&count));

        // Anticheat...
        let mask = bytemuck::pod_read_unaligned::<u32>(&self.buf[self.shift + 0x18b8..][..std::mem::size_of::<u32>()]);
        self.buf[self.shift + 0x5030..][..std::mem::size_of::<u32>()]
            .copy_from_slice(bytemuck::bytes_of(&(mask ^ count)));

        true
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
struct RawChip {
    #[bitfield(name = "id", ty = "u16", bits = "0..=8")]
    #[bitfield(name = "variant", ty = "u16", bits = "9..=15")]
    id_and_variant: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x2);

impl<'a> crate::save::ChipsView<'a> for ChipsView<'a> {
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
        let [idx1, idx2] = bytemuck::pod_read_unaligned::<[u8; 2]>(
            &self.save.buf[tag_chips_offset..][..std::mem::size_of::<[u8; 2]>()],
        );
        if idx1 == 0xff || idx2 == 0xff {
            None
        } else {
            Some([idx1 as usize, idx2 as usize])
        }
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<crate::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[self.save.shift
                + 0x2178
                + folder_index * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(crate::save::Chip {
            id: raw.id() as usize,
            code: b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[raw.variant() as usize] as char,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        Some(self.save.buf[self.save.shift + 0x2230 + id * 0xc + variant] as usize)
    }
}

pub struct PatchCard56sView<'a> {
    save: &'a Save,
}

#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
struct RawPatchCard {
    #[bitfield(name = "id", ty = "u8", bits = "0..=6")]
    #[bitfield(name = "disabled", ty = "bool", bits = "7..=7")]
    id_and_disabled: [u8; 1],
}
const _: () = assert!(std::mem::size_of::<RawPatchCard>() == 0x1);

impl<'a> crate::save::PatchCard56sView<'a> for PatchCard56sView<'a> {
    fn count(&self) -> usize {
        self.save.buf[self.save.shift + 0x65F0] as usize
    }

    fn patch_card(&self, slot: usize) -> Option<crate::save::PatchCard> {
        if slot >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawPatchCard>(
            &self.save.buf[self.save.shift + 0x6620 + slot * std::mem::size_of::<RawPatchCard>()..]
                [..std::mem::size_of::<RawPatchCard>()],
        );

        Some(crate::save::PatchCard {
            id: raw.id() as usize,
            enabled: !raw.disabled(),
        })
    }
}

pub struct PatchCard56sViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> crate::save::PatchCard56sViewMut<'a> for PatchCard56sViewMut<'a> {
    fn set_count(&mut self, count: usize) {
        self.save.buf[self.save.shift + 0x65F0] = count as u8;
    }

    fn set_patch_card(&mut self, slot: usize, patch_card: crate::save::PatchCard) -> bool {
        let view = PatchCard56sView { save: self.save };
        if slot >= view.count() {
            return false;
        }

        self.save.buf[self.save.shift + 0x6620 + slot..][..std::mem::size_of::<RawPatchCard>()].copy_from_slice(
            bytemuck::bytes_of(&{
                let mut raw = RawPatchCard::default();
                raw.set_id(patch_card.id as u8);
                raw.set_disabled(!patch_card.enabled);
                raw
            }),
        );

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

impl<'a> crate::save::ChipsViewMut<'a> for ChipsViewMut<'a> {
    fn set_equipped_folder(&mut self, folder_index: usize) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() {
            return false;
        }
        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        self.save.buf[navi_stats_offset + 0x2d] = folder_index as u8;
        true
    }

    fn set_chip(&mut self, folder_index: usize, chip_index: usize, chip: crate::save::Chip) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() || chip_index >= 30 {
            return false;
        }

        let variant = if let Some(variant) = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"
            .iter()
            .position(|c| *c == chip.code as u8)
        {
            variant
        } else {
            return false;
        };

        self.save.buf[self.save.shift
            + 0x2178
            + folder_index * (30 * std::mem::size_of::<RawChip>())
            + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()]
            .copy_from_slice(bytemuck::bytes_of(&{
                let mut raw = RawChip::default();
                raw.set_id(chip.id as u16);
                raw.set_variant(variant as u16);
                raw
            }));

        true
    }

    fn set_tag_chip_indexes(&mut self, folder_index: usize, chip_indexes: Option<[usize; 2]>) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() {
            return false;
        }

        let raw = if let Some([idx1, idx2]) = chip_indexes {
            if idx1 >= 30 || idx2 >= 30 {
                return false;
            }
            [idx1 as u8, idx2 as u8]
        } else {
            [0xff, 0xff]
        };

        let navi_stats_offset = self.save.navi_stats_offset(NaviView { save: self.save }.navi());
        let tag_chips_offset = navi_stats_offset + 0x56 + folder_index * 2;

        self.save.buf[tag_chips_offset..][..std::mem::size_of::<[u8; 2]>()].copy_from_slice(bytemuck::bytes_of(&raw));

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

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
struct RawNavicustPart {
    #[bitfield(name = "variant", ty = "u8", bits = "0..=1")]
    #[bitfield(name = "id", ty = "u8", bits = "2..=7")]
    id_and_variant: [u8; 1],
    _unk_01: u8,
    _unk_02: u8,
    col: u8,
    row: u8,
    rot: u8,
    compressed: u8,
    _unk_07: u8,
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x8);

impl<'a> crate::save::NavicustView<'a> for NavicustView<'a> {
    fn width(&self) -> usize {
        7
    }

    fn height(&self) -> usize {
        7
    }

    fn navicust_part(&self, i: usize) -> Option<crate::save::NavicustPart> {
        if i >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.buf[self.save.shift + 0x4190 + i * std::mem::size_of::<RawNavicustPart>()..]
                [..std::mem::size_of::<RawNavicustPart>()],
        );

        if raw.id() == 0 {
            return None;
        }

        Some(crate::save::NavicustPart {
            id: raw.id() as usize,
            variant: raw.variant() as usize,
            col: raw.col,
            row: raw.row,
            rot: raw.rot,
            compressed: raw.compressed != 0,
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

impl<'a> crate::save::NavicustViewMut<'a> for NavicustViewMut<'a> {
    fn set_navicust_part(&mut self, i: usize, part: crate::save::NavicustPart) -> bool {
        if part.id >= super::NUM_NAVICUST_PARTS.0 || part.variant >= super::NUM_NAVICUST_PARTS.1 {
            return false;
        }
        if i >= (NavicustView { save: self.save }).count() {
            return false;
        }

        self.save.buf[self.save.shift + 0x4190 + i * std::mem::size_of::<RawNavicustPart>()..]
            [..std::mem::size_of::<RawNavicustPart>()]
            .copy_from_slice(bytemuck::bytes_of(&{
                let mut raw = RawNavicustPart {
                    col: part.col,
                    row: part.row,
                    rot: part.rot,
                    compressed: if part.compressed { 1 } else { 0 },
                    ..Default::default()
                };
                raw.set_id(part.id as u8);
                raw.set_variant(part.variant as u8);
                raw
            }));

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

impl<'a> crate::save::NaviView<'a> for NaviView<'a> {
    fn navi(&self) -> usize {
        self.save.buf[self.save.shift + 0x1b81] as usize
    }
}
