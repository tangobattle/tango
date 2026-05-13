pub(super) fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    ((seed << 1) + (seed >> 0x1f) + std::num::Wrapping(1)).0 ^ 0x873ca9e5
}

pub(super) fn generate_rng1_state(rng: &mut impl rand::Rng) -> u32 {
    (0..rng.gen_range(0..0x100000)).fold(0, |acc, _| step_rng(acc))
}

pub(super) fn generate_rng2_state(rng: &mut impl rand::Rng) -> u32 {
    (0..rng.gen_range(0..0x100000)).fold(0xa338244f, |acc, _| step_rng(acc))
}

pub(super) fn random_battle_settings_and_background(extended: bool, rng: &mut impl rand::Rng) -> (u8, u8) {
    (
        rng.gen_range(0..if !extended { 0x44u8 } else { 0x60 }),
        rng.gen_range(0..0x1bu8),
    )
}
