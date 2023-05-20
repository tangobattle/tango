use byteorder::ByteOrder;

pub trait SaveClone {
    fn clone_box(&self) -> Box<dyn Save + Sync + Send>;
}

impl<T> SaveClone for T
where
    T: 'static + Save + Sync + Send + Clone,
{
    fn clone_box(&self) -> Box<dyn Save + Sync + Send> {
        Box::new(self.clone())
    }
}

pub enum PatchCardsView<'a> {
    PatchCard4s(Box<dyn PatchCard4sView<'a> + 'a>),
    PatchCard56s(Box<dyn PatchCard56sView<'a> + 'a>),
}

pub enum PatchCardsViewMut<'a> {
    PatchCard4s(Box<dyn PatchCard4sViewMut<'a> + 'a>),
    PatchCard56s(Box<dyn PatchCard56sViewMut<'a> + 'a>),
}

pub trait Save
where
    Self: SaveClone,
{
    fn to_vec(&self) -> Vec<u8>;
    fn as_raw_wram(&self) -> &[u8];

    fn rebuild_checksum(&mut self);

    fn view_chips(&self) -> Option<Box<dyn ChipsView + '_>> {
        None
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn ChipsViewMut + '_>> {
        None
    }

    fn view_patch_cards(&self) -> Option<PatchCardsView> {
        None
    }

    fn view_patch_cards_mut(&mut self) -> Option<PatchCardsViewMut> {
        None
    }

    fn view_navicust(&self) -> Option<Box<dyn NavicustView + '_>> {
        None
    }

    fn view_navicust_mut(&mut self) -> Option<Box<dyn NavicustViewMut + '_>> {
        None
    }

    fn view_auto_battle_data(&self) -> Option<Box<dyn AutoBattleDataView + '_>> {
        None
    }

    fn view_auto_battle_data_mut(&mut self) -> Option<Box<dyn AutoBattleDataViewMut + '_>> {
        None
    }

    fn view_navi(&self) -> Option<Box<dyn NaviView + '_>> {
        None
    }
}

impl Clone for Box<dyn Save + Send + Sync> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub fn mask_save(buf: &mut [u8], mask_offset: usize) {
    let mask = byteorder::LittleEndian::read_u32(&buf[mask_offset..mask_offset + 4]);
    for b in buf.iter_mut() {
        *b = *b ^ (mask as u8);
    }
    byteorder::LittleEndian::write_u32(&mut buf[mask_offset..mask_offset + 4], mask);
}

pub fn compute_save_raw_checksum(buf: &[u8], checksum_offset: usize) -> u32 {
    buf.iter().map(|v| *v as u32).sum::<u32>()
        - buf[checksum_offset..checksum_offset + 4]
            .iter()
            .map(|v| *v as u32)
            .sum::<u32>()
}

#[derive(Clone, Debug, std::hash::Hash, Eq, PartialEq)]
pub struct Chip {
    pub id: usize,
    pub code: char,
}

pub trait ChipsView<'a> {
    fn num_folders(&self) -> usize;
    fn equipped_folder_index(&self) -> usize;
    fn regular_chip_index(&self, folder_index: usize) -> Option<usize>;
    fn tag_chip_indexes(&self, folder_index: usize) -> Option<[usize; 2]>;
    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<Chip>;
    fn pack_count(&self, id: usize, variant: usize) -> Option<usize> {
        let _ = id;
        let _ = variant;
        None
    }
}

pub trait ChipsViewMut<'a> {
    fn set_equipped_folder(&mut self, folder_index: usize) -> bool {
        let _ = folder_index;
        false
    }
    fn set_chip(&mut self, folder_index: usize, chip_index: usize, chip: Chip) -> bool;
    fn set_tag_chip_indexes(&mut self, folder_index: usize, chip_indexes: Option<[usize; 2]>) -> bool {
        let _ = folder_index;
        let _ = chip_indexes;
        false
    }
    fn set_regular_chip_index(&mut self, folder_index: usize, chip_index: usize) -> bool {
        let _ = folder_index;
        let _ = chip_index;
        false
    }
    fn set_pack_count(&mut self, id: usize, variant: usize, count: usize) -> bool {
        let _ = id;
        let _ = variant;
        let _ = count;
        false
    }
    fn rebuild_anticheat(&mut self);
}

#[derive(Clone, Debug, std::hash::Hash, Eq, PartialEq)]
pub struct PatchCard {
    pub id: usize,
    pub enabled: bool,
}

pub trait PatchCard56sView<'a> {
    fn count(&self) -> usize;
    fn patch_card(&self, slot: usize) -> Option<PatchCard>;
}

pub trait PatchCard56sViewMut<'a> {
    fn set_count(&mut self, count: usize);
    fn set_patch_card(&mut self, slot: usize, patch_card: PatchCard) -> bool;
    fn rebuild_anticheat(&mut self);
}

pub trait PatchCard4sView<'a> {
    fn patch_card(&self, slot: usize) -> Option<PatchCard>;
}

pub trait PatchCard4sViewMut<'a> {
    fn set_patch_card(&mut self, slot: usize, patch_card: Option<PatchCard>);
}

pub trait NaviView<'a> {
    fn navi(&self) -> usize;
}

#[derive(Clone, Debug, std::hash::Hash, Eq, PartialEq)]
pub struct NavicustPart {
    pub id: usize,
    pub variant: usize,
    pub col: u8,
    pub row: u8,
    pub rot: u8,
    pub compressed: bool,
}

pub trait NavicustView<'a> {
    fn count(&self) -> usize {
        25
    }
    fn style(&self) -> Option<usize> {
        None
    }
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn navicust_part(&self, i: usize) -> Option<NavicustPart>;
    fn materialized(&self) -> Option<crate::navicust::MaterializedNavicust>;
}

pub trait NavicustViewMut<'a> {
    fn set_style(&mut self, _style: usize) -> bool {
        false
    }

    fn set_navicust_part(&mut self, i: usize, part: NavicustPart) -> bool;
    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets);
}

pub trait AutoBattleDataView<'a> {
    fn chip_use_count(&self, id: usize) -> Option<usize>;
    fn secondary_chip_use_count(&self, id: usize) -> Option<usize>;
    fn materialized(&self) -> crate::abd::MaterializedAutoBattleData;
}

pub trait AutoBattleDataViewMut<'a> {
    fn set_chip_use_count(&mut self, id: usize, count: usize) -> bool;
    fn set_secondary_chip_use_count(&mut self, id: usize, count: usize) -> bool;
    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets);
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("invalid size: {0} bytes")]
    InvalidSize(usize),

    #[error("invalid game name: {0:02x?}")]
    InvalidGameName(Vec<u8>),

    #[error("invalid checksum: {actual:08x} not in {expected:08x?} (shift: {shift}, attempt: {attempt})")]
    ChecksumMismatch {
        expected: Vec<u32>,
        actual: u32,
        shift: usize,
        attempt: usize,
    },

    #[error("invalid shift: {0}")]
    InvalidShift(usize),
}
