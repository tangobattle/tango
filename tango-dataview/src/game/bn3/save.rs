use bitvec::view::BitView;

use crate::{
    game::bn3::rom::extra_ncp_color,
    save::{ChipsView as _, NavicustView as _},
};

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
    crate::save::compute_raw_checksum(buf, CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, crate::save::Error> {
        let buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(crate::save::Error::InvalidSize(buf.len()))?;

        let n = &buf[GAME_NAME_OFFSET..][..20];
        if n != b"ROCKMANEXE3 20021002" && n != b"BBN3 v0.5.0 20021002" {
            return Err(crate::save::Error::InvalidGameName(n.to_vec()));
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
                        return Err(crate::save::Error::ChecksumMismatch {
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

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, crate::save::Error> {
        Ok(Self {
            buf: buf
                .get(..SAVE_SIZE)
                .and_then(|buf| buf.try_into().ok())
                .ok_or(crate::save::Error::InvalidSize(buf.len()))?,
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

    fn set_flag(&mut self, i: usize, v: bool) {
        self.buf[0x0030 + i / 8]
            .view_bits_mut::<bitvec::order::Msb0>()
            .set(i % 8, v)
    }
}

impl crate::save::Save for Save {
    fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn view_chips(&self) -> Option<Box<dyn crate::save::ChipsView<'_> + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn crate::save::ChipsViewMut<'_> + '_>> {
        Some(Box::new(ChipsViewMut { save: self }))
    }

    fn view_navi(&self) -> Option<crate::save::NaviView<'_>> {
        Some(crate::save::NaviView::Navicust(Box::new(NavicustView { save: self })))
    }

    fn view_navi_mut(&mut self) -> Option<crate::save::NaviViewMut<'_>> {
        Some(crate::save::NaviViewMut::Navicust(Box::new(NavicustViewMut {
            save: self,
        })))
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SAVE_SIZE].copy_from_slice(&self.buf);
        buf
    }

    fn folder_limits(&self, assets: &dyn crate::rom::Assets) -> crate::save::FolderLimits {
        let crate::save::NaviView::Navicust(nc) = self.view_navi().unwrap() else {
            unreachable!();
        };
        let layout = assets.navicust_layout().unwrap();

        // Base Regular Memory (raised permanently by RegUP items). The Reg+5
        // NaviCust bonus is applied on top below.
        let mut reg_memory: u8 = self.buf[0x1897];

        let mut mega: usize = nc
            .style()
            .and_then(|s| assets.style(s))
            .and_then(|style| {
                if matches!(style.typ(), crate::rom::StyleType::Team) {
                    Some(8)
                } else {
                    None
                }
            })
            .unwrap_or(5);
        let mut giga: usize = 1;

        let grid = nc.materialized();

        // Reg+5 raises regular memory wherever it is placed in the grid.
        let mut seen = std::collections::HashSet::new();
        for &cell in grid.iter() {
            let Some(slot) = cell else { continue };
            if !seen.insert(slot) {
                continue; // a part spans several cells; count once
            }
            if nc.navicust_part(slot).is_some_and(|part| part.id / 4 == 0x28) {
                reg_memory += 5; // Reg+5
            }
        }

        // MegFldr/GigFldr only count when they touch the command line.
        let mut seen = std::collections::HashSet::new();
        for &cell in grid.row(layout.command_line).iter() {
            let Some(slot) = cell else { continue };
            if !seen.insert(slot) {
                continue; // a part spans several command-line cells; count once
            }
            let Some(part) = nc.navicust_part(slot) else {
                continue;
            };
            match part.id / 4 {
                0x0c => mega += 1, // MegFldr1
                0x0d => mega += 2, // MegFldr2
                0x30 => giga += 1, // GigFldr1
                _ => {}
            }
        }

        if let Some(ec) = nc.ex_code().and_then(|code| assets.ex_code(code as u8)) {
            match ec.effect {
                crate::rom::ExCodeEffect::MegaFolder(v) => mega += v as usize,
                crate::rom::ExCodeEffect::GigaFolder(v) => giga += v as usize,
                _ => {}
            }
        }

        crate::save::FolderLimits {
            reg_memory: Some(reg_memory),
            mega_limit: Some(mega.clamp(0, 10)),
            giga_limit: Some(giga.clamp(0, 10)),
            max_copies: |chip| match chip.class() {
                crate::rom::ChipClass::Mega | crate::rom::ChipClass::Giga => 1,
                crate::rom::ChipClass::Standard => 4,
                _ => 0,
            },
            ..Default::default()
        }
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&checksum));
    }
}

pub struct ChipsView<'a> {
    save: &'a Save,
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, bytemuck::NoUninit, Clone, Copy, Default)]
struct RawChip {
    id: u16,
    code: u16,
}
const _: () = assert!(std::mem::size_of::<RawChip>() == 0x4);

