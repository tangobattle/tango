mod link_navis;

use tango_gamesupport_common::dataview::save::{
    ChipsView as _, NaviView as _, NavicustView as _, PatchCard56sView as _, Save as _,
};

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
    game_info: GameInfo,
}

const JP_SHOP_REGION_END: usize = 0x410c;
const SHIFTABLE_REGION_END: usize = 0x50fc;

fn convert_jp_to_us(buf: &mut [u8; SAVE_SIZE]) {
    // Extend the shop data section.
    buf.copy_within(JP_SHOP_REGION_END..SHIFTABLE_REGION_END, JP_SHOP_REGION_END + 0x40);
    for p in &mut buf[JP_SHOP_REGION_END..][..0x40] {
        *p = 0;
    }
}

fn convert_us_to_jp(buf: &mut [u8; SAVE_SIZE]) {
    // Truncate the shop data section.
    buf.copy_within(
        JP_SHOP_REGION_END + 0x40..SHIFTABLE_REGION_END + 0x40,
        JP_SHOP_REGION_END,
    );
    for p in &mut buf[SHIFTABLE_REGION_END..][..0x40] {
        *p = 0;
    }
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, tango_gamesupport_common::dataview::save::Error> {
        let mut buf: [u8; SAVE_SIZE] = buf
            .get(SAVE_START_OFFSET..)
            .and_then(|buf| buf.get(..SAVE_SIZE))
            .and_then(|buf| buf.try_into().ok())
            .ok_or(tango_gamesupport_common::dataview::save::Error::InvalidSize(buf.len()))?;

        tango_gamesupport_common::dataview::save::mask(&mut buf[..], MASK_OFFSET);

        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()]) as usize;
        if shift != 0 {
            return Err(tango_gamesupport_common::dataview::save::Error::InvalidShift(shift));
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
                return Err(tango_gamesupport_common::dataview::save::Error::InvalidGameName(
                    n.to_vec(),
                ));
            }
        };

        let mut save = Self { buf, game_info };

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            return Err(tango_gamesupport_common::dataview::save::Error::ChecksumMismatch {
                actual: save.checksum(),
                expected: vec![computed_checksum],
                shift,
            });
        }

        // Saves are canonicalized into US format. This will also cause a checksum rebuild, unfortunately.
        if save.game_info.region == Region::JP {
            convert_jp_to_us(&mut save.buf);
            save.rebuild_checksum();
        }

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, tango_gamesupport_common::dataview::save::Error> {
        let buf = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(tango_gamesupport_common::dataview::save::Error::InvalidSize(buf.len()))?;

        let mut save = Self { buf, game_info };

        // Saves are canonicalized into US format. This will also cause a checksum rebuild, unfortunately.
        if save.game_info.region == Region::JP {
            convert_jp_to_us(&mut save.buf);
            save.rebuild_checksum();
        }

        Ok(save)
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    pub fn as_us_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    pub fn as_jp_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        let mut buf = self.buf;
        convert_us_to_jp(&mut buf);
        std::borrow::Cow::Owned(buf.to_vec())
    }

    pub fn compute_checksum(&self) -> u32 {
        tango_gamesupport_common::dataview::save::compute_raw_checksum(&self.buf, CHECKSUM_OFFSET)
            + match self.game_info.variant {
                Variant::Gregar => 0x72,
                Variant::Falzar => 0x18,
            }
    }

    fn navi_stats_offset(&self, id: usize) -> usize {
        0x47cc + std::mem::size_of::<RawNaviStats>() * if id == 0 { 0 } else { 1 }
    }

    /// Navi `id`'s stats block, decoded.
    fn navi_stats(&self, id: usize) -> RawNaviStats {
        bytemuck::pod_read_unaligned::<RawNaviStats>(
            &self.buf[self.navi_stats_offset(id)..][..std::mem::size_of::<RawNaviStats>()],
        )
    }
}

