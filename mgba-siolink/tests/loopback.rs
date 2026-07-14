//! Loopback proof for generic SIO rollback: two cores linked through the
//! lockstep driver exchange data over emulated MULTI mode, and restoring a
//! pair snapshot then replaying the same key schedule reproduces the exact
//! same trajectory — including snapshots taken with a transfer in flight
//! or a core parked by the lockstep protocol.

use mgba_siolink::{testrom, Pair};

/// Digest of the pair's rollback-relevant state. Deliberately built from
/// discrete savestate fields rather than the raw state bytes: mgba
/// serializes into an uninitialized buffer without touching reserved
/// regions, so whole-struct bytes aren't comparable. CPU registers plus
/// both RAMs plus the lockstep blobs are more than enough to expose any
/// trajectory divergence within a tick or two.
fn digest(pair: &mut Pair) -> u32 {
    let snap = pair.save().unwrap();
    let mut h = crc32fast::Hasher::new();
    for i in 0..2 {
        let s = snap.core_state(i);
        for r in 0..16 {
            h.update(&s.gpr(r).to_le_bytes());
        }
        h.update(&s.cpsr().to_le_bytes());
        h.update(s.wram());
        h.update(s.iwram());
        h.update(snap.driver_blob(i));
    }
    h.finalize()
}

fn read_log(pair: &mut Pair, core: usize, halfwords: usize) -> Vec<u16> {
    let mut buf = vec![0u8; halfwords * 2];
    pair.core_mut(core).raw_read_range(testrom::LOG_ADDR, -1, &mut buf);
    buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

/// Number of completed (master, slave) exchange pairs at the head of a log.
fn exchanges(log: &[u16]) -> usize {
    log.chunks_exact(2)
        .enumerate()
        .take_while(|(k, pair)| {
            let expected = (k + 1) as u16;
            pair[0] == expected && pair[1] == expected | 0x8000
        })
        .count()
}

fn keys_for_tick(t: u32) -> [u32; 2] {
    // The test ROM never reads the joypad; feeding a varying schedule just
    // exercises key latching at tick boundaries.
    [(t * 0x11) & 0x3ff, (t * 0x07) & 0x3ff]
}

#[test]
fn exchange_and_rollback_determinism() {
    // Route mgba's C-side logging through the (backend-less, therefore
    // silent) Rust log facade instead of its printf stub.
    mgba::log::install_default_logger();

    let rom = testrom::build();
    let mut pair = Pair::new([rom.clone(), rom]).unwrap();

    assert_eq!(pair.player_id(0), 0);
    assert_eq!(pair.player_id(1), 1);

    // Phase 1: the link comes up and data actually crosses it.
    let mut tick = 0u32;
    while exchanges(&read_log(&mut pair, 0, 64)) < 16 || exchanges(&read_log(&mut pair, 1, 64)) < 16 {
        pair.tick(keys_for_tick(tick));
        tick += 1;
        assert!(
            tick < 600,
            "link never came up: logs {:04x?} / {:04x?}",
            &read_log(&mut pair, 0, 16),
            &read_log(&mut pair, 1, 16)
        );
    }
    // Both sides observed the identical exchange sequence.
    assert_eq!(read_log(&mut pair, 0, 32), read_log(&mut pair, 1, 32));

    // Phase 2: per-tick snapshots along a trajectory...
    const SPAN: usize = 40;
    const RESTORE_AT: usize = 10;
    let mut snapshots = Vec::new();
    let mut digests = Vec::new();
    for k in 0..SPAN {
        snapshots.push(pair.save().unwrap());
        digests.push(digest(&mut pair));
        pair.tick(keys_for_tick(tick + k as u32));
    }
    let final_digest = digest(&mut pair);

    // ...then rewind into the middle and replay the same key schedule: every
    // subsequent tick must land on the identical pair state, bit for bit.
    pair.load(&snapshots[RESTORE_AT]).unwrap();
    assert_eq!(
        digest(&mut pair),
        digests[RESTORE_AT],
        "restore did not reproduce the snapshotted state"
    );
    for k in RESTORE_AT..SPAN {
        assert_eq!(
            digest(&mut pair),
            digests[k],
            "replay diverged at tick {} of {}",
            k,
            SPAN
        );
        pair.tick(keys_for_tick(tick + k as u32));
    }
    assert_eq!(
        digest(&mut pair),
        final_digest,
        "replay diverged at the end of the span"
    );

    // The exchange kept running after the rollback replay.
    let log = read_log(&mut pair, 0, 128);
    assert!(
        exchanges(&log) > 16,
        "no further exchanges after rollback: {:04x?}",
        &log[..32]
    );
}
