//! RNG state generation shared by the modern games (BN4, BN5, BN6, EXE4.5).
//! The match is seeded exactly once, at the comm-menu-settings path, which
//! seeds rng1 (local) and rng2 (shared) identically; the deterministic match
//! carries the game's RNG across rounds from there, so there's no per-round
//! re-seed. The earlier games (BN1–BN3) keep their own per-game `rng.rs` with
//! different generators and signatures.

fn step_rng(seed: u32) -> u32 {
    let seed = std::num::Wrapping(seed);
    ((seed << 1) + (seed >> 0x1f) + std::num::Wrapping(1)).0 ^ 0x873ca9e5
}

pub(crate) fn generate_rng2_state(rng: &mut impl rand::Rng) -> u32 {
    (0..rng.gen_range(0..0x100000)).fold(0xa338244f, |acc, _| step_rng(acc))
}
