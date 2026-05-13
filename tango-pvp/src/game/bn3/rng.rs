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

pub(super) fn random_background(rng: &mut impl rand::Rng) -> u8 {
    const BATTLE_BACKGROUNDS: &[u8] = &[0x00, 0x04, 0x05, 0x06, 0x17, 0x10, 0x02, 0x0a];
    BATTLE_BACKGROUNDS[rng.gen_range(0..BATTLE_BACKGROUNDS.len())]
}

pub(super) fn bn3_match_type(rng: &mut impl rand::Rng, match_type: (u8, u8)) -> u8 {
    match match_type {
        (0, 1) => 0,
        (0, 2) => 1,
        (0, 3) => 2,
        (0, _) => rng.gen_range(0..3),
        (1, _) => 3,
        _ => 0,
    }
}