impl<'a> crate::save::ChipsView<'a> for ChipsView<'a> {
    fn num_folders(&self) -> usize {
        3 // TODO
    }

    fn equipped_folder_index(&self) -> usize {
        self.save.buf[0x1882] as usize
    }

    fn regular_chip_index(&self, folder_index: usize) -> Option<Option<usize>> {
        let idx = self.save.buf[0x189d + folder_index];
        Some(if idx >= 30 { None } else { Some(idx as usize) })
    }

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<crate::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[0x1410
                + folder_index * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(crate::save::Chip {
            id: raw.id as usize,
            code: num_traits::FromPrimitive::from_u16(raw.code)?,
        })
    }

    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        if id >= super::NUM_PACK_CHIPS {
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

pub struct ChipsViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> crate::save::ChipsViewMut<'a> for ChipsViewMut<'a> {
    fn set_chip(&mut self, folder_index: usize, chip_index: usize, chip: crate::save::Chip) -> bool {
        if folder_index >= (ChipsView { save: self.save }).num_folders() || chip_index >= 30 {
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
        if folder_index >= (ChipsView { save: self.save }).num_folders() || chip_index >= 30 {
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
        if folder_index >= (ChipsView { save: self.save }).num_folders() {
            return false;
        }

        // 0xff (out of the 0..30 range) reads back as "no regular".
        let raw = match chip_index {
            Some(i) if i < 30 => i as u8,
            None => 0xff,
            Some(_) => return false,
        };
        self.save.buf[0x189d + folder_index] = raw;
        true
    }

    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        if id >= super::NUM_PACK_CHIPS {
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

pub struct NavicustView<'a> {
    save: &'a Save,
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

impl<'a> crate::save::NavicustView<'a> for NavicustView<'a> {
    fn size(&self) -> [usize; 2] {
        [5, 5]
    }

    fn style(&self) -> Option<usize> {
        // The style byte packs element (bits 0..=2, only 0..=4 valid) and
        // type (bits 3..=5); bits 6-7 are unused. A byte outside that range
        // isn't a real style (e.g. an empty / never-assigned slot), so
        // report no style rather than a bogus one.
        let raw = self.save.buf[0x1881];
        super::is_valid_style(raw).then_some(raw as usize)
    }

    fn ex_code(&self) -> Option<usize> {
        Some(self.save.buf[0x1270] as usize)
    }

    fn navicust_part(&self, i: usize) -> Option<crate::save::NavicustPart> {
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

        Some(crate::save::NavicustPart {
            id: raw.id as usize,
            col: raw.col,
            row: raw.row,
            rot: raw.rot,
            compressed: self.save.flag(0x02e0 + raw.id as usize),
        })
    }

    fn materialized(&self) -> crate::navicust::MaterializedNavicust {
        crate::navicust::materialized_from_wram(&self.save.buf[0x1d90..][..(5 * 5)], [5, 5])
    }

    fn navicust_color_bar(&self) -> Vec<Option<crate::rom::NavicustPartColor>> {
        vec![
            Some(crate::rom::NavicustPartColor::White),
            Some(crate::rom::NavicustPartColor::Pink),
            Some(crate::rom::NavicustPartColor::Yellow),
            self.style().and_then(|s| extra_ncp_color(s as u8)),
        ]
    }

    fn unrestricted_colors(&self) -> Option<Vec<crate::rom::NavicustPartColor>> {
        // The color bar (White/Pink/Yellow + the style's extra color) is
        // exactly the set BN3 lets you use freely; any other color is
        // capped at a single installed program across the whole grid.
        Some(self.navicust_color_bar().into_iter().flatten().collect())
    }
}

pub struct NavicustViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> crate::save::NavicustViewMut<'a> for NavicustViewMut<'a> {
    fn set_style(&mut self, style: usize) -> bool {
        // Reject anything that isn't a real style (bad element/type bits);
        // a valid style fully occupies the byte, so write it as-is.
        let Ok(raw) = u8::try_from(style) else {
            return false;
        };
        if !super::is_valid_style(raw) {
            return false;
        }
        self.save.buf[0x1881] = raw;
        true
    }

    fn set_ex_code(&mut self, code: Option<u8>) -> bool {
        // 0 = no EX Code (matches `NavicustView::ex_code`'s read at 0x1270).
        let raw = match code {
            None => 0,
            Some(code) if super::rom::is_valid_ex_code(code) => code,
            Some(_) => return false,
        };
        self.save.buf[0x1270] = raw;
        true
    }

    fn set_navicust_part(&mut self, i: usize, part: Option<crate::save::NavicustPart>) -> bool {
        if i >= (NavicustView { save: self.save }).count() {
            return false;
        }
        let raw = match part {
            Some(part) => {
                if part.id >= super::NUM_NAVICUST_PARTS {
                    return false;
                }
                // Unlike BN4+, BN3 keeps the compressed state in a
                // per-program flag bitfield rather than inline in the part
                // record — mirror `NavicustView`'s read at `0x02e0 + id`.
                self.save.set_flag(0x02e0 + part.id, part.compressed);
                RawNavicustPart {
                    id: part.id as u8,
                    col: part.col,
                    row: part.row,
                    rot: part.rot,
                    ..Default::default()
                }
            }
            // An all-zero part (id 0) reads back as an empty slot. Leaving
            // the old program's compressed flag set is harmless: it's only
            // consulted for ids that are actually placed, and re-placing a
            // part always rewrites it.
            None => RawNavicustPart::default(),
        };
        self.save.buf[0x1300 + i * std::mem::size_of::<RawNavicustPart>()..][..std::mem::size_of::<RawNavicustPart>()]
            .copy_from_slice(bytemuck::bytes_of(&raw));

        true
    }

    fn clear_materialized(&mut self) {
        self.save.buf[0x1d90..][..(5 * 5)].copy_from_slice(&[0; 5 * 5]);
    }

    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets) {
        let materialized = crate::navicust::materialize(&NavicustView { save: self.save }, [5, 5], assets);
        self.save.buf[0x1d90..][..(5 * 5)].copy_from_slice(
            &materialized
                .into_iter()
                .map(|v| v.map(|v| v + 1).unwrap_or(0) as u8)
                .chain(std::iter::repeat(0))
                .take(5 * 5)
                .collect::<Vec<_>>(),
        );
        // BN3 computes the navicust color bar on read (White/Pink/Yellow +
        // a style-derived color), so there's nothing to rebuild there.

        // Recompile MegaMan's ability array (0x5770) from style + the placed
        // programs (+ their command-line membership, just written above) +
        // the EX Code — the array the game actually reads. Without this, edits
        // to navicust/style/excode don't take effect until the customizer is
        // re-entered in-game. Battle-scratch slots are left for the game to
        // recompute at battle start.
        let abil = super::abilities::compile(&self.save.buf);
        for i in 0..super::abilities::ABILITY_ARRAY_LEN {
            if !super::abilities::SCRATCH.contains(&i) {
                self.save.buf[super::abilities::ABILITY_ARRAY_OFFSET + i] = abil[i];
            }
        }
    }
}
