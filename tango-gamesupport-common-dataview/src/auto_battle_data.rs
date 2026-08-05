pub struct MaterializedAutoBattleData([Option<usize>; 42]);

/// Slot allocation per deck section: the most-used chip in a section fills the
/// first count, the next-most the second, and so on; once the section's chips
/// run out the remaining slots stay empty. Shared by [`materialize_section`]
/// (which expands each run into that many slots) and [`GroupedAutoBattleData`]
/// (which keeps the run length as a count). Combos are always empty — the game
/// reserves those slots — so they carry no use-count ranking.
const SECONDARY_STANDARD_SLOTS: &[usize] = &[1, 1, 1];
const STANDARD_SLOTS: &[usize] = &[4, 4, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
const MEGA_SLOTS: &[usize] = &[1, 1, 1, 1, 1];
const GIGA_SLOTS: &[usize] = &[1];
const COMBO_SLOTS: &[usize] = &[1, 1, 1, 1, 1, 1, 1, 1];
const PROGRAM_ADVANCE_SLOTS: &[usize] = &[1];

/// One deck section in grouped form: `(chip, slots)` runs, where `chip` is
/// `None` for an unfilled run and `slots` is how many deck slots it fills.
/// Chips are ranked by use count (ties broken by id) and the section's slot
/// allocation is handed out top-down.
fn group_section(
    assets: &dyn crate::rom::Assets,
    use_counts: &[usize],
    chip_counts: &[usize],
    class: crate::rom::ChipClass,
) -> Vec<(Option<usize>, usize)> {
    let mut ranked = use_counts
        .iter()
        .enumerate()
        .filter(|(id, count)| assets.chip(*id).map(|c| c.class() == class).unwrap_or(false) && **count > 0)
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(id, count)| (std::cmp::Reverse(**count), *id));
    ranked
        .into_iter()
        .map(|(id, _)| Some(id))
        .chain(std::iter::repeat(None))
        .zip(chip_counts)
        .map(|(item, count)| (item, *count))
        .collect()
}

/// The materialized (expanded) form of [`group_section`]: each `(chip, slots)`
/// run flattened into `slots` repeated slots.
fn materialize_section(
    assets: &dyn crate::rom::Assets,
    use_counts: &[usize],
    chip_counts: &[usize],
    class: crate::rom::ChipClass,
) -> impl Iterator<Item = Option<usize>> {
    group_section(assets, use_counts, chip_counts, class)
        .into_iter()
        .flat_map(|(item, count)| std::iter::repeat_n(item, count))
}

impl MaterializedAutoBattleData {
    pub fn from_wram(buf: &[u8]) -> Self {
        #[repr(transparent)]
        #[derive(Clone, Copy)]
        struct RawMaterializedAutoBattleData([u16; 42]);
        unsafe impl bytemuck::Pod for RawMaterializedAutoBattleData {}
        unsafe impl bytemuck::Zeroable for RawMaterializedAutoBattleData {}

        Self(
            bytemuck::pod_read_unaligned::<RawMaterializedAutoBattleData>(buf)
                .0
                .into_iter()
                .map(|v| if v == 0xffff { None } else { Some(v as usize) })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        )
    }

    pub fn empty() -> Self {
        Self([None; 42])
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

        use crate::rom::ChipClass;
        Self(
            std::iter::empty()
                .chain(materialize_section(
                    assets,
                    &secondary_use_counts,
                    SECONDARY_STANDARD_SLOTS,
                    ChipClass::Standard,
                ))
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    STANDARD_SLOTS,
                    ChipClass::Standard,
                ))
                .chain(materialize_section(assets, &use_counts, MEGA_SLOTS, ChipClass::Mega))
                .chain(materialize_section(assets, &use_counts, GIGA_SLOTS, ChipClass::Giga))
                .chain(COMBO_SLOTS.iter().flat_map(|&count| std::iter::repeat_n(None, count)))
                .chain(materialize_section(
                    assets,
                    &use_counts,
                    PROGRAM_ADVANCE_SLOTS,
                    ChipClass::ProgramAdvance,
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

/// The auto-battle-data deck in grouped form: each section is a list of
/// `(chip, slots)` runs rather than the materialized deck's expanded per-slot
/// list. `slots` is how many deck slots the chip occupies — the most-used
/// standard chip takes 4, and so on — and `chip` is `None` for an unfilled run.
/// The materialized deck is exactly this with each run repeated `slots` times,
/// so a UI can show one "N× chip" row per run instead of N identical rows.
pub struct GroupedAutoBattleData {
    pub secondary_standard_chips: Vec<(Option<usize>, usize)>,
    pub standard_chips: Vec<(Option<usize>, usize)>,
    pub mega_chips: Vec<(Option<usize>, usize)>,
    pub giga_chip: Vec<(Option<usize>, usize)>,
    pub combos: Vec<(Option<usize>, usize)>,
    pub program_advance: Vec<(Option<usize>, usize)>,
}

impl GroupedAutoBattleData {
    /// Build the grouped deck from a save's per-chip use counts — the same
    /// ranking and slot allocation [`MaterializedAutoBattleData::materialize`]
    /// uses, kept grouped instead of expanded.
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

        use crate::rom::ChipClass;
        Self {
            secondary_standard_chips: group_section(
                assets,
                &secondary_use_counts,
                SECONDARY_STANDARD_SLOTS,
                ChipClass::Standard,
            ),
            standard_chips: group_section(assets, &use_counts, STANDARD_SLOTS, ChipClass::Standard),
            mega_chips: group_section(assets, &use_counts, MEGA_SLOTS, ChipClass::Mega),
            giga_chip: group_section(assets, &use_counts, GIGA_SLOTS, ChipClass::Giga),
            combos: COMBO_SLOTS.iter().map(|&count| (None, count)).collect(),
            program_advance: group_section(assets, &use_counts, PROGRAM_ADVANCE_SLOTS, ChipClass::ProgramAdvance),
        }
    }
}
