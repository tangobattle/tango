pub(super) fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    ((seed << 1) + (seed >> 0x1f) + std::num::Wrapping(1)).0 ^ 0x873ca9e5
}

fn generate_rng1_state(rng: &mut impl rand::Rng) -> u32 {
    (0..rng.gen_range(0..0x100000)).fold(0, |acc, _| step_rng(acc))
}

pub fn generate_rng2_state(rng: &mut impl rand::Rng) -> u32 {
    (0..rng.gen_range(0..0x100000)).fold(0xa338244f, |acc, _| step_rng(acc))
}

pub fn pick_rng_states(rng: &mut impl rand::Rng, is_offerer: bool) -> (u32, u32) {
    let offerer_rng1_state = generate_rng1_state(rng);
    let answerer_rng1_state = generate_rng1_state(rng);
    let rng1_state = if is_offerer { offerer_rng1_state } else { answerer_rng1_state };
    let rng2_state = generate_rng2_state(rng);
    (rng1_state, rng2_state)
}
