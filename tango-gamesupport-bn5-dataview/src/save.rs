use tango_gamesupport_common::dataview::save::{
    ChipsView as _, NaviView as _, NavicustView as _, PatchCard56sView as _, Save as _,
};

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
    game_info: GameInfo,
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
                return Err(tango_gamesupport_common::dataview::save::Error::InvalidGameName(
                    n.to_vec(),
                ));
            }
        };

        let save = Self { buf, game_info };

        let computed_checksum = save.compute_checksum();
        if save.checksum() != computed_checksum {
            return Err(tango_gamesupport_common::dataview::save::Error::ChecksumMismatch {
                actual: save.checksum(),
                expected: vec![computed_checksum],
                shift,
            });
        }

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, tango_gamesupport_common::dataview::save::Error> {
        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()]) as usize;
        if shift != 0 {
            return Err(tango_gamesupport_common::dataview::save::Error::InvalidShift(shift));
        }

        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(tango_gamesupport_common::dataview::save::Error::InvalidSize(buf.len()))?,
            game_info,
        })
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    pub fn compute_checksum(&self) -> u32 {
        tango_gamesupport_common::dataview::save::compute_raw_checksum(&self.buf, CHECKSUM_OFFSET)
            + match self.game_info.variant {
                Variant::Protoman => 0x72,
                Variant::Colonel => 0x18,
            }
    }

    /// Base of navi `id`'s per-navi stats block; see [`RawNaviStats`]. Slot 0
    /// is the player; 1.. are the team link navis.
    fn navi_stats_offset(&self, id: usize) -> usize {
        0x52e6 + id * std::mem::size_of::<RawNaviStats>()
    }

    /// Navi `id`'s stats block, decoded.
    fn navi_stats(&self, id: usize) -> RawNaviStats {
        bytemuck::pod_read_unaligned::<RawNaviStats>(
            &self.buf[self.navi_stats_offset(id)..][..std::mem::size_of::<RawNaviStats>()],
        )
    }

    /// MegaMan's karma: 0 fully dark, [`KARMA_MAX`] fully light. The
    /// bundled dark templates carry 0 and the light ones 1000, which is
    /// the whole of what separates them beyond the HP the counter at
    /// [`DARK_HP_LOSSES_OFFSET`] has docked.
    pub fn karma(&self) -> u16 {
        self.navi_stats(0).karma
    }

    /// Set MegaMan's karma, clamped the way the game keeps it, and
    /// bring the anti-tamper mirror along — see [`KARMA_MIRROR_OFFSET`].
    pub fn set_karma(&mut self, karma: u16) {
        let karma = karma.min(KARMA_MAX);
        let key = bytemuck::pod_read_unaligned::<u32>(&self.buf[KARMA_KEY_OFFSET..][..4]);
        let at = self.navi_stats_offset(0) + std::mem::offset_of!(RawNaviStats, karma);
        self.buf[at..][..2].copy_from_slice(&karma.to_le_bytes());
        self.buf[KARMA_MIRROR_OFFSET..][..4].copy_from_slice(&(karma as u32 ^ key).to_le_bytes());
    }

    /// How much max HP Dark Chip use has cost this save — see
    /// [`DARK_HP_LOSSES_OFFSET`]. The HP itself is the stats blocks'
    /// business; this is only the counter the game stops docking at.
    pub fn dark_hp_losses(&self) -> u16 {
        bytemuck::pod_read_unaligned::<u16>(&self.buf[DARK_HP_LOSSES_OFFSET..][..2])
    }

    /// Set the Dark Chip HP-loss counter.
    pub fn set_dark_hp_losses(&mut self, losses: u16) {
        self.buf[DARK_HP_LOSSES_OFFSET..][..2].copy_from_slice(&losses.to_le_bytes());
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::AnyBitPattern)]
#[allow(dead_code)] // reserved/HP fields are mapped for documentation, not all read
struct RawNaviStats {
    /// Base max HP, from HP Memories only (excludes NaviCust).
    base_max_hp: u16,
    /// Current HP.
    current_hp: u16,
    effective_max_hp: u16,
    /// MegaMan's karma in slot 0 (0 fully dark, 1000 fully light — see
    /// [`Save::karma`]); link navis leave it alone.
    karma: u16,
    /// 0x08..0x60, unmapped.
    _rest: [u8; 0x58],
}
const _: () = assert!(std::mem::size_of::<RawNaviStats>() == 0x60);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, base_max_hp) == 0x00);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, current_hp) == 0x02);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, effective_max_hp) == 0x04);
const _: () = assert!(std::mem::offset_of!(RawNaviStats, karma) == 0x06);

/// Where the karma clamp stops: fully light.
pub const KARMA_MAX: u16 = 1000;

/// Karma's anti-tamper mirror: a u32 the game keeps equal to
/// `karma ^ key`, with the key the u32 at [`KARMA_KEY_OFFSET`]. The
/// game's own writer/verifier pair (US Protoman `0x080064d8` /
/// `0x080064f2`, reached through the record getter at `0x08010dc8`)
/// reads karma out of navi record 0 at `+0x44`, XORs the key over it
/// and keeps the result here, at the head of the save's last section —
/// so a karma write has to land in both places or the save reads as
/// tampered.
const KARMA_MIRROR_OFFSET: usize = 0x61e0;
const KARMA_KEY_OFFSET: usize = 0x2338;