/// The operative-navi stats block (BN6 `NaviStats`), one per slot. Field names
/// follow the game's own layout; bytes whose exact role is unconfirmed are kept
/// as `unk_*`, and undocumented gaps as `_reserved_*`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::AnyBitPattern, bytemuck::NoUninit)]
#[allow(dead_code)] // most fields are documented for completeness, not all read
struct RawNaviStats {
    unk_00: u8,
    /// Buster levels.
    attack: u8,
    speed: u8,
    charge: u8,
    b_button: u8,
    b_pwr_atk: u8,
    fst_barr: u8,
    b_left_ability: u8,
    _reserved_08: [u8; 0x1],
    /// RegMem capacity (Reg+ programs).
    reg_memory: u8,
    /// Custom-gauge / Mega / Giga folder limits.
    custom_level: u8,
    mega_limit: u8,
    giga_limit: u8,
    _reserved_0d: [u8; 0x1],
    /// Emotion/mood state.
    mood: u8,
    _reserved_0f: [u8; 0x3],
    unk_12: u8,
    _reserved_13: [u8; 0x6],
    cust_hp_bug: u8,
    _reserved_1a: [u8; 0x1],
    /// NaviCust ability flags.
    float_shoes: u8,
    air_shoes: u8,
    under_shirt: u8,
    _reserved_1e: [u8; 0x2],
    unk_20: u8,
    beast_out_counter: u8,
    _reserved_22: [u8; 0x1],
    super_armor: u8,
    emotion_bug: u8,
    humor: u8,
    _reserved_26: [u8; 0x1],
    unk_27: u8,
    _reserved_28: [u8; 0x1],
    /// Operative navi identity: 0 for MegaMan, the linked navi's id for slot 1.
    navi_id: u8,
    _reserved_2a: [u8; 0x2],
    transformation: u8,
    /// Equipped folder index.
    equipped_folder: u8,
    /// Regular-chip index per folder (0xff = none).
    regular_chip_indexes: [u8; 3],
    processing_bug: u8,
    _reserved_32: [u8; 0x3],
    slip_run: u8,
    _reserved_36: [u8; 0x3],
    a_pwr_atk: u8,
    _reserved_3a: [u8; 0x4],
    /// Base max HP, from HP Memories only (excludes NaviCust).
    base_max_hp: u16,
    /// Current HP.
    current_hp: u16,
    /// Effective max HP (base + NaviCust).
    effective_max_hp: u16,
    _reserved_44: [u8; 0x2],
    unk_46: u8,
    _reserved_47: [u8; 0x1],
    unk_48: u8,
    _reserved_49: [u8; 0x1],
    unk_4a: u8,
    _reserved_4b: [u8; 0x1],
    unk_4c: u8,
    _reserved_4d: [u8; 0x2],
    unk_4f: u8,
    chip_recovery: u16,
    _reserved_52: [u8; 0x4],
    /// Tag-chip index pairs per folder (0xff = unset).
    tag_chip_indexes: [[u8; 2]; 3],
    _reserved_5c: [u8; 0x3],
    poem: u8,
    _reserved_60: [u8; 0x3],
    turns_until_cust_bug: u8,
}
const _: () = assert!(std::mem::size_of::<RawNaviStats>() == 0x64);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, attack) == 0x01);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, b_pwr_atk) == 0x05);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, b_left_ability) == 0x07);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, reg_memory) == 0x09);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, custom_level) == 0x0a);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, mega_limit) == 0x0b);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, giga_limit) == 0x0c);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, mood) == 0x0e);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, float_shoes) == 0x1b);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, air_shoes) == 0x1c);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, super_armor) == 0x23);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, navi_id) == 0x29);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, equipped_folder) == 0x2d);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, regular_chip_indexes) == 0x2e);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, base_max_hp) == 0x3e);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, current_hp) == 0x40);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, effective_max_hp) == 0x42);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, tag_chip_indexes) == 0x56);

