pub struct MaterializedAutoBattleData([Option<usize>; 42]);

fn materialize_section<'a>(
    assets: &dyn crate::rom::Assets,
    use_counts: &'a [usize],
    chip_counts: &'a [usize],
    class: crate::rom::ChipClass,
) -> impl Iterator<Item = Option<usize>> + 'a {
    let mut materialized = use_counts
        .iter()
        .enumerate()
        .filter(|(id, count)| assets.chip(*id).map(|c| c.class() == class).unwrap_or(false) && **count > 0)
        .collect::<Vec<_>>();
    materialized.sort_by_key(|(id, count)| (std::cmp::Reverse(**count), *id));
    materialized
        .into_iter()
        .map(|(id, _)| Some(id))
        .chain(std::iter::repeat(None))
        .zip(chip_counts)
        .flat_map(|(item, count)| vec![item; *count])
}

impl MaterializedAutoBattleData {
    pub fn from_wram(buf: &[u8]) -> Self {
        Self(
            bytemuck::cast_slice::<_, u16>(buf)
                .into_iter()
                .map(|v| if *v == 0xffff { None } else { Some(*v as usize) })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        )
    }

    pub fn materialize(
        auto_battle_data_view: &(dyn crate::save::AutoBattleDataView + '_),
        assets: &dyn crate::rom::Assets,
    ) -> Self {
        let use_counts = (0..assets.num_chips())
            .map(|id| auto_battle_data_view.chip_use_count(id).unwrap_or(0))
            .collect::<Vec<_>>();

        let secondary_use_counts = (0..assets.num_chips())
            .map(|id| auto_battle_data_view.secondary_chip_use_count(id).unwrap_or(0))
            .collect::<Vec<_>>();

        Self(
            std::iter::empty()
                .chain(materialize_section(
                    assets,
                    &secondary_use_counts,
                    &[1, 1, 1],
                    crate::rom::ChipClass::Standard,
                ))
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    &[4, 4, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
                    crate::rom::ChipClass::Standard,
                ))
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    &[1, 1, 1, 1, 1],
                    crate::rom::ChipClass::Mega,
                ))
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    &[1],
                    crate::rom::ChipClass::Giga,
                ))
                .chain(
                    std::iter::repeat(None)
                        .zip(&[1, 1, 1, 1, 1, 1, 1, 1])
                        .flat_map(|(item, count)| vec![item; *count]),
                )
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    &[1],
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
        &self.0[0..][..3]
    }

    pub fn standard_chips(&self) -> &[Option<usize>] {
        &self.0[3..][..24]
    }

    pub fn mega_chips(&self) -> &[Option<usize>] {
        &self.0[27..][..5]
    }

    pub fn giga_chip(&self) -> Option<usize> {
        self.0[32]
    }

    pub fn combos(&self) -> &[Option<usize>] {
        &self.0[33..][..8]
    }

    pub fn program_advance(&self) -> Option<usize> {
        self.0[41]
    }
}
