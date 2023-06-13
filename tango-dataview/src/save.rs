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
    fn as_sram_dump(&self) -> Vec<u8>;
    fn as_raw_wram<'a>(&'a self) -> std::borrow::Cow<'a, [u8]>;

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

    fn view_navi(&self) -> Option<NaviView> {
        None
    }

    fn view_navi_mut(&mut self) -> Option<NaviViewMut> {
        None
    }

    fn view_auto_battle_data(&self) -> Option<Box<dyn AutoBattleDataView + '_>> {
        None
    }

    fn view_auto_battle_data_mut(&mut self) -> Option<Box<dyn AutoBattleDataViewMut + '_>> {
        None
    }

    fn bugfrags(&self) -> Option<u32> {
        None
    }

    fn set_bugfrags(&mut self, count: u32) -> bool {
        let _ = count;
        false
    }
}

impl Clone for Box<dyn Save + Send + Sync> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub fn mask(buf: &mut [u8], mask_offset: usize) {
    let mask = bytemuck::pod_read_unaligned::<u32>(&buf[mask_offset..][..std::mem::size_of::<u32>()]);
    for b in buf.iter_mut() {
        *b = *b ^ (mask as u8);
    }
    buf[mask_offset..][..std::mem::size_of::<u32>()].copy_from_slice(bytemuck::bytes_of(&mask));
}

pub fn compute_raw_checksum(buf: &[u8], checksum_offset: usize) -> u32 {
    buf.iter().map(|v| *v as u32).sum::<u32>()
        - buf[checksum_offset..][..std::mem::size_of::<u32>()]
            .iter()
            .map(|v| *v as u32)
            .sum::<u32>()
}

#[derive(num_derive::FromPrimitive, Clone, Copy, Debug, std::hash::Hash, Eq, PartialEq)]
pub enum ChipCode {
    A = 0,
    B = 1,
    C = 2,
    D = 3,
    E = 4,
    F = 5,
    G = 6,
    H = 7,
    I = 8,
    J = 9,
    K = 10,
    L = 11,
    M = 12,
    N = 13,
    O = 14,
    P = 15,
    Q = 16,
    R = 17,
    S = 18,
    T = 19,
    U = 20,
    V = 21,
    W = 22,
    X = 23,
    Y = 24,
    Z = 25,
    Star = 26,
}

impl std::fmt::Display for ChipCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ChipCode::A => "A",
            ChipCode::B => "B",
            ChipCode::C => "C",
            ChipCode::D => "D",
            ChipCode::E => "E",
            ChipCode::F => "F",
            ChipCode::G => "G",
            ChipCode::H => "H",
            ChipCode::I => "I",
            ChipCode::J => "J",
            ChipCode::K => "K",
            ChipCode::L => "L",
            ChipCode::M => "M",
            ChipCode::N => "N",
            ChipCode::O => "O",
            ChipCode::P => "P",
            ChipCode::Q => "Q",
            ChipCode::R => "R",
            ChipCode::S => "S",
            ChipCode::T => "T",
            ChipCode::U => "U",
            ChipCode::V => "V",
            ChipCode::W => "W",
            ChipCode::X => "X",
            ChipCode::Y => "Y",
            ChipCode::Z => "Z",
            ChipCode::Star => "*",
        })
    }
}

#[derive(Clone, Debug, std::hash::Hash, Eq, PartialEq)]
pub struct Chip {
    pub id: usize,
    pub code: ChipCode,
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
    fn set_patch_card(&mut self, slot: usize, patch_card: Option<PatchCard>) -> bool;
}

pub enum NaviView<'a> {
    LinkNavi(Box<dyn LinkNaviView<'a> + 'a>),
    Navicust(Box<dyn NavicustView<'a> + 'a>),
}

pub enum NaviViewMut<'a> {
    LinkNavi(Box<dyn LinkNaviViewMut<'a> + 'a>),
    Navicust(Box<dyn NavicustViewMut<'a> + 'a>),
}

pub trait LinkNaviView<'a> {
    fn navi(&self) -> usize;
}

pub trait LinkNaviViewMut<'a> {
    fn set_navi(&self, navi: usize) -> bool;
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
    fn clear_materialized(&mut self);
    fn rebuild_materialized(&mut self, assets: &dyn crate::rom::Assets);
}

pub trait AutoBattleDataView<'a> {
    fn chip_use_count(&self, id: usize) -> Option<usize>;
    fn secondary_chip_use_count(&self, id: usize) -> Option<usize>;
    fn materialized(&self) -> crate::auto_battle_data::MaterializedAutoBattleData;
}

pub trait AutoBattleDataViewMut<'a> {
    fn set_chip_use_count(&mut self, id: usize, count: usize) -> bool;
    fn set_secondary_chip_use_count(&mut self, id: usize, count: usize) -> bool;
    fn clear_materialized(&mut self);
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