/// How many times using Dark Chips has cost the save a point of max
/// HP: a u16 at `+0x16` of the section at 0x29ac. The game bumps it
/// after a dark battle until it reaches 499 and docks base max HP by
/// one alongside, which is why a dark save's MegaMan reads 1000 minus
/// this.
const DARK_HP_LOSSES_OFFSET: usize = 0x29c2;

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
        if (NaviView { save: self }).navi() != 0 {
            return None;
        }
        Some(Box::new(PatchCard56sView { save: self }))
    }

    fn view_patch_card56s_mut(
        &mut self,
    ) -> Option<Box<dyn tango_gamesupport_common::dataview::save::PatchCard56sViewMut + '_>> {
        if (NaviView { save: &*self }).navi() != 0 {
            return None;
        }
        Some(Box::new(PatchCard56sView { save: self }))
    }

    fn view_auto_battle_data(
        &self,
    ) -> Option<Box<dyn tango_gamesupport_common::dataview::save::AutoBattleDataView + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn view_auto_battle_data_mut(
        &mut self,
    ) -> Option<Box<dyn tango_gamesupport_common::dataview::save::AutoBattleDataViewMut + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[SAVE_START_OFFSET..][..SAVE_SIZE].copy_from_slice(&self.buf);
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
        3 // TODO
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[0x52d5] as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<Option<usize>> {
        let idx = self.save.buf[0x52d6 + folder_index];
        Some(if idx >= 30 { None } else { Some(idx as usize) })
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<tango_gamesupport_common::dataview::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x2df4
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
        self.save.buf.get(0x2eac + id * 0xc + variant).map(|&b| b as usize)
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
        self.save.buf[0x79a0] as usize
    }

    fn patch_card(&self, slot: usize) -> Option<tango_gamesupport_common::dataview::save::PatchCard> {
        if slot >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawPatchCard>(
            &self.save.buf[0x79d0 + slot * std::mem::size_of::<RawPatchCard>()..]
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
        self.save.buf[0x79a0] = count as u8;
    }

    fn set_patch_card(&mut self, slot: usize, patch_card: tango_gamesupport_common::dataview::save::PatchCard) -> bool {
        if slot >= self.count() {
            return false;
        }

        self.save.buf[0x79d0 + slot..][..std::mem::size_of::<RawPatchCard>()].copy_from_slice(bytemuck::bytes_of(&{
            let mut raw = RawPatchCard::default();
            raw.set_id(patch_card.id as u8);
            raw.set_disabled(!patch_card.enabled);
            raw
        }));

        true
    }

    fn rebuild_anticheat(&mut self) {
        let mask = match self.save.game_info.variant {
            Variant::Protoman => 0x43,
            Variant::Colonel => 0x8d,
        };
        for id in 0..0x200 {
            self.save.buf[0x60dc + id] = self.save.buf[0x1220 + id] ^ mask;
        }
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::ChipsViewMut for ChipsView<S> {
    fn set_equipped_folder(&mut self, folder_index: usize) -> bool {
        if folder_index >= self.num_folders() {
            return false;
        }
        self.save.buf[0x52d5] = folder_index as u8;
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

        self.save.buf[0x2df4
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
        self.save.buf[0x2df4
            + folder_index * (30 * std::mem::size_of::<RawChip>())
            + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()]
            .fill(0xff);

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
            Some(i) if i < 30 => i as u8,
            None => 0xff,
            Some(_) => return false,
        };
        self.save.buf[0x52d6 + folder_index] = raw;
        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        self.save.buf[0x2eac + id * 0xc + variant] = count as u8;
        true
    }

    fn rebuild_anticheat(&mut self) {
        let mask = match self.save.game_info.variant {
            Variant::Protoman => 0x17,
            Variant::Colonel => 0x81,
        };
        for id in 0..0x200 {
            self.save.buf[0x5cc4 + id] = self.save.buf[0x1440 + id] ^ mask;
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
/// 0-padded.
const NAVICUST_COLOR_BAR_OFFSET: usize = 0x1e8;
const NAVICUST_COLOR_BAR_LEN: usize = 6;

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::NavicustView for NavicustView<S> {
    fn size(&self) -> [usize; 2] {
        [5, 5]
    }

    fn navicust_part(&self, i: usize) -> Option<tango_gamesupport_common::dataview::save::NavicustPart> {
        if i >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.buf[0x4d6c + i * std::mem::size_of::<RawNavicustPart>()..]
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
            &self.save.buf[0x4d48..][..(5 * 5)],
            [5, 5],
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
        self.save.buf[0x4d6c + i * std::mem::size_of::<RawNavicustPart>()..][..std::mem::size_of::<RawNavicustPart>()]
            .copy_from_slice(bytemuck::bytes_of(&raw));

        true
    }

    fn clear_materialized(&mut self) {
        self.save.buf[0x4d48..][..0x24].copy_from_slice(&[0; 0x24]);
        self.save.buf[NAVICUST_COLOR_BAR_OFFSET..][..NAVICUST_COLOR_BAR_LEN]
            .copy_from_slice(&[0; NAVICUST_COLOR_BAR_LEN]);
    }

    fn rebuild_materialized(&mut self, assets: &dyn tango_gamesupport_common::dataview::rom::Assets) {
        let materialized = tango_gamesupport_common::dataview::navicust::materialize(&*self, [5, 5], assets);
        self.save.buf[0x4d48..][..0x24].copy_from_slice(
            &materialized
                .into_iter()
                .map(|v| v.map(|v| v + 1).unwrap_or(0) as u8)
                .chain(std::iter::repeat(0))
                .take(0x24)
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

pub struct AutoBattleDataView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::AutoBattleDataView
    for AutoBattleDataView<S>
{
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

    fn materialized(&self) -> tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData {
        tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData::from_wram(
            &self.save.buf[0x554c..][..42 * std::mem::size_of::<u16>()],
        )
    }
}

impl<S: std::ops::DerefMut<Target = Save>> AutoBattleDataView<S> {
    fn set_materialized(
        &mut self,
        materialized: &tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData,
    ) {
        self.save.buf[0x554c..][..42 * std::mem::size_of::<u16>()].copy_from_slice(&bytemuck::pod_collect_to_vec(
            &materialized
                .as_slice()
                .iter()
                .map(|v| v.unwrap_or(0xffff) as u16)
                .collect::<Vec<_>>(),
        ));
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::AutoBattleDataViewMut
    for AutoBattleDataView<S>
{
    fn set_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        if id >= super::NUM_CHIPS {
            return false;
        }
        self.save.buf[0x7340 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()]
            .copy_from_slice(bytemuck::bytes_of(&(count as u16)));
        true
    }

    fn set_secondary_chip_use_count(&mut self, id: usize, count: usize) -> bool {
        if id >= super::NUM_CHIPS {
            return false;
        }
        self.save.buf[0x2340 + id * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()]
            .copy_from_slice(bytemuck::bytes_of(&(count as u16)));
        true
    }

    fn clear_materialized(&mut self) {
        self.set_materialized(
            &tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData::empty(),
        );
    }

    fn rebuild_materialized(&mut self, assets: &dyn tango_gamesupport_common::dataview::rom::Assets) {
        let materialized =
            tango_gamesupport_common::dataview::auto_battle_data::MaterializedAutoBattleData::materialize(
                &*self, assets,
            );
        self.set_materialized(&materialized);
    }
}

pub struct NaviView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common::dataview::save::NaviView for NaviView<S> {
    fn navi(&self) -> usize {
        self.save.buf[0x2941] as usize
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

    fn folder_limits(
        &self,
        assets: &dyn tango_gamesupport_common::dataview::rom::Assets,
    ) -> tango_gamesupport_common::dataview::save::FolderLimits {
        let Some(navicust) = self.save.view_navicust() else {
            return tango_gamesupport_common::dataview::save::FolderLimits::default();
        };
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

        tango_gamesupport_common::dataview::save::FolderLimits {
            mega_limit: Some(mega.clamp(0, 10) as usize),
            giga_limit: Some(giga.clamp(0, 10)),
            dark_limit: Some(3),
            reg_memory: Some(self.save.buf[0x52b1]),
            max_copies: |chip| {
                if chip.dark() {
                    return 1;
                }
                match chip.class() {
                    tango_gamesupport_common::dataview::rom::ChipClass::Mega
                    | tango_gamesupport_common::dataview::rom::ChipClass::Giga => 1,
                    tango_gamesupport_common::dataview::rom::ChipClass::Standard => 4,
                    _ => 0,
                }
            },
            ..Default::default()
        }
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common::dataview::save::NaviViewMut for NaviView<S> {
    fn set_navi(&mut self, navi: usize) -> bool {
        self.save.buf[0x2941] = navi as u8;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Karma reads off navi record 0, and a write keeps the anti-tamper
    /// mirror equal to `karma ^ key` — the relation the game verifies.
    #[test]
    fn karma_round_trips_with_its_mirror() {
        let mut buf = vec![0u8; SAVE_SIZE];
        buf[KARMA_KEY_OFFSET..][..4].copy_from_slice(&0x0800_0000u32.to_le_bytes());
        let mut save = Save::from_wram(
            &buf,
            GameInfo {
                region: Region::US,
                variant: Variant::Protoman,
            },
        )
        .unwrap();

        save.set_karma(1000);
        assert_eq!(save.karma(), 1000);
        let mirror = bytemuck::pod_read_unaligned::<u32>(&save.buf[KARMA_MIRROR_OFFSET..][..4]);
        assert_eq!(mirror, 0x0800_0000 ^ 1000);

        // The clamp is the game's own.
        save.set_karma(u16::MAX);
        assert_eq!(save.karma(), KARMA_MAX);

        save.set_dark_hp_losses(3);
        assert_eq!(save.dark_hp_losses(), 3);
    }
}
