use byteorder::ReadBytesExt;
use itertools::Itertools;

const SECONDARY_STANDARD_CHIP_COUNTS: &[usize] = &[1, 1, 1];
const STANDARD_CHIP_COUNTS: &[usize] = &[4, 4, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
const MEGA_CHIP_COUNTS: &[usize] = &[1, 1, 1, 1, 1];
const GIGA_CHIP_COUNTS: &[usize] = &[1];
const COMBO_COUNTS: &[usize] = &[1, 1, 1, 1, 1, 1, 1, 1];
const PROGRAM_ADVANCE_COUNTS: &[usize] = &[1];

pub struct MaterializedAutoBattleData([Option<usize>; 42]);

fn materialize_section<'a>(
    assets: &dyn crate::rom::Assets,
    use_counts: &'a [usize],
    chip_counts: &'a [usize],
    class: crate::rom::ChipClass,
) -> impl Iterator<Item = Option<usize>> + 'a {
    use_counts
        .iter()
        .enumerate()
        .filter(|(id, count)| assets.chip(*id).map(|c| c.class() == class).unwrap_or(false) && **count > 0)
        .sorted_by_key(|(id, count)| (std::cmp::Reverse(**count), *id))
        .map(|(id, _)| Some(id))
        .chain(std::iter::repeat(None))
        .zip(chip_counts)
        .flat_map(|(item, count)| vec![item; *count])
}

impl MaterializedAutoBattleData {
    pub fn from_save(mut buf: &[u8]) -> Self {
        Self(
            (0..42)
                .map(|_| {
                    let v = buf.read_u16::<byteorder::LittleEndian>().unwrap() as usize;
                    if v == 0xffff {
                        return None;
                    }
                    return Some(v);
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        )
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

        Self(
            std::iter::empty()
                .chain(materialize_section(
                    assets,
                    &secondary_use_counts,
                    &SECONDARY_STANDARD_CHIP_COUNTS[..],
                    crate::rom::ChipClass::Standard,
                ))
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    &STANDARD_CHIP_COUNTS[..],
                    crate::rom::ChipClass::Standard,
                ))
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    &MEGA_CHIP_COUNTS[..],
                    crate::rom::ChipClass::Mega,
                ))
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    &GIGA_CHIP_COUNTS[..],
                    crate::rom::ChipClass::Giga,
                ))
                .chain(
                    std::iter::repeat(None)
                        .zip(COMBO_COUNTS)
                        .flat_map(|(item, count)| vec![item; *count]),
                )
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    &PROGRAM_ADVANCE_COUNTS[..],
                    crate::rom::ChipClass::ProgramAdvance,
                ))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        )
    }

    pub fn as_slice(&self) -> &[Option<usize>] {
        &self.0
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
