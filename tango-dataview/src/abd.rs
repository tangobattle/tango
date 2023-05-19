use itertools::Itertools;

pub const SECONDARY_STANDARD_CHIP_COUNTS: &[usize; 3] = &[1, 1, 1];
pub const STANDARD_CHIP_COUNTS: &[usize; 16] = &[4, 4, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
pub const MEGA_CHIP_COUNTS: &[usize; 5] = &[1, 1, 1, 1, 1];
pub const GIGA_CHIP_COUNTS: &[usize; 1] = &[1];
pub const COMBO_COUNTS: &[usize; 8] = &[1, 1, 1, 1, 1, 1, 1, 1];
pub const PROGRAM_ADVANCE_COUNTS: &[usize; 1] = &[1];

pub struct MaterializedAutoBattleData {
    pub secondary_standard_chips: [Option<usize>; 3],
    pub standard_chips: [Option<usize>; 16],
    pub mega_chips: [Option<usize>; 5],
    pub giga_chip: Option<usize>,
    pub combos: [Option<usize>; 8],
    pub program_advance: Option<usize>,
}

impl MaterializedAutoBattleData {
    pub fn new(
        auto_battle_data_view: &(dyn crate::save::AutoBattleDataView + '_),
        assets: &dyn crate::rom::Assets,
    ) -> Self {
        let mut use_counts = vec![];
        loop {
            if let Some(count) = auto_battle_data_view.chip_use_count(use_counts.len()) {
                use_counts.push(count);
            } else {
                break;
            }
        }

        let mut secondary_use_counts = vec![];
        loop {
            if let Some(count) = auto_battle_data_view.secondary_chip_use_count(secondary_use_counts.len()) {
                secondary_use_counts.push(count);
            } else {
                break;
            }
        }

        MaterializedAutoBattleData {
            secondary_standard_chips: secondary_use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == crate::rom::ChipClass::Standard)
                        .unwrap_or(false)
                        && **count > 0
                })
                .sorted_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| Some(id))
                .chain(std::iter::repeat(None))
                .take(3)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            standard_chips: use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == crate::rom::ChipClass::Standard)
                        .unwrap_or(false)
                        && **count > 0
                })
                .sorted_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| Some(id))
                .chain(std::iter::repeat(None))
                .take(16)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            mega_chips: use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == crate::rom::ChipClass::Mega)
                        .unwrap_or(false)
                        && **count > 0
                })
                .sorted_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| Some(id))
                .chain(std::iter::repeat(None))
                .take(5)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            giga_chip: use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == crate::rom::ChipClass::Giga)
                        .unwrap_or(false)
                        && **count > 0
                })
                .min_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| id),
            combos: [None; 8],
            program_advance: use_counts
                .iter()
                .enumerate()
                .filter(|(id, count)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class() == crate::rom::ChipClass::ProgramAdvance)
                        .unwrap_or(false)
                        && **count > 0
                })
                .min_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                .map(|(id, _)| id),
        }
    }
}
