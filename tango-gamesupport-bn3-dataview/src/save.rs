use bitvec::view::BitView;

use crate::rom::extra_ncp_color;
use tango_gamesupport_common_dataview::save::{ChipsView as _, Save as _};

pub const SAVE_SIZE: usize = 0x57b0;
pub const GAME_NAME_OFFSET: usize = 0x1e00;
pub const CHECKSUM_OFFSET: usize = 0x1dd8;

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Variant {
    White,
    Blue,
}

const fn checksum_start_for_variant(variant: Variant) -> u32 {
    match variant {
        Variant::White => 0x16,
        Variant::Blue => 0x22,
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct GameInfo {
    pub variant: Variant,
}

#[derive(Clone)]
pub struct Save {
    buf: [u8; SAVE_SIZE],
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8]) -> u32 {
    tango_gamesupport_common_dataview::save::compute_raw_checksum(buf, CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, tango_gamesupport_common_dataview::save::Error> {
        let buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(tango_gamesupport_common_dataview::save::Error::InvalidSize(buf.len()))?;

        let n = &buf[GAME_NAME_OFFSET..][..20];
        if n != b"ROCKMANEXE3 20021002" && n != b"BBN3 v0.5.0 20021002" {
            return Err(tango_gamesupport_common_dataview::save::Error::InvalidGameName(
                n.to_vec(),
            ));
        }

        let save_checksum = bytemuck::pod_read_unaligned::<u32>(&buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()]);
        let raw_checksum = compute_raw_checksum(&buf);
        let game_info = {
            const WHITE: u32 = checksum_start_for_variant(Variant::White);
            const BLUE: u32 = checksum_start_for_variant(Variant::Blue);
            GameInfo {
                variant: match save_checksum.checked_sub(raw_checksum) {
                    Some(WHITE) => Variant::White,
                    Some(BLUE) => Variant::Blue,
                    _ => {
                        return Err(tango_gamesupport_common_dataview::save::Error::ChecksumMismatch {
                            actual: save_checksum,
                            expected: vec![raw_checksum + WHITE, raw_checksum + BLUE],
                            shift: 0,
                        });
                    }
                },
            }
        };

        let save = Self { buf, game_info };

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, tango_gamesupport_common_dataview::save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(tango_gamesupport_common_dataview::save::Error::InvalidSize(buf.len()))?,
            game_info,
        })
    }

    #[allow(dead_code)]
    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    #[allow(dead_code)]
    pub fn compute_checksum(&self) -> u32 {
        compute_raw_checksum(&self.buf) + checksum_start_for_variant(self.game_info.variant)
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }

    fn flag(&self, i: usize) -> bool {
        self.buf[0x0030 + i / 8].view_bits::<bitvec::order::Msb0>()[i % 8]
    }

    #[allow(dead_code)]
    fn set_flag(&mut self, i: usize, v: bool) {
        self.buf[0x0030 + i / 8]
            .view_bits_mut::<bitvec::order::Msb0>()
            .set(i % 8, v)
    }
}

impl tango_gamesupport_common_dataview::save::Save for Save {
    fn view_navi(&self) -> Option<Box<dyn tango_gamesupport_common_dataview::save::NaviView + '_>> {
        Some(Box::new(NaviView { save: self }))
    }

    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
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

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SAVE_SIZE].copy_from_slice(&self.buf);
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

const EQUIPPED_FOLDER_OFFSET: usize = 0x1882;
/// The Regular-chip slot the game uses for the equipped folder in battle.
/// The three persistent per-folder selections immediately follow it.
const ACTIVE_REGULAR_CHIP_OFFSET: usize = 0x189c;
const REGULAR_CHIP_INDEXES_OFFSET: usize = ACTIVE_REGULAR_CHIP_OFFSET + 1;

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawChip {
    id: u16,
    code: u16,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x4);

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
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x1410
                + folder_index * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(tango_gamesupport_common_dataview::save::Chip {
            id: raw.id as usize,
            code: num_traits::FromPrimitive::from_u16(raw.code)?,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= super::NUM_PACK_CHIPS || variant >= 6 {
            return None;
        }
        // counts-first record: buf[base + id*0x12 + variant], variant = code position.
        // Unused code slots hold 0xff padding; a real count never exceeds 99, so
        // treat anything larger as "not owned".
        self.save
            .buf
            .get(0x1f60 + id * 0x12 + variant)
            .map(|&b| if b <= 99 { b as usize } else { 0 })
    }
}

