pub(super) fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    ((seed << 1) + (seed >> 0x1f) + std::num::Wrapping(1)).0 ^ 0x873ca9e5
}

pub(super) fn generate_rng_state(rng: &mut impl rand::Rng) -> u32 {
    (0..rng.gen_range(0..0x100000)).fold(0xa338244f, |acc, _| step_rng(acc))
}

pub(super) fn random_background(rng: &mut impl rand::Rng) -> u8 {
    const BATTLE_BACKGROUNDS: &[u8] = &[0x00, 0x01, 0x02, 0x03, 0x05, 0x08, 0x15, 0x18];
    BATTLE_BACKGROUNDS[rng.gen_range(0..BATTLE_BACKGROUNDS.len())]
}
