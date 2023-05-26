use crate::save::NavicustView as _;

pub const SAVE_SIZE: usize = 0x73d2;
pub const MASK_OFFSET: usize = 0x1554;
pub const GAME_NAME_OFFSET: usize = 0x2208;
pub const CHECKSUM_OFFSET: usize = 0x21e8;
pub const SHIFT_OFFSET: usize = 0x1550;

pub const EREADER_NAME_OFFSET: usize = 0x1772;
pub const EREADER_NAME_SIZE: usize = 0x10;
pub const EREADER_DESCRIPTION_OFFSET: usize = 0x0522;
pub const EREADER_DESCRIPTION_SIZE: usize = 0x5c;

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
    shift: usize,
    game_info: GameInfo,
}

fn compute_raw_checksum(buf: &[u8], shift: usize) -> u32 {
    crate::save::compute_save_raw_checksum(&buf, shift + CHECKSUM_OFFSET)
}

impl Save {
    pub fn new(buf: &[u8]) -> Result<Self, crate::save::Error> {
        let mut buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(crate::save::Error::InvalidSize(buf.len()))?;

        crate::save::mask_save(&mut buf[..], MASK_OFFSET);

        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(crate::save::Error::InvalidShift(shift));
        }

        let n = &buf[shift + GAME_NAME_OFFSET..][..20];
        if n != b"ROCKMANEXE4 20031022" {
            return Err(crate::save::Error::InvalidGameName(n.to_vec()));
        }

        let game_info = {
            const RED_SUN: u32 = checksum_start_for_variant(Variant::RedSun);
            const BLUE_MOON: u32 = checksum_start_for_variant(Variant::BlueMoon);

            let expected_checksum =
                bytemuck::pod_read_unaligned::<u32>(&buf[shift + CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()]);
            let raw_checksum = compute_raw_checksum(&buf, shift);

            let (variant, region) = match expected_checksum.checked_sub(raw_checksum) {
                Some(RED_SUN) => (Variant::RedSun, Region::US),
                Some(BLUE_MOON) => (Variant::BlueMoon, Region::US),
                None => match expected_checksum.checked_sub(raw_checksum - buf[0] as u32) {
                    Some(RED_SUN) => (Variant::RedSun, Region::JP),
                    Some(BLUE_MOON) => (Variant::BlueMoon, Region::JP),
                    _ => {
                        return Err(crate::save::Error::ChecksumMismatch {
                            expected: vec![expected_checksum],
                            actual: raw_checksum,
                            shift,
                            attempt: 1,
                        });
                    }
                },
                _ => {
                    return Err(crate::save::Error::ChecksumMismatch {
                        expected: vec![expected_checksum],
                        actual: raw_checksum,
                        shift,
                        attempt: 0,
                    });
                }
            };

            GameInfo {
                variant,
                region: if buf[0] == 0 { Region::ANY } else { region },
            }
        };

        let save = Self { buf, shift, game_info };

        Ok(save)
    }

    pub fn from_wram(buf: &[u8], game_info: GameInfo) -> Result<Self, crate::save::Error> {
        let buf: [u8; SAVE_SIZE] = buf
            .get(..SAVE_SIZE)
            .and_then(|buf| buf.try_into().ok())
            .ok_or(crate::save::Error::InvalidSize(buf.len()))?;

        let shift = bytemuck::pod_read_unaligned::<u32>(&buf[SHIFT_OFFSET..][..std::mem::size_of::<u32>()]) as usize;
        if shift > 0x1fc || (shift & 3) != 0 {
            return Err(crate::save::Error::InvalidShift(shift));
        }

        Ok(Self { buf, game_info, shift })
    }

    pub fn checksum(&self) -> u32 {
        bytemuck::pod_read_unaligned::<u32>(&self.buf[self.shift + CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()])
    }

    pub fn compute_checksum(&self) -> u32 {
        compute_raw_checksum(&self.buf, self.shift) + checksum_start_for_variant(self.game_info.variant)
            - if self.game_info.region == Region::JP {
                self.buf[0] as u32
            } else {
                0
            }
    }

    pub fn shift(&self) -> usize {
        self.shift
    }

    pub fn game_info(&self) -> &GameInfo {
        &self.game_info
    }
}