impl tango_gamesupport_common::dataview::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::ChipsViewMut + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navi(&self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::NaviView + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    fn view_navi_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::NaviViewMut + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    fn view_navicust(&self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::NavicustView + '_>> {
        // A link navi has no editable navicust of its own.
        if (NaviView { save: self }).navi() != 0 {
            return None;
        }
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_navicust_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::NavicustViewMut + '_>> {
        if (NaviView { save: &*self }).navi() != 0 {
            return None;
        }
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_patch_card56s(&self) -> Option<Box<dyn tango_gamesupport_common::dataview::save::PatchCard56sView + '_>> {
        if self.game_info.region != Region::JP {
            return None;
        }
        if (NaviView { save: self }).navi() != 0 {
            return None;
        }
        Some(Box::new(PatchCard56sView { save: self }))
    }

    fn view_patch_card56s_mut(
        &mut self,
    ) -> Option<Box<dyn tango_gamesupport_common::dataview::save::PatchCard56sViewMut + '_>> {
        if self.game_info.region != Region::JP {
            return None;
        }
        if (NaviView { save: &*self }).navi() != 0 {
            return None;
        }
        Some(Box::new(PatchCard56sView { save: self }))
    }

    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        match self.game_info.region {
            Region::US => self.as_us_wram(),
            Region::JP => self.as_jp_wram(),
        }
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[SAVE_START_OFFSET..][..SAVE_SIZE].copy_from_slice(&self.as_raw_wram());
        tango_gamesupport_common::dataview::save::mask(&mut buf[SAVE_START_OFFSET..][..SAVE_SIZE], MASK_OFFSET);
        buf
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&checksum));
    }
}

pub struct ChipsView<S> {
    save: S,
}