impl<S: std::ops::DerefMut<Target = Save>> tango_gamesupport_common_dataview::save::ChipsViewMut for ChipsView<S> {
    fn set_chip(
        &mut self,
        folder_index: usize,
        chip_index: usize,
        chip: tango_gamesupport_common_dataview::save::Chip,
    ) -> bool {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return false;
        }

        self.save.buf[0x1410
            + folder_index * (30 * std::mem::size_of::<RawChip>())
            + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()]
            .copy_from_slice(bytemuck::bytes_of(&RawChip {
                id: chip.id as u16,
                code: chip.code as u16,
            }));

        true
    }

    fn clear_chip(&mut self, folder_index: usize, chip_index: usize) -> bool {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return false;
        }

        // 0xffff code reads back as an invalid ChipCode, so `chip()` returns None.
        self.save.buf[0x1410
            + folder_index * (30 * std::mem::size_of::<RawChip>())
            + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()]
            .fill(0xff);

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
        self.save.buf[REGULAR_CHIP_INDEXES_OFFSET + folder_index] = raw;
        if self.equipped_folder_index() == folder_index {
            self.save.buf[ACTIVE_REGULAR_CHIP_OFFSET] = raw;
        }
        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        if id >= super::NUM_PACK_CHIPS || variant >= 6 {
            return false;
        }
        if let Some(b) = self.save.buf.get_mut(0x1f60 + id * 0x12 + variant) {
            *b = count as u8;
            true
        } else {
            false
        }
    }

    fn rebuild_anticheat(&mut self) {
        // BN3 has no anti-cheat shadow copy (introduced in BN4).
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_save(equipped_folder_index: usize) -> Save {
        let mut buf = [0; SAVE_SIZE];
        buf[EQUIPPED_FOLDER_OFFSET] = equipped_folder_index as u8;
        buf[ACTIVE_REGULAR_CHIP_OFFSET] = 0xff;
        buf[REGULAR_CHIP_INDEXES_OFFSET..][..3].fill(0xff);
        Save::from_wram(
            &buf,
            GameInfo {
                variant: Variant::White,
            },
        )
        .unwrap()
    }

    #[test]
    fn regular_chip_keeps_the_equipped_folder_cache_in_sync() {
        let mut save = blank_save(1);

        {
            let mut chips = save.view_chips_mut().unwrap();

            // Editing an unequipped folder must not change the cache used by
            // the currently equipped folder.
            assert!(chips.set_regular_chip_index(0, Some(9)));
            assert_eq!(chips.regular_chip_index(0), Some(Some(9)));
        }
        assert_eq!(save.buf[ACTIVE_REGULAR_CHIP_OFFSET], 0xff);

        {
            let mut chips = save.view_chips_mut().unwrap();
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
    _unk_05: [u8; 3],
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x8);

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::NavicustView for NavicustView<S> {
    fn size(&self) -> [usize; 2] {
        [5, 5]
    }

    fn style(&self) -> Option<usize> {
        Some((self.save.buf[0x1881] & 0x3f) as usize)
    }

    fn navicust_part(&self, i: usize) -> Option<tango_gamesupport_common_dataview::save::NavicustPart> {
        if i >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.buf[0x1300 + i * std::mem::size_of::<RawNavicustPart>()..]
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
            compressed: self.save.flag(0x02e0 + raw.id as usize),
        })
    }

    fn materialized(&self) -> tango_gamesupport_common_dataview::navicust::MaterializedNavicust {
        tango_gamesupport_common_dataview::navicust::materialized_from_wram(&self.save.buf[0x1d90..][..(5 * 5)], [5, 5])
    }

    fn navicust_color_bar(&self) -> Vec<Option<tango_gamesupport_common_dataview::rom::NavicustPartColor>> {
        vec![
            Some(tango_gamesupport_common_dataview::rom::NavicustPartColor::White),
            Some(tango_gamesupport_common_dataview::rom::NavicustPartColor::Pink),
            Some(tango_gamesupport_common_dataview::rom::NavicustPartColor::Yellow),
            extra_ncp_color(self.style().unwrap() as u8),
        ]
    }
}

