//! The determinism-critical derivations both peers must compute
//! identically from the negotiated state. Divergence here doesn't fail
//! loudly — it desyncs the simulation or splits the reconnect
//! rendezvous — so every implementation shares these functions.

/// The match seed: the two peers' commit nonces XOR'd. Order-free, so
/// both sides compute the same seed without agreeing who is "first".
pub fn derive_rng_seed(local_nonce: &[u8; 16], peer_nonce: &[u8; 16]) -> [u8; 16] {
    std::array::from_fn(|i| local_nonce[i] ^ peer_nonce[i])
}

/// The match clock: the *offerer's* commit-time wall clock (ms since
/// the unix epoch) wins. Pinned into every core's cart RTC on both
/// sides and recorded as the replay's `ts`.
pub fn pick_match_ts(is_offerer: bool, local_ts: u64, peer_ts: u64) -> u64 {
    if is_offerer {
        local_ts
    } else {
        peer_ts
    }
}

/// Picks the per-match local_player_index. Both peers must call this with
/// the same shared RNG state at the same point in the protocol so they end
/// up on opposite sides. Advances the RNG by one draw.
pub fn pick_local_player_index(rng: &mut rand_pcg::Mcg128Xsl64, is_offerer: bool) -> u8 {
    use rand::Rng;
    let did_polite_win = rng.gen::<bool>();
    if did_polite_win == is_offerer {
        0
    } else {
        1
    }
}

/// Derive the matchmaking reconnect `session_id`, the rendezvous code both peers
/// re-dial after a mid-match drop. It must be reproducible by either peer yet
/// unguessable to anyone else (so a stranger can't camp the rendezvous and
/// hijack the reconnect).
///
/// Two independent secrets are mixed in, neither sufficient alone:
///
/// * `rng_seed` — the shared match RNG seed (XOR of both commit nonces, exchanged
///   over the encrypted data channel). The *signaling server* never sees it.
/// * the two DTLS certificate fingerprints — per-connection, high-entropy, and
///   verified during the handshake, but unlike `rng_seed` never written to disk
///   (the seed doubles as the in-match RNG seed, so it lands in replay files). A
///   *replay holder* never sees the fingerprints.
///
/// So no single party outside the two peers can reproduce the id: the server has
/// the fingerprints but not the seed; a replay leaks the seed but not the
/// fingerprints. The two fingerprints are folded together by XOR — commutative,
/// so both peers reach the same value without having to agree on an order (which
/// is "local" vs "remote" is swapped between them).
///
/// Falls back to seed-only (the original construction) when a fingerprint is
/// missing or the two differ in length — e.g. a peer whose signaling stack didn't
/// surface one — so the two ends still agree on an id rather than silently
/// diverging. Domain-separated from the lobby commitment (same `Shake128`, over
/// `"tango:lobby:"`).
///
/// We also prefix it with _ as the client does not allow construction of
/// link codes containing _, but the server does permit them.
pub fn derive_reconnect_session_id(rng_seed: &[u8; 16], fp_a: &[u8], fp_b: &[u8]) -> String {
    use sha3::digest::{ExtendableOutput, Update, XofReader};
    let mut h = sha3::Shake128::default();
    h.update(b"tango:reconnect:");
    h.update(rng_seed);
    // Both fingerprints are SHA-256 digests (equal length); the empty / unequal
    // guard keeps the two peers in lockstep on the seed-only fallback when one is
    // absent rather than mixing in a lopsided value.
    if !fp_a.is_empty() && fp_a.len() == fp_b.len() {
        let folded: Vec<u8> = fp_a.iter().zip(fp_b).map(|(a, b)| a ^ b).collect();
        h.update(&folded);
    }
    let mut out = [0u8; 16];
    h.finalize_xof().read(&mut out);
    let mut code: String = "_".into();
    code.extend(out.iter().map(|b| format!("{b:02x}")));
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_seed_is_commutative() {
        let a = [0x11u8; 16];
        let b: [u8; 16] = std::array::from_fn(|i| i as u8);
        assert_eq!(derive_rng_seed(&a, &b), derive_rng_seed(&b, &a));
    }

    #[test]
    fn player_index_sides_are_opposite() {
        use rand::SeedableRng;
        let seed = [7u8; 16];
        let mut rng_a = rand_pcg::Mcg128Xsl64::from_seed(seed);
        let mut rng_b = rand_pcg::Mcg128Xsl64::from_seed(seed);
        let a = pick_local_player_index(&mut rng_a, true);
        let b = pick_local_player_index(&mut rng_b, false);
        assert_eq!(a + b, 1);
    }

    #[test]
    fn reconnect_id_fingerprints_commute_and_fall_back() {
        let seed = [3u8; 16];
        let fa = [0xaau8; 32];
        let fb = [0x55u8; 32];
        assert_eq!(
            derive_reconnect_session_id(&seed, &fa, &fb),
            derive_reconnect_session_id(&seed, &fb, &fa)
        );
        // One side missing → both must agree on the seed-only form.
        assert_eq!(
            derive_reconnect_session_id(&seed, &[], &fb),
            derive_reconnect_session_id(&seed, &[], &[])
        );
        assert!(derive_reconnect_session_id(&seed, &fa, &fb).starts_with('_'));
    }
}