impl crate::save::Save for Save {
    fn view_chips(&self) -> Option<Box<dyn crate::save::ChipsView + '_>> {
        Some(Box::new(ChipsView { save: self }))
    }

    fn view_navi(&self) -> Option<crate::save::NaviView> {
        Some(crate::save::NaviView::Navicust(Box::new(NavicustView { save: self })))
    }

    fn view_navi_mut(&mut self) -> Option<crate::save::NaviViewMut> {
        Some(crate::save::NaviViewMut::Navicust(Box::new(NavicustViewMut {
            save: self,
        })))
    }

    fn view_patch_cards(&self) -> Option<crate::save::PatchCardsView> {
        Some(crate::save::PatchCardsView::PatchCard4s(Box::new(PatchCard4sView {
            save: self,
        })))
    }

    fn view_patch_cards_mut(&mut self) -> Option<crate::save::PatchCardsViewMut> {
        Some(crate::save::PatchCardsViewMut::PatchCard4s(Box::new(
            PatchCard4sViewMut { save: self },
        )))
    }

    fn view_auto_battle_data(&self) -> Option<Box<dyn crate::save::AutoBattleDataView + '_>> {
        Some(Box::new(AutoBattleDataView { save: self }))
    }

    fn view_auto_battle_data_mut(&mut self) -> Option<Box<dyn crate::save::AutoBattleDataViewMut + '_>> {
        Some(Box::new(AutoBattleDataViewMut { save: self }))
    }

    fn as_raw_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]> {
        std::borrow::Cow::Borrowed(&self.buf)
    }

    fn to_sram_dump(&self) -> Vec<u8> {
        let mut buf = vec![0; 65536];
        buf[..SAVE_SIZE].copy_from_slice(&self.buf);
        crate::save::mask_save(&mut buf[..SAVE_SIZE], MASK_OFFSET);
        buf
    }

    fn rebuild_checksum(&mut self) {
        let checksum = self.compute_checksum();
        self.buf[CHECKSUM_OFFSET..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&checksum));
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

    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<crate::save::Chip> {
        if folder_index >= self.num_folders() || chip_index >= 30 {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawChip>(
            &self.save.buf[self.save.shift
                + 0x262c
                + folder_index * (30 * std::mem::size_of::<RawChip>())
                + chip_index * std::mem::size_of::<RawChip>()..][..std::mem::size_of::<RawChip>()],
        );

        Some(crate::save::Chip {
            id: raw.id() as usize,
            code: b"ABCDEFGHIJKLMNOPQRSTUVWXYZ*"[raw.variant() as usize] as char,
        })
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
    col: u8,
    row: u8,
    rot: u8,
    compressed: u8,
    _unk_06: [u8; 2],
}
const _: () = assert!(std::mem::size_of::<RawNavicustPart>() == 0x8);

impl<'a> crate::save::NavicustView<'a> for NavicustView<'a> {
    fn width(&self) -> usize {
        5
    }

    fn height(&self) -> usize {
        5
    }

    fn navicust_part(&self, i: usize) -> Option<crate::save::NavicustPart> {
        if i >= self.count() {
            return None;
        }

        let raw = bytemuck::pod_read_unaligned::<RawNavicustPart>(
            &self.save.buf[self.save.shift + 0x4564 + i * std::mem::size_of::<RawNavicustPart>()..]
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
        Some(crate::navicust::materialized_from_wram(
            &self.save.buf[self.save.shift + 0x4540..][..(self.height() * self.width())],
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

        self.save.buf[self.save.shift + 0x4564 + i * std::mem::size_of::<RawNavicustPart>()..]
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
        self.save.buf[self.save.shift + 0x4540..][..0x24].copy_from_slice(&[0; 0x24]);
    }

    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets) {
        let materialized = crate::navicust::materialize(&NavicustView { save: self.save }, assets);
        self.save.buf[self.save.shift + 0x4540..][..0x24].copy_from_slice(
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

impl<'a> crate::save::PatchCard4sView<'a> for PatchCard4sView<'a> {
    fn patch_card(&self, slot: usize) -> Option<crate::save::PatchCard> {
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
        Some(crate::save::PatchCard { id, enabled })
    }
}

pub struct PatchCard4sViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> crate::save::PatchCard4sViewMut<'a> for PatchCard4sViewMut<'a> {
    fn set_patch_card(&mut self, slot: usize, patch_card: Option<crate::save::PatchCard>) -> bool {
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

impl<'a> crate::save::AutoBattleDataView<'a> for AutoBattleDataView<'a> {
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

    fn materialized(&self) -> crate::auto_battle_data::MaterializedAutoBattleData {
        crate::auto_battle_data::MaterializedAutoBattleData::from_wram(
            &self.save.buf[self.save.shift + 0x5064..][..42 * std::mem::size_of::<u16>()],
        )
    }
}

pub struct AutoBattleDataViewMut<'a> {
    save: &'a mut Save,
}

impl<'a> AutoBattleDataViewMut<'a> {
    fn set_materialized(&mut self, materialized: &crate::auto_battle_data::MaterializedAutoBattleData) {
        self.save.buf[self.save.shift + 0x5064..][..42 * std::mem::size_of::<u16>()].copy_from_slice(
            &bytemuck::pod_collect_to_vec(
                &materialized
                    .as_slice()
                    .iter()
                    .map(|v| v.unwrap_or(0xffff) as u16)
                    .collect::<Vec<_>>(),
            ),
        );
    }
}

impl<'a> crate::save::AutoBattleDataViewMut<'a> for AutoBattleDataViewMut<'a> {
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
        self.set_materialized(&crate::auto_battle_data::MaterializedAutoBattleData::empty());
    }

    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets) {
        let materialized = crate::auto_battle_data::MaterializedAutoBattleData::materialize(
            &AutoBattleDataView { save: self.save },
            assets,
        );
        self.set_materialized(&materialized);
    }
}