pub struct NaviView<S> {
    save: S,
}

impl<S: std::ops::Deref<Target = Save>> tango_gamesupport_common_dataview::save::NaviView for NaviView<S> {
    // BN3 has no link-navi roster; the player navi is implicit, so the id is a
    // placeholder the Navi card ignores (the ROM has no navi entry for it).
    fn navi(&self) -> usize {
        0
    }

    fn max_hp(&self, _assets: &dyn tango_gamesupport_common_dataview::rom::Assets) -> u16 {
        let mut base_max_hp = 100 + 20 * (self.save.buf[0x1a20] as u16);

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

            if let Some(ex_code) = super::rom::ex_codes::ex_code(self.save.buf[0x1270]) {
                if let super::rom::ex_codes::ExCodeEffect::MaxHp(n) = ex_code.effect {
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
        let Some(nc) = self.save.view_navicust() else {
            unreachable!();
        };
        let layout = assets.navicust_layout().unwrap();

        // Base Regular Memory (raised permanently by RegUP items). The Reg+5
        // NaviCust bonus is applied on top below.
        let mut reg_memory: u8 = self.save.buf[0x1897];

        let mut mega: usize = if super::rom::style_type(self.save.buf[0x1881] & 0x3f) == super::rom::StyleType::Team {
            8
        } else {
            5
        };
        let mut giga: usize = 1;

        let grid = nc.materialized();

        // Reg+5 raises regular memory wherever it is placed in the grid.
        let mut seen = std::collections::HashSet::new();
        for &cell in grid.iter() {
            let Some(slot) = cell else { continue };
            if !seen.insert(slot) {
                continue; // a part spans several cells; count once
            }
            let Some(part) = nc.navicust_part(slot) else {
                continue;
            };
            for effect in super::rom::navicust::navicust_part_effects(part.id) {
                if let super::rom::navicust::NavicustEffect::RegMemory(n) = effect {
                    reg_memory += *n; // Reg+5
                }
            }
        }

        // MegFldr/GigFldr only count when they touch the command line.
        let mut seen = std::collections::HashSet::new();
        for &cell in grid.row(layout.command_line).iter() {
            let Some(slot) = cell else { continue };
            if !seen.insert(slot) {
                continue;
            }
            let Some(part) = nc.navicust_part(slot) else {
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

        if let Some(ec) = super::rom::ex_codes::ex_code(self.save.buf[0x1270]) {
            match ec.effect {
                super::rom::ex_codes::ExCodeEffect::MegaFolder(v) => mega += v as usize,
                super::rom::ex_codes::ExCodeEffect::GigaFolder(v) => giga += v as usize,
                _ => {}
            }
        }

        tango_gamesupport_common_dataview::save::FolderLimits {
            reg_memory: Some(reg_memory),
            mega_limit: Some(mega.clamp(0, 10)),
            giga_limit: Some(giga.clamp(0, 10)),
            max_copies: |chip| match chip.class() {
                tango_gamesupport_common_dataview::rom::ChipClass::Mega
                | tango_gamesupport_common_dataview::rom::ChipClass::Giga => 1,
                tango_gamesupport_common_dataview::rom::ChipClass::Standard => 4,
                _ => 0,
            },
            ..Default::default()
        }
    }
}