#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
struct RawChip {
    #[bitfield(name = "id", ty = "u16", bits = "0..=8")]
    #[bitfield(name = "code", ty = "u16", bits = "9..=15")]
    id_and_code: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x2);

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::ChipsView for ChipsView<S> {
    fn num_folders(&self) -> usize {
        self.save.buf[0x1c09] as usize
    }

    fn equipped_folder_index(&self) -> usize {
        self.save
            .navi_stats((NaviView { save: &*self.save }).navi())
            .equipped_folder as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<Option<usize>> {
        if folder_index >= self.num_folders() {
            return None;
        }

        let idx = self
            .save
            .navi_stats((NaviView { save: &*self.save }).navi())
            .regular_chip_indexes[folder_index];
        Some(if idx >= 30 { None } else { Some(idx as usize) })
    }

    fn tag_chip_indexes(&self, folder_index: usize) -> Option<Option<[usize; 2]>> {
        if folder_index >= self.num_folders() {
            return None;
        }

        let [idx1, idx2] = self
            .save
            .navi_stats((NaviView { save: &*self.save }).navi())
            .tag_chip_indexes[folder_index];
        Some(if idx1 == 0xff || idx2 == 0xff {
            None
        } else {
            Some([idx1 as usize, idx2 as usize])
        })
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<tango_gamesupport_common::dataview::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x2178
                + folder_index * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(tango_gamesupport_common::dataview::save::Chip {
            id: raw.id() as usize,
            code: num_traits::FromPrimitive::from_u16(raw.code())?,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= super::NUM_PACK_CHIPS {
            return None;
        }
        self.save.buf.get(0x2230 + id * 0xc + variant).map(|&b| b as usize)
    }
}

pub struct PatchCard56sView<S> {
    save: S,
}

#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
struct RawPatchCard {
    #[bitfield(name = "id", ty = "u8", bits = "0..=6")]
    #[bitfield(name = "disabled", ty = "bool", bits = "7..=7")]
    id_and_disabled: [u8; 1],
}
const _: () = assert!(std::mem::size_of::<RawPatchCard>() == 0x1);

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::PatchCard56sView
    for PatchCard56sView<S>
{
    fn count(&self) -> usize {
        self.save.buf[0x65F0] as usize
    }

    fn patch_card(&self, slot: usize) -> Option<tango_gamesupport_common::dataview::save::PatchCard> {
        if slot >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawPatchCard>(
            &self.save.buf[0x6620 + slot * std::mem::size_of::<RawPatchCard>()..]
                [..std::mem::size_of::<RawPatchCard>()],
        );

        Some(tango_gamesupport_common::dataview::save::PatchCard {
            id: raw.id() as usize,
            enabled: !raw.disabled(),
        })
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::PatchCard56sViewMut
    for PatchCard56sView<S>
{
    fn set_count(&mut self, count: usize) {
        self.save.buf[0x65F0] = count as u8;
    }

    fn set_patch_card(&mut self, slot: usize, patch_card: tango_gamesupport_common::dataview::save::PatchCard) -> bool {
        if slot >= self.count() {
            return false;
        }

        self.save.buf[0x6620 + slot..][..std::mem::size_of::<RawPatchCard>()].copy_from_slice(bytemuck::bytes_of(&{
            let mut raw = RawPatchCard::default();
            raw.set_id(patch_card.id as u8);
            raw.set_disabled(!patch_card.enabled);
            raw
        }));

        true
    }

    fn rebuild_anticheat(&mut self) {
        let mask = match self.save.game_info.variant {
            Variant::Gregar => 0x43,
            Variant::Falzar => 0x8d,
        };
        for id in 0..0x200 {
            self.save.buf[0x5038 + id] = self.save.buf[0x0670 + id] ^ mask;
        }
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::ChipsViewMut for ChipsView<S> {
    fn set_equipped_folder(&mut self, folder_index: usize) -> bool {
        if folder_index >= self.num_folders() {
            return false;
        }
        let navi = (NaviView { save: &*self.save }).navi();
        let navi_stats_offset = self.save.navi_stats_offset(navi);
        self.save.buf[navi_stats_offset + std::mem::offset_of!(RawNaviStats, equipped_folder)] = folder_index as u8;
        true
    }

    fn set_chip(
        &mut self,
        folder_index: usize,
        chip_index: usize,
        chip: tango_gamesupport_common::dataview::save::Chip,
    ) -> bool {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return false;
        }

        self.save.buf[0x2178
            + folder_index * (30 * std::mem::size_of::<RawChip>())
            + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()]
            .copy_from_slice(bytemuck::bytes_of(&{
                let mut raw = RawChip::default();
                raw.set_id(chip.id as u16);
                raw.set_code(chip.code as u16);
                raw
            }));

        true
    }

    fn clear_chip(&mut self, folder_index: usize, chip_index: usize) -> bool {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return false;
        }

        // 0xff code reads back as an invalid ChipCode, so `chip()`
        // returns None — i.e. an empty slot.
        self.save.buf[0x2178
            + folder_index * (30 * std::mem::size_of::<RawChip>())
            + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()]
            .fill(0xff);

        true
    }

    fn set_tag_chip_indexes(&mut self, folder_index: usize, chip_indexes: Option<[usize; 2]>) -> bool {
        if folder_index >= self.num_folders() {
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

        let navi = (NaviView { save: &*self.save }).navi();
        let navi_stats_offset = self.save.navi_stats_offset(navi);

        // A chip can't be both a Tag chip and the Regular chip: reject the
        // pair if either slot is this folder's Regular chip.
        if let Some([idx1, idx2]) = chip_indexes {
            let reg = self.save.buf
                [navi_stats_offset + std::mem::offset_of!(RawNaviStats, regular_chip_indexes) + folder_index]
                as usize;
            if reg == idx1 || reg == idx2 {
                return false;
            }
        }

        let tag_chips_offset =
            navi_stats_offset + std::mem::offset_of!(RawNaviStats, tag_chip_indexes) + folder_index * 2;
        self.save.buf[tag_chips_offset..][..std::mem::size_of::<[u8; 2]>()].copy_from_slice(bytemuck::bytes_of(&raw));

        true
    }

    fn set_regular_chip_index(&mut self, folder_index: usize, chip_index: Option<usize>) -> bool {
        if folder_index >= self.num_folders() {
            return false;
        }

        // 0xff (out of the 0..30 range) reads back as "no regular".
        let raw = match chip_index {
            Some(i) if i < 30 => i as u8,
            None => 0xff,
            Some(_) => return false,
        };
        let navi = (NaviView { save: &*self.save }).navi();
        let navi_stats_offset = self.save.navi_stats_offset(navi);

        // A chip can't be both the Regular chip and a Tag chip: reject if the
        // target slot is already part of this folder's Tag pair.
        if let Some(i) = chip_index {
            let tag_chips_offset =
                navi_stats_offset + std::mem::offset_of!(RawNaviStats, tag_chip_indexes) + folder_index * 2;
            let [t1, t2] = bytemuck::pod_read_unaligned::<[u8; 2]>(
                &self.save.buf[tag_chips_offset..][..std::mem::size_of::<[u8; 2]>()],
            );
            if t1 as usize == i || t2 as usize == i {
                return false;
            }
        }

        self.save.buf[navi_stats_offset + std::mem::offset_of!(RawNaviStats, regular_chip_indexes) + folder_index] =
            raw;
        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        self.save.buf[0x2230 + id * 0xc + variant] = count as u8;
        true
    }

    fn rebuild_anticheat(&mut self) {
        let mask = match self.save.game_info.variant {
            Variant::Gregar => 0x17,
            Variant::Falzar => 0x81,
        };
        for id in 0..0x200 {
            self.save.buf[0x4c20 + id] = self.save.buf[0x08a0 + id] ^ mask;
        }
    }
}

pub struct NavicustView<S> {
    save: S,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawNavicustPart {
    id: u8,
    _unk_01: u8,
    _unk_02: u8,
    col: u8,
    row: u8,
    rot: u8,
    compressed: u8,
    _unk_07: u8,
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x8);

/// The navicust color bar lives at this fixed offset: 6 bytes holding the
/// distinct part colors in placement order (color encoding shared with
/// `rom::navicust_part_color`), 0-padded.
const NAVICUST_COLOR_BAR_OFFSET: usize = 0x90;
const NAVICUST_COLOR_BAR_LEN: usize = 6;

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::NavicustView for NavicustView<S> {
    fn size(&self) -> [usize; 2] {
        [7, 7]
    }

    fn navicust_part(&self, i: usize) -> Option<tango_gamesupport_common::dataview::save::NavicustPart> {
        if i >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.buf[0x4190 + i * std::mem::size_of::<RawNavicustPart>()..]
                [..std::mem::size_of::<RawNavicustPart>()],
        );

        if raw.id == 0 {
            return None;
        }

        Some(tango_gamesupport_common::dataview::save::NavicustPart {
            id: raw.id as usize,
            col: raw.col,
            row: raw.row,
            rot: raw.rot,
            compressed: raw.compressed != 0,
        })
    }

    fn materialized(&self) -> tango_gamesupport_common::dataview::navicust::MaterializedNavicust {
        tango_gamesupport_common::dataview::navicust::materialized_from_wram(
            &self.save.buf[0x414c..][..(7 * 7)],
            [7, 7],
        )
    }

    fn navicust_color_bar(&self) -> Vec<Option<tango_gamesupport_common::dataview::rom::NavicustPartColor>> {
        self.save.buf[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN]
            .iter()
            .map(|&b| super::rom::navicust_part_color(b))
            .collect()
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::NavicustViewMut
    for NavicustView<S>
{
    fn set_navicust_part(
        &mut self,
        i: usize,
        part: Option<tango_gamesupport_common::dataview::save::NavicustPart>,
    ) -> bool {
        if i >= self.count() {
            return false;
        }
        let raw = match part {
            Some(part) => {
                if part.id >= super::NUM_NAVICUST_PARTS {
                    return false;
                }
                RawNavicustPart {
                    id: part.id as u8,
                    col: part.col,
                    row: part.row,
                    rot: part.rot,
                    compressed: if part.compressed { 1 } else { 0 },
                    ..Default::default()
                }
            }
            // An all-zero part (id 0) reads back as an empty slot.
            None => RawNavicustPart::default(),
        };
        self.save.buf[0x4190 + i * std::mem::size_of::<RawNavicustPart>()..][..std::mem::size_of::<RawNavicustPart>()]
            .copy_from_slice(bytemuck::bytes_of(&raw));

        true
    }

    fn clear_materialized(&mut self) {
        self.save.buf[0x414c..][..0x44].copy_from_slice(&[0; 0x44]);
        self.save.buf[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN]
            .copy_from_slice(&[0; NAVICUST_COLOR_BAR_LEN]);
    }

    fn rebuild_materialized(&mut self, assets: &dyn tango_gamesupport_common::dataview::rom::Assets) {
        let materialized = tango_gamesupport_common::dataview::navicust::materialize(&*self, [7, 7], assets);
        self.save.buf[0x414c..][..0x44].copy_from_slice(
            &materialized
                .into_iter()
                .map(|v| v.map(|v| v + 1).unwrap_or(0) as u8)
                .chain(std::iter::repeat(0))
                .take(0x44)
                .collect::<Vec<_>>(),
        );

        // Rebuild the color bar: distinct part colors in placement order.
        let bar = tango_gamesupport_common::dataview::navicust::materialize_color_bar(&*self, assets);
        let mut bytes = [0u8; NAVICUST_COLOR_BAR_LEN];
        for (slot, color) in bar.iter().flatten().enumerate().take(NAVICUST_COLOR_BAR_LEN) {
            bytes[slot] =
                tango_gamesupport_common::dataview::navicust::color_to_raw(color, super::rom::navicust_part_color);
        }
        self.save.buf[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN].copy_from_slice(&bytes);
    }
}

pub struct NaviView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::NaviView for NaviView<S> {
    fn navi(&self) -> usize {
        // The equip byte only takes effect while the operator flag is up
        // (see [`NaviViewMut::set_navi`]): with it down the game boots and
        // battles as MegaMan no matter what the byte says, so report what
        // the game would actually run. In particular this reads saves
        // written by the old byte-only editor as what they really are.
        if self.save.buf[0x1cb4] & 0x10 != 0 {
            self.save.buf[0x1b81] as usize
        } else {
            0
        }
    }

    fn max_hp(&self, assets: &dyn tango_gamesupport_common::dataview::rom::Assets) -> u16 {
        let navi = self.navi();
        let raw = self.save.navi_stats(navi);

        if navi != 0 {
            return raw.effective_max_hp;
        }

        let mut max_hp = raw.base_max_hp;

        if let Some(navicust) = self.save.view_navicust() {
            let grid = navicust.materialized();
            let mut seen = std::collections::HashSet::new();
            for cell in grid {
                let Some(slot) = cell else { continue };
                if !seen.insert(slot) {
                    continue;
                }
                let Some(part) = navicust.navicust_part(slot) else {
                    continue;
                };
                for effect in super::rom::navicust::navicust_part_effects(part.id) {
                    if let super::rom::navicust::NavicustEffect::MaxHp(n) = effect {
                        max_hp += *n
                    }
                }
            }
        }

        if let Some(pc) = self.save.view_patch_card56s() {
            for slot in 0..pc.count() {
                let Some(card) = pc.patch_card(slot) else { continue };
                if !card.enabled {
                    continue;
                }
                let Some(info) = assets.patch_card56(card.id) else {
                    continue;
                };
                for effect in info.effects() {
                    match effect.kind {
                        tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MaxHpMinus => {
                            max_hp = max_hp.saturating_sub(effect.parameter as u16 * 10);
                        }
                        tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MaxHpPlus => {
                            max_hp += effect.parameter as u16 * 10;
                        }
                        tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MaxHpPlusPercent => {
                            max_hp = (max_hp as u32 * (100 + effect.parameter as u32) / 100) as u16;
                        }
                        tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MaxHpMinusPercent => {
                            max_hp = (max_hp as u32 * (100 - effect.parameter as u32) / 100) as u16;
                        }
                        _ => {}
                    }
                }
            }
        }

        max_hp
    }

    fn buster_stats(
        &self,
        assets: &dyn tango_gamesupport_common::dataview::rom::Assets,
    ) -> Option<tango_gamesupport_common::dataview::save::NaviBusterStats> {
        let navi = self.navi();
        let raw = self.save.navi_stats(navi);

        if navi != 0 {
            return Some(tango_gamesupport_common::dataview::save::NaviBusterStats {
                attack: raw.attack,
                speed: raw.speed,
                charge: raw.charge,
                b_power_attack: raw.b_pwr_atk,
            });
        }

        let mut attack = 0;
        let mut speed = 0;
        let mut charge = 0;

        if navi == 0 {
            if let Some(navicust) = self.save.view_navicust() {
                let grid = navicust.materialized();
                let mut seen = std::collections::HashSet::new();
                for cell in grid {
                    let Some(slot) = cell else { continue };
                    if !seen.insert(slot) {
                        continue;
                    }
                    let Some(part) = navicust.navicust_part(slot) else {
                        continue;
                    };
                    for effect in super::rom::navicust::navicust_part_effects(part.id) {
                        use super::rom::navicust::NavicustEffect as E;
                        match effect {
                            // Buster +N programs clamp at 4 (…MAX sets 4).
                            E::Attack(n) => attack = (attack + *n).min(4),
                            E::Speed(n) => speed = (speed + *n).min(4),
                            E::Charge(n) => charge = (charge + *n).min(4),
                            E::AttackMax => attack = 4,
                            E::SpeedMax => speed = 4,
                            E::ChargeMax => charge = 4,
                            _ => {}
                        }
                    }
                }
            }

            if let Some(pc) = self.save.view_patch_card56s() {
                for slot in 0..pc.count() {
                    let Some(card) = pc.patch_card(slot) else { continue };
                    if !card.enabled {
                        continue;
                    }
                    let Some(info) = assets.patch_card56(card.id) else {
                        continue;
                    };
                    for effect in info.effects() {
                        use tango_gamesupport_common::dataview::rom::PatchCard56EffectKind as K;
                        let p = effect.parameter;
                        match effect.kind {
                            K::AttackPlus => attack = attack.saturating_add(p),
                            K::AttackMinus => attack = attack.saturating_sub(p),
                            K::AttackTimes => attack = attack.saturating_mul(p),
                            K::SpeedPlus => speed = speed.saturating_add(p),
                            K::SpeedMinus => speed = speed.saturating_sub(p),
                            K::ChargePlus => charge = charge.saturating_add(p),
                            K::ChargeMinus => charge = charge.saturating_sub(p),
                            _ => {}
                        }
                    }
                }
            }
        }

        Some(tango_gamesupport_common::dataview::save::NaviBusterStats {
            attack: attack.saturating_add(1),
            speed: speed.saturating_add(1),
            charge: charge.saturating_add(1),
            // The player's own navi fires the plain MegaBuster charge (a
            // non-signature default), so report no power attack for it; only a
            // link navi carries a signature charge shot here.
            b_power_attack: if navi == 0 { 0 } else { raw.b_pwr_atk },
        })
    }

    fn folder_limits(
        &self,
        assets: &dyn tango_gamesupport_common::dataview::rom::Assets,
    ) -> tango_gamesupport_common::dataview::save::FolderLimits {
        let (mega_limit, giga_limit, reg_memory) = match self.save.view_navicust() {
            Some(navicust) => {
                let layout = assets.navicust_layout().unwrap();

                let mut mega: isize = 5;
                let mut giga: usize = 1;

                let grid = navicust.materialized();
                let mut seen = std::collections::HashSet::new();
                for &cell in grid.row(layout.command_line).iter() {
                    let Some(slot) = cell else { continue };
                    if !seen.insert(slot) {
                        continue;
                    }
                    let Some(part) = navicust.navicust_part(slot) else {
                        continue;
                    };
                    for effect in super::rom::navicust::navicust_part_effects(part.id) {
                        match effect {
                            super::rom::navicust::NavicustEffect::MegaLimit(n) => mega += *n as isize,
                            super::rom::navicust::NavicustEffect::GigaLimit(n) => giga += *n as usize,
                            _ => {}
                        }
                    }
                }

                if let Some(pc) = self.save.view_patch_card56s() {
                    for slot in 0..pc.count() {
                        let Some(card) = pc.patch_card(slot) else { continue };
                        if !card.enabled {
                            continue;
                        }
                        let Some(info) = assets.patch_card56(card.id) else {
                            continue;
                        };
                        for effect in info.effects() {
                            match effect.kind {
                                tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MegaFolderPlus => {
                                    mega += effect.parameter as isize;
                                }
                                tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::MegaFolderMinus => {
                                    mega = mega.saturating_sub(effect.parameter as isize);
                                }
                                tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::GigaFolderPlus => {
                                    giga += effect.parameter as usize;
                                }
                                tango_gamesupport_common::dataview::rom::PatchCard56EffectKind::GigaFolderMinus => {
                                    giga = giga.saturating_sub(effect.parameter as usize);
                                }
                                _ => {}
                            }
                        }
                    }
                }

                (
                    mega.clamp(0, 10) as usize,
                    giga.clamp(0, 10),
                    // RegMem always tracks MegaMan's (slot 0) block.
                    self.save.navi_stats(0).reg_memory,
                )
            }
            None => {
                let stats = self.save.navi_stats(self.navi());
                (stats.mega_limit as usize, stats.giga_limit as usize, stats.reg_memory)
            }
        };

        tango_gamesupport_common::dataview::save::FolderLimits {
            mega_limit: Some(mega_limit),
            giga_limit: Some(giga_limit),
            reg_memory: Some(reg_memory),
            tag_memory: Some(60),
            max_copies: |chip| 6usize.saturating_sub(chip.mb() as usize / 10).clamp(1, 5),
            ..Default::default()
        }
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::NaviViewMut for NaviView<S> {
    fn set_navi(&mut self, navi: usize) -> bool {
        if navi >= link_navis::NAVI_STATS.len() {
            return false;
        }
        self.save.buf[0x1b81] = navi as u8;

        if navi == 0 {
            // Take the operator flag down and clear the equip index; slot
            // 1's stats block stays behind as residue, exactly like an
            // in-game unequip (the bundled MegaMan templates still carry
            // their last navi's block).
            self.save.buf[0x1cb4] &= !0x10;
            self.save.buf[0x1c34..][..2].copy_from_slice(&0u16.to_le_bytes());
            return true;
        }

        let stats = &link_navis::NAVI_STATS[navi];
        let off = self.save.navi_stats_offset(navi);
        self.save.buf[off..][..std::mem::size_of::<RawNaviStats>()].copy_from_slice(bytemuck::bytes_of(stats));

        // The equip byte and stats block alone are inert: the game gates
        // the whole link-navi system on the operator flag at 0x1cb4 (bit
        // 4) — without it a save boots and battles as MegaMan no matter
        // what the byte says (verified by priming edited saves into a
        // live battle). Real equips also stamp a per-navi index at 0x1c34
        // (335 + 15·id, zero for MegaMan; every bundled link-navi
        // template carries it, both regions and both variants). The flag
        // is the behavioral gate; the index is mirrored for byte-parity
        // with the game's own equips.
        self.save.buf[0x1cb4] |= 0x10;
        self.save.buf[0x1c34..][..2].copy_from_slice(&((335 + 15 * navi) as u16).to_le_bytes());

        true
    }
}
