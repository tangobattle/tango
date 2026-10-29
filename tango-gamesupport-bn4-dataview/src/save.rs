use crate::rom::PatchCard4Effect;
use tango_gamesupport_common_dataview::save::{ChipsView as _, NavicustView as _, Save as _};

pub const SAVE_SIZE: usize = 0x73d2;
pub const MASK_OFFSET: usize = 0x1554;
pub const GAME_NAME_OFFSET: usize = 0x2208;
pub const CHECKSUM_OFFSET: usize = 0x21e8;
pub const SHIFT_OFFSET: usize = 0x1550;

pub const EREADER_NAME_OFFSET: usize = 0x1772;
pub const EREADER_NAME_SIZE: usize = 0x10;
pub const EREADER_DESCRIPTION_OFFSET: usize = 0x0522;
pub const EREADER_DESCRIPTION_SIZE: usize = 0x5c;
pub const PATCH_CARD4_SLOTS: usize = 6;

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

impl Region {
    pub const JP: Self = Region { jp: true, us: false };
    pub const US: Self = Region { jp: false, us: true };
    pub const ANY: Self = Region { jp: true, us: true };
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
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8], shift: usize) -> u32 {
    tango_gamesupport_common_dataview::save::compute_raw_checksum(buf, shift + CHECKSUM_OFFSET)
}

const SHIFTABLE_REGION_START: usize = 0x2130;
const SHIFTABLE_REGION_END: usize = 0x5e20;

fn unapply_shift(buf: &mut [u8; SAVE_SIZE]) {
    let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()]) as usize;
    buf.copy_within(
        SHIFTABLE_REGION_START + shift..SHIFTABLE_REGION_END + shift,
        SHIFTABLE_REGION_START,
    );
    for p in &mut buf[SHIFTABLE_REGION_END..][..shift] {
        *p = 0;
    }
    buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&0u32));
}

#[allow(dead_code)]
fn apply_shift(shift: usize, buf: &mut [u8; SAVE_SIZE]) {
    buf.copy_within(
        SHIFTABLE_REGION_START..SHIFTABLE_REGION_END,
        SHIFTABLE_REGION_START + shift,
    );
    for p in &mut buf[SHIFTABLE_REGION_START..][..shift] {
        *p = 0;
    }
    buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&(shift as u32)));
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, tango_gamesupport_common_dataview::save::Error> {
        let mut buf: [u8; SAVE_SIZE] = tango_gamesupport_common_dataview::save::read_image(buf, 0)?;

        tango_gamesupport_common_dataview::save::mask(&mut buf[..], MASK_OFFSET);

        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(tango_gamesupport_common_dataview::save::Error::InvalidShift(shift));
        }

        let n = &buf[shift + GAME_NAME_OFFSET..][..20];
        if n != b"ROCKMANEXE4 20031022" {
            return Err(tango_gamesupport_common_dataview::save::Error::InvalidGameName(
                n.to_vec(),
            ));
        }

        let game_info = {
            const RED_SUN: u32 = checksum_start_for_variant(Variant::RedSun);
            const BLUE_MOON: u32 = checksum_start_for_variant(Variant::BlueMoon);

            let expected_checksum =
                tango_gamesupport_common_dataview::save::read_checksum_u32(&buf, shift + CHECKSUM_OFFSET);
            let raw_checksum = compute_raw_checksum(&buf, shift);

            let (variant, region) = match expected_checksum.checked_sub(raw_checksum) {
                Some(RED_SUN) => (Variant::RedSun, Region::US),
                Some(BLUE_MOON) => (Variant::BlueMoon, Region::US),
                _ => match expected_checksum.checked_sub(raw_checksum - buf[0] as u32) {
                    Some(RED_SUN) => (Variant::RedSun, Region::JP),
                    Some(BLUE_MOON) => (Variant::BlueMoon, Region::JP),
                    Some(expected_jp_checksum) => {
                        return Err(tango_gamesupport_common_dataview::save::Error::ChecksumMismatch {
                            expected: vec![expected_checksum, expected_jp_checksum],
                            actual: raw_checksum,
                            shift,
                        });
                    }
                    _ => {
                        return Err(tango_gamesupport_common_dataview::save::Error::ChecksumMismatch {
                            expected: vec![expected_checksum],
                            actual: raw_checksum,
                            shift,
                        });
                    }
                },
            };

            GameInfo {
                variant,
                region: if buf[0] == 0 { Region::ANY } else { region },
            }
        };

        let mut save = Self { buf, game_info };
        unapply_shift(&mut save.buf);
        save.rebuild_checksum();

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, tango_gamesupport_common_dataview::save::Error> {
        let buf: [u8; SAVE_SIZE] = tango_gamesupport_common_dataview::save::read_image(buf, 0)?;

        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(tango_gamesupport_common_dataview::save::Error::InvalidShift(shift));
        }

        let mut save = Self { buf, game_info };
        unapply_shift(&mut save.buf);
        save.rebuild_checksum();

        Ok(save)
    }

    pub fn checksum(&self) -> u32 {
        tango_gamesupport_common_dataview::save::read_checksum_u32(&self.buf, CHECKSUM_OFFSET)
    }

    pub fn compute_checksum(&self) -> u32 {
        tango_gamesupport_common_dataview::save::compute_raw_checksum(&self.buf, CHECKSUM_OFFSET)
            + checksum_start_for_variant(self.game_info.variant)
            - if self.game_info.region == Region::JP {
                self.buf[0] as u32
            } else {
                0
            }
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }
}

