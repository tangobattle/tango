use itertools::Itertools;

const SECONDARY_STANDARD_CHIP_COUNTS: &[usize; 3] = &[1, 1, 1];
const STANDARD_CHIP_COUNTS: &[usize; 16] = &[4, 4, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
const MEGA_CHIP_COUNTS: &[usize; 5] = &[1, 1, 1, 1, 1];
const GIGA_CHIP_COUNTS: &[usize; 1] = &[1];
const COMBO_COUNTS: &[usize; 8] = &[1, 1, 1, 1, 1, 1, 1, 1];
const PROGRAM_ADVANCE_COUNTS: &[usize; 1] = &[1];

pub struct MaterializedAutoBattleData([Option<usize>; 42]);

impl MaterializedAutoBattleData {
    pub fn new(v: [Option<usize>; 42]) -> Self {
        Self(v)
    }

    pub fn materialize(
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

        Self::new(
            std::iter::empty()
                .chain(
                    secondary_use_counts
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
                        .zip(SECONDARY_STANDARD_CHIP_COUNTS)
                        .flat_map(|(item, count)| vec![item; *count]),
                )
                .chain(
                    use_counts
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
                        .zip(STANDARD_CHIP_COUNTS)
                        .flat_map(|(item, count)| vec![item; *count]),
                )
                .chain(
                    use_counts
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
                        .zip(MEGA_CHIP_COUNTS)
                        .flat_map(|(item, count)| vec![item; *count]),
                )
                .chain(
                    std::iter::repeat(None)
                        .zip(COMBO_COUNTS)
                        .flat_map(|(item, count)| vec![item; *count]),
                )
                .chain(
                    use_counts
                        .iter()
                        .enumerate()
                        .filter(|(id, count)| {
                            assets
                                .chip(*id)
                                .map(|c| c.class() == crate::rom::ChipClass::Giga)
                                .unwrap_or(false)
                                && **count > 0
                        })
                        .sorted_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                        .map(|(id, _)| Some(id))
                        .chain(std::iter::repeat(None))
                        .zip(GIGA_CHIP_COUNTS)
                        .flat_map(|(item, count)| vec![item; *count]),
                )
                .chain(
                    use_counts
                        .iter()
                        .enumerate()
                        .filter(|(id, count)| {
                            assets
                                .chip(*id)
                                .map(|c| c.class() == crate::rom::ChipClass::ProgramAdvance)
                                .unwrap_or(false)
                                && **count > 0
                        })
                        .sorted_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
                        .map(|(id, _)| Some(id))
                        .chain(std::iter::repeat(None))
                        .zip(PROGRAM_ADVANCE_COUNTS)
                        .flat_map(|(item, count)| vec![item; *count]),
                )
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        )
    }

    pub fn secondary_standard_chips(&self) -> &[Option<usize>] {
        &self.0[0..3]
    }

    pub fn standard_chips(&self) -> &[Option<usize>] {
        &self.0[3..27]
    }

    pub fn mega_chips(&self) -> &[Option<usize>] {
        &self.0[27..32]
    }

    pub fn giga_chip(&self) -> Option<usize> {
        self.0[32]
    }

    pub fn combos(&self) -> &[Option<usize>] {
        &self.0[33..41]
    }

    pub fn program_advance(&self) -> Option<usize> {
        self.0[41]
    }
}