impl tango_gamesupport_common_dataview::save::Save for Save {
    fn game_violations(
        &self,
        assets: &dyn tango_gamesupport_common_dataview::rom::Assets,
    ) -> Option<Box<dyn std::any::Any + Send + Sync>> {
        let assets = assets.underlying_any().downcast_ref::<crate::rom::Assets>()?;
        let findings = crate::build::patch_card4_violations(self, assets);
        (!findings.is_empty()).then(|| Box::new(findings) as Box<dyn std::any::Any + Send + Sync>)
    }

    fn view_chips(&self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::ChipsViewMut + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navicust(&self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::NavicustView + '_>> {
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_navicust_mut(&mut self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::NavicustViewMut + '_>> {
        Some(Box::new(NavicustView { save: self }))
    }

    fn view_auto_battle_data(
        &self,
    ) -> Option<Box<dyn tango_gamesupport_common_dataview::save::AutoBattleDataView + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn view_auto_battle_data_mut(
        &mut self,
    ) -> Option<Box<dyn tango_gamesupport_common_dataview::save::AutoBattleDataViewMut + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn view_navi(&self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::NaviView + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        tango_gamesupport_common_dataview::save::sram_dump(&self.buf, 0, Some(MASK_OFFSET))
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        tango_gamesupport_common_dataview::save::write_checksum_u32(&mut self.buf, CHECKSUM_OFFSET, checksum);
    }
}

pub struct ChipsView<S> {
    save: S,
}

const EQUIPPED_FOLDER_OFFSET: usize = 0x2132;
/// The equipped folder's Regular chip (0xff = none), which the folder
/// screen copies from that folder's own selection as it closes. Battle
/// startup only checks it against 0xff (0x8007b3e in B4WE): the Regular
/// chip itself is the folder's first stored slot. The three per-folder
/// selections immediately follow it; each is a chip index in the order
/// the folder screen shows (see [`ChipsView::raw_chip_offset`]).
const ACTIVE_REGULAR_CHIP_OFFSET: usize = 0x214c;
const REGULAR_CHIP_INDEXES_OFFSET: usize = ACTIVE_REGULAR_CHIP_OFFSET + 1;
const FOLDERS_OFFSET: usize = 0x262c;
const FOLDER_SIZE: usize = 30;

#[repr(transparent)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default, c2rust_bitfields::BitfieldStruct)]
struct RawChip {
    #[bitfield(name = "id", ty = "u16", bits = "0..=8")]
    #[bitfield(name = "code", ty = "u16", bits = "9..=15")]
    id_and_code: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x2);

impl<S: std::ops::Deref<Target = Save>> ChipsView<S> {
    /// The save offset of the chip the folder screen shows at `chip_index`.
    ///
    /// BN4 stores a folder's Regular chip in the folder's first slot, with
    /// the chips shown before it each moved one slot later: the folder
    /// screen moves every folder's Regular chip back to its index when it
    /// opens (0x802efe0 in B4WE) and to the front again when it closes
    /// (0x802ef88), and battle startup deals slot 0 as the Regular chip and
    /// shuffles only the other 29 (0x8007b34). Chip indexes here, like the
    /// Regular chip index, are in the folder screen's order.
    fn raw_chip_offset(&self, folder_index: usize, chip_index: usize) -> usize {
        let slot = match self.save.buf[REGULAR_CHIP_INDEXES_OFFSET + folder_index] as usize {
            r if r < FOLDER_SIZE && chip_index < r => chip_index + 1,
            r if r < FOLDER_SIZE && chip_index == r => 0,
            _ => chip_index,
        };
        FOLDERS_OFFSET + (folder_index * FOLDER_SIZE + slot) * std::mem::size_of::<RawChip>()
    }
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::ChipsView for ChipsView<S> {
    fn num_folders(&self) -> usize {
        3 // TODO
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[EQUIPPED_FOLDER_OFFSET] as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<Option<usize>> {
        if folder_index >= self.num_folders() {
            return None;
        }
        let idx = self.save.buf[REGULAR_CHIP_INDEXES_OFFSET + folder_index];
        Some(if idx >= 30 { None } else { Some(idx as usize) })
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<tango_gamesupport_common_dataview::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= FOLDER_SIZE {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[self.raw_chip_offset(folder_index, chip_index)..][..std::mem::size_of::<RawChip>()],
        );

        Some(tango_gamesupport_common_dataview::save::Chip {
            id: raw.id() as usize,
            code: num_traits::FromPrimitive::from_u16(raw.code())?,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= super::NUM_PACK_CHIPS || variant >= 4 {
            return None;
        }
        self.save.buf.get(0x26e4 + id * 0xc + variant).map(|&b| b as usize)
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::ChipsViewMut for ChipsView<S> {
    fn set_equipped_folder(&mut self, folder_index: usize) -> bool {
        if folder_index >= self.num_folders() {
            return false;
        }
        self.save.buf[EQUIPPED_FOLDER_OFFSET] = folder_index as u8;
        self.save.buf[ACTIVE_REGULAR_CHIP_OFFSET] = self.save.buf[REGULAR_CHIP_INDEXES_OFFSET + folder_index];
        true
    }

    fn set_chip(
        &mut self,
        folder_index: usize,
        chip_index: usize,
        chip: tango_gamesupport_common_dataview::save::Chip,
    ) -> bool {
        if folder_index >= self.num_folders() || chip_index >= FOLDER_SIZE {
            return false;
        }

        let offset = self.raw_chip_offset(folder_index, chip_index);
        self.save.buf[offset..][..std::mem::size_of::<RawChip>()].copy_from_slice(bytemuck::bytes_of(&{
            let mut raw = RawChip::default();
            raw.set_id(chip.id as u16);
            raw.set_code(chip.code as u16);
            raw
        }));

        true
    }

    fn clear_chip(&mut self, folder_index: usize, chip_index: usize) -> bool {
        if folder_index >= self.num_folders() || chip_index >= FOLDER_SIZE {
            return false;
        }

        // 0xff code reads back as an invalid ChipCode, so `chip()`
        // returns None — i.e. an empty slot.
        let offset = self.raw_chip_offset(folder_index, chip_index);
        self.save.buf[offset..][..std::mem::size_of::<RawChip>()].fill(0xff);

        true
    }

    fn set_tag_chip_indexes(&mut self, _folder_index: usize, _chip_indexes: Option<[usize; 2]>) -> bool {
        false
    }

    fn set_regular_chip_index(&mut self, folder_index: usize, chip_index: Option<usize>) -> bool {
        if folder_index >= self.num_folders() {
            return false;
        }

        // 0xff (out of the 0..30 range) reads back as "no regular".
        let raw = match chip_index {
            Some(i) if i < FOLDER_SIZE => i as u8,
            None => 0xff,
            Some(_) => return false,
        };

        // The Regular chip is stored in the folder's first slot (see
        // `raw_chip_offset`): put the old one back at its index, then move
        // the new one to the front, as the folder screen does when it
        // opens and closes.
        let old = self.regular_chip_index(folder_index).flatten();
        let chip_size = std::mem::size_of::<RawChip>();
        let folder =
            &mut self.save.buf[FOLDERS_OFFSET + folder_index * FOLDER_SIZE * chip_size..][..FOLDER_SIZE * chip_size];
        if let Some(old) = old {
            folder[..(old + 1) * chip_size].rotate_left(chip_size);
        }
        if let Some(new) = chip_index {
            folder[..(new + 1) * chip_size].rotate_right(chip_size);
        }
        self.save.buf[REGULAR_CHIP_INDEXES_OFFSET + folder_index] = raw;
        if self.equipped_folder_index() == folder_index {
            self.save.buf[ACTIVE_REGULAR_CHIP_OFFSET] = raw;
        }
        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        if id >= super::NUM_PACK_CHIPS || variant >= 4 {
            return false;
        }
        self.save.buf[0x26e4 + id * 0xc + variant] = count as u8;
        true
    }

    fn rebuild_anticheat(&mut self) {
        let mask = match self.save.game_info.variant {
            Variant::RedSun => 0x64,
            Variant::BlueMoon => 0x47,
        };
        for id in 0..0x200 {
            self.save.buf[0x58fc + id] = self.save.buf[0x0670 + id] ^ mask;
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
    col: u8,
    row: u8,
    rot: u8,
    compressed: u8,
    _unk_06: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x8);

/// The navicust color bar: 6 bytes holding the distinct part colors in
/// placement order (color encoding shared with `rom::navicust_part_color`),
/// 0-padded. Sits before the shiftable region, so the save shift doesn't
/// move it.
const NAVICUST_COLOR_BAR_OFFSET: usize = 0x190;
const NAVICUST_COLOR_BAR_LEN: usize = 6;

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::NavicustView for NavicustView<S> {
    fn size(&self) -> [usize; 2] {
        [5, 5]
    }

    fn navicust_part(&self, i: usize) -> Option<tango_gamesupport_common_dataview::save::NavicustPart> {
        if i >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.buf[0x4564 + i * std::mem::size_of::<RawNavicustPart>()..]
                [..std::mem::size_of::<RawNavicustPart>()],
        );

        if raw.id == 0 {
            return None;
        }

        Some(tango_gamesupport_common_dataview::save::NavicustPart {
            id: raw.id as usize,
            col: raw.col,
            row: raw.row,
            rot: raw.rot,
            compressed: raw.compressed != 0,
        })
    }

    fn materialized(&self) -> tango_gamesupport_common_dataview::navicust::MaterializedNavicust {
        tango_gamesupport_common_dataview::navicust::materialized_from_wram(&self.save.buf[0x4540..][..(5 * 5)], [5, 5])
    }

    fn navicust_color_bar(&self) -> Vec<Option<tango_gamesupport_common_dataview::rom::NavicustPartColor>> {
        self.save.buf[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN]
            .iter()
            .map(|&b| super::rom::navicust_part_color(b))
            .collect()
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::NavicustViewMut
    for NavicustView<S>
{
    fn set_navicust_part(
        &mut self,
        i: usize,
        part: Option<tango_gamesupport_common_dataview::save::NavicustPart>,
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
        self.save.buf[0x4564 + i * std::mem::size_of::<RawNavicustPart>()..][..std::mem::size_of::<RawNavicustPart>()]
            .copy_from_slice(bytemuck::bytes_of(&raw));

        true
    }

    fn clear_materialized(&mut self) {
        self.save.buf[0x4540..][..0x24].copy_from_slice(&[0; 0x24]);
        self.save.buf[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN]
            .copy_from_slice(&[0; NAVICUST_COLOR_BAR_LEN]);
    }

    fn rebuild_materialized(&mut self, assets: &dyn tango_gamesupport_common_dataview::rom::Assets) {
        let materialized = tango_gamesupport_common_dataview::navicust::materialize(&*self, [5, 5], assets);
        self.save.buf[0x4540..][..0x24].copy_from_slice(
            &materialized
                .into_iter()
                .map(|v| v.map(|v| v + 1).unwrap_or(0) as u8)
                .chain(std::iter::repeat(0))
                .take(0x24)
                .collect::<Vec<_>>(),
        );

        // Rebuild the color bar: distinct part colors in placement order.
        let bar = tango_gamesupport_common_dataview::navicust::materialize_color_bar(&*self, assets);
        let mut bytes = [0u8; NAVICUST_COLOR_BAR_LEN];
        for (slot, color) in bar.iter().flatten().enumerate().take(NAVICUST_COLOR_BAR_LEN) {
            bytes[slot] =
                tango_gamesupport_common_dataview::navicust::color_to_raw(color, super::rom::navicust_part_color);
        }
        self.save.buf[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN].copy_from_slice(&bytes);
    }
}
/// The six Mod Card catalog slots (0A-0F). BN4's patch cards are this
/// crate's own model -- no other game has the slot-based shape -- so the
/// view is concrete: reach it via [`Save::view_patch_card4s`] after
/// downcasting the boxed save (`AsAny`), not through a shared trait.
pub struct PatchCard4sView<S> {
    save: S,
}

impl Save {
    pub fn view_patch_card4s(&self) -> PatchCard4sView<&Save> {
        PatchCard4sView { save: self }
    }

    pub fn view_patch_card4s_mut(&mut self) -> PatchCard4sView<&mut Save> {
        PatchCard4sView { save: self }
    }
}

impl<S: std::ops::Deref<Target = Save>> PatchCard4sView<S> {
    pub fn patch_card(&self, slot: usize) -> Option<tango_gamesupport_common_dataview::save::PatchCard> {
        if slot >= PATCH_CARD4_SLOTS {
            return None;
        }

        let mut id = self.save.buf[0x464c + slot] as usize;

        let enabled = if id < super::NUM_PATCH_CARD4S {
            true
        } else {
            id = self.save.buf[0x4653 + slot] as usize;
            if id >= super::NUM_PATCH_CARD4S {
                return None;
            }
            false
        };
        Some(tango_gamesupport_common_dataview::save::PatchCard { id, enabled })
    }
}

impl<S: std::ops::DerefMut<Target = Save>> PatchCard4sView<S> {
    pub fn set_patch_card(
        &mut self,
        slot: usize,
        patch_card: Option<tango_gamesupport_common_dataview::save::PatchCard>,
    ) -> bool {
        if slot >= PATCH_CARD4_SLOTS {
            return false;
        }

        if patch_card
            .as_ref()
            .map(|p| p.id == 0 || p.id >= super::NUM_PATCH_CARD4S)
            .unwrap_or(false)
        {
            return false;
        }

        self.save.buf[0x464c + slot] = 0xff;
        self.save.buf[0x4653 + slot] = 0xff;

        if let Some(patch_card) = patch_card {
            self.save.buf[if patch_card.enabled { 0x464c } else { 0x4653 } + slot] = patch_card.id as u8;
        }

        true
    }

    pub fn rebuild_anticheat(&mut self) {
        let mask = match self.save.game_info.variant {
            Variant::BlueMoon => 0x31,
            Variant::RedSun => 0x43,
        };
        for id in 0..0x100 {
            self.save.buf[0x5d14 + id] = self.save.buf[0x03d0 + id] ^ mask;
        }
    }
}

pub struct AutoBattleDataView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::AutoBattleDataView
    for AutoBattleDataView<S>
{
    fn chip_use_count(&self, id: usize) -> Option<usize> {
        if id >= super::NUM_CHIPS {
            return None;
        }
        Some(bytemuck::pod_read_unaligned::<u16>(
            &self.save.buf[0x6f50 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()],
        ) as usize)
    }

    fn secondary_chip_use_count(&self, id: usize) -> Option<usize> {
        if id >= super::NUM_CHIPS {
            return None;
        }
        Some(bytemuck::pod_read_unaligned::<u16>(
            &self.save.buf[0x1bb0 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()],
        ) as usize)
    }

    fn materialized(&self) -> tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData {
        tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData::from_wram(
            &self.save.buf[0x5064..][..42 * std::mem::size_of::<u16>()],
        )
    }
}

impl<S: std::ops::DerefMut<Target = Save>> AutoBattleDataView<S> {
    fn set_materialized(
        &mut self,
        materialized: &tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData,
    ) {
        self.save.buf[0x5064..][..42 * std::mem::size_of::<u16>()].copy_from_slice(&bytemuck::pod_collect_to_vec(
            &materialized
                .as_slice()
                .iter()
                .map(|v| v.unwrap_or(0xffff) as u16)
                .collect::<Vec<_>>(),
        ));
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::AutoBattleDataViewMut
    for AutoBattleDataView<S>
{
    fn set_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        if id >= super::NUM_CHIPS {
            return false;
        }
        self.save.buf[0x6f50 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()]
            .copy_from_slice(bytemuck::bytes_of(&(count as u16)));
        true
    }

    fn set_secondary_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        if id >= super::NUM_CHIPS {
            return false;
        }
        self.save.buf[0x1bb0 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()]
            .copy_from_slice(bytemuck::bytes_of(&(count as u16)));
        true
    }

    fn clear_materialized(&mut self) {
        self.set_materialized(
            &tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData::empty(),
        );
    }

    fn rebuild_materialized(&mut self, assets: &dyn tango_gamesupport_common_dataview::rom::Assets) {
        let materialized = tango_gamesupport_common_dataview::auto_battle_data::MaterializedAutoBattleData::materialize(
            &*self, assets,
        );
        self.set_materialized(&materialized);
    }
}

pub struct NaviView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::NaviView for NaviView<S> {
    // BN4 has no link-navi roster; the player navi is implicit, so the id is a
    // placeholder the Navi card ignores (the ROM has no navi entry for it).
    fn navi(&self) -> usize {
        0
    }

    fn max_hp(&self, _assets: &dyn tango_gamesupport_common_dataview::rom::Assets) -> u16 {
        let mut base_max_hp =
            bytemuck::pod_read_unaligned::<u16>(&self.save.buf[0x21ca..][..std::mem::size_of::<u16>()]);

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
                        base_max_hp += *n
                    }
                }
            }
        }

        {
            let pc = self.save.view_patch_card4s();
            for slot in 0..PATCH_CARD4_SLOTS {
                let Some(card) = pc.patch_card(slot) else { continue };
                if !card.enabled {
                    continue;
                }
                let Some(info) = super::rom::patch_cards::PATCH_CARDS
                    .get(card.id)
                    .and_then(|c| c.as_ref())
                else {
                    continue;
                };
                if let PatchCard4Effect::MaxHp(n) = info.effect {
                    base_max_hp += n;
                }
            }
        }

        base_max_hp
    }

    fn folder_limits(
        &self,
        assets: &dyn tango_gamesupport_common_dataview::rom::Assets,
    ) -> tango_gamesupport_common_dataview::save::FolderLimits {
        let Some(navicust) = self.save.view_navicust() else {
            unreachable!();
        };
        let layout = assets.navicust_layout().unwrap();

        let mut mega: usize = 5;
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
                    super::rom::navicust::NavicustEffect::MegaLimit(n) => mega += *n as usize,
                    super::rom::navicust::NavicustEffect::GigaLimit(n) => giga += *n as usize,
                    _ => {}
                }
            }
        }

        let pc = self.save.view_patch_card4s();
        for i in 0..6 {
            let Some(c) = pc.patch_card(i) else {
                continue;
            };
            if !c.enabled {
                continue;
            }
            let Some(c) = super::rom::patch_cards::PATCH_CARDS.get(c.id).and_then(|c| c.as_ref()) else {
                continue;
            };
            match c.effect {
                PatchCard4Effect::MegaFolder(v) => mega += v as usize,
                PatchCard4Effect::GigaFolder(v) => giga += v as usize,
                _ => {}
            }
        }

        tango_gamesupport_common_dataview::save::FolderLimits {
            mega_limit: Some(mega.clamp(0, 10)),
            giga_limit: Some(giga.clamp(0, 10)),
            reg_memory: Some(self.save.buf[0x2148] + 4),
            max_copies: |chip| {
                // Dark chips can never enter a folder in BN4 — the game
                // only offers them in battle — so no copies are legal.
                if chip.dark() {
                    return 0;
                }
                match chip.class() {
                    tango_gamesupport_common_dataview::rom::ChipClass::Mega
                    | tango_gamesupport_common_dataview::rom::ChipClass::Giga => 1,
                    tango_gamesupport_common_dataview::rom::ChipClass::Standard => 4,
                    _ => 0,
                }
            },
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_save() -> Save {
        let mut buf = [0; SAVE_SIZE];
        buf[ACTIVE_REGULAR_CHIP_OFFSET] = 0xff;
        buf[REGULAR_CHIP_INDEXES_OFFSET..][..3].fill(0xff);
        Save::from_wram(
            &buf,
            GameInfo {
                variant: Variant::RedSun,
                region: Region::US,
            },
        )
        .unwrap()
    }

    #[test]
    fn regular_chip_keeps_the_equipped_folder_cache_in_sync() {
        let mut save = blank_save();

        {
            let mut chips = save.view_chips_mut().unwrap();

            // Editing an unequipped folder persists its selection without
            // changing the cache used by the currently equipped folder.
            assert!(chips.set_regular_chip_index(1, Some(9)));
            assert_eq!(chips.regular_chip_index(1), Some(Some(9)));
        }
        assert_eq!(save.buf[ACTIVE_REGULAR_CHIP_OFFSET], 0xff);

        {
            let mut chips = save.view_chips_mut().unwrap();

            // Equipping that folder materializes its stored selection into
            // the active byte consumed by battle startup.
            assert!(chips.set_equipped_folder(1));
        }
        assert_eq!(save.buf[ACTIVE_REGULAR_CHIP_OFFSET], 9);

        {
            let mut chips = save.view_chips_mut().unwrap();

            // Subsequent changes and clearing update both representations.
            assert!(chips.set_regular_chip_index(1, Some(4)));
        }
        assert_eq!(save.buf[ACTIVE_REGULAR_CHIP_OFFSET], 4);
        assert_eq!(save.buf[REGULAR_CHIP_INDEXES_OFFSET + 1], 4);

        {
            let mut chips = save.view_chips_mut().unwrap();
            assert!(chips.set_regular_chip_index(1, None));
        }
        assert_eq!(save.buf[ACTIVE_REGULAR_CHIP_OFFSET], 0xff);
        assert_eq!(save.buf[REGULAR_CHIP_INDEXES_OFFSET + 1], 0xff);
    }

    fn stored_ids(save: &Save, folder_index: usize, n: usize) -> Vec<u16> {
        (0..n)
            .map(|i| {
                let offset = FOLDERS_OFFSET + (folder_index * FOLDER_SIZE + i) * 2;
                u16::from_le_bytes([save.buf[offset], save.buf[offset + 1]]) & 0x1ff
            })
            .collect()
    }

    #[test]
    fn regular_chip_is_read_from_the_first_stored_slot() {
        // The folder screen shows Cannon A, LifeAura D, FullCust * with
        // FullCust as the Regular chip (index 2). The game stores FullCust
        // first and the two chips before it one slot later.
        let mut save = blank_save();
        for (i, raw) in [0x0d1u16 | 26 << 9, 0x001, 0x0c9 | 3 << 9].into_iter().enumerate() {
            save.buf[FOLDERS_OFFSET + i * 2..][..2].copy_from_slice(&raw.to_le_bytes());
        }
        save.buf[ACTIVE_REGULAR_CHIP_OFFSET] = 2;
        save.buf[REGULAR_CHIP_INDEXES_OFFSET] = 2;

        let chips = save.view_chips().unwrap();
        let chip = |i| chips.chip(0, i).map(|c| (c.id, c.code));
        assert_eq!(chips.regular_chip_index(0), Some(Some(2)));
        assert_eq!(
            [chip(0), chip(1), chip(2)],
            [
                Some((0x001, tango_gamesupport_common_dataview::save::ChipCode::A)),
                Some((0x0c9, tango_gamesupport_common_dataview::save::ChipCode::D)),
                Some((0x0d1, tango_gamesupport_common_dataview::save::ChipCode::Star)),
            ]
        );
    }

    #[test]
    fn regular_chip_edits_move_it_to_the_first_stored_slot() {
        let mut save = blank_save();
        {
            let mut chips = save.view_chips_mut().unwrap();
            for i in 0..6 {
                assert!(chips.set_chip(
                    0,
                    i,
                    tango_gamesupport_common_dataview::save::Chip {
                        id: i + 1,
                        code: tango_gamesupport_common_dataview::save::ChipCode::A,
                    },
                ));
            }
            assert!(chips.set_regular_chip_index(0, Some(4)));
        }
        assert_eq!(stored_ids(&save, 0, 6), [5, 1, 2, 3, 4, 6]);
        assert_eq!(save.buf[ACTIVE_REGULAR_CHIP_OFFSET], 4);

        {
            let mut chips = save.view_chips_mut().unwrap();
            // Reads and writes stay in the folder screen's order.
            assert_eq!(
                (0..6).map(|i| chips.chip(0, i).unwrap().id).collect::<Vec<_>>(),
                [1, 2, 3, 4, 5, 6]
            );
            assert!(chips.set_chip(
                0,
                4,
                tango_gamesupport_common_dataview::save::Chip {
                    id: 9,
                    code: tango_gamesupport_common_dataview::save::ChipCode::A,
                },
            ));
            // Moving the Regular chip puts the old one back first.
            assert!(chips.set_regular_chip_index(0, Some(1)));
        }
        assert_eq!(stored_ids(&save, 0, 6), [2, 1, 3, 4, 9, 6]);

        {
            let mut chips = save.view_chips_mut().unwrap();
            assert!(chips.set_regular_chip_index(0, None));
        }
        assert_eq!(stored_ids(&save, 0, 6), [1, 2, 3, 4, 9, 6]);
        assert_eq!(save.buf[ACTIVE_REGULAR_CHIP_OFFSET], 0xff);
    }
}
