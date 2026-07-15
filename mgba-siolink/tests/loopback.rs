//! Loopback proof for generic SIO rollback: two to four cores linked
//! through the lockstep driver exchange data over emulated MULTI mode, and
//! restoring a link snapshot then replaying the same key schedule
//! reproduces the exact same trajectory — including snapshots taken with a
//! transfer in flight or a core parked by the lockstep protocol.

use mgba_siolink::{testrom, Link};

/// Digest of the link's rollback-relevant state. Deliberately built from
/// discrete savestate fields rather than the raw state bytes: mgba
/// serializes into an uninitialized buffer without touching reserved
/// regions, so whole-struct bytes aren't comparable. CPU registers plus
/// both RAMs plus the lockstep blobs are more than enough to expose any
/// trajectory divergence within a tick or two.
fn digest(link: &mut Link) -> u32 {
    let snap = link.save().unwrap();
    let mut h = crc32fast::Hasher::new();
    for i in 0..link.num_players() {
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

fn read_log(link: &mut Link, core: usize, halfwords: usize) -> Vec<u16> {
    let mut buf = vec![0u8; halfwords * 2];
    link.core_mut(core).raw_read_range(testrom::LOG_ADDR, -1, &mut buf);
    buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

/// Number of completed exchanges at the head of a log: entry `k` must show
/// every attached player's tagged payload and 0xFFFF in every unattached
/// slot.
fn exchanges(log: &[u16], num_players: usize) -> usize {
    log.chunks_exact(testrom::LOG_ENTRY_HALFWORDS)
        .enumerate()
        .take_while(|(k, entry)| {
            entry.iter().enumerate().all(|(slot, &got)| {
                got == if slot < num_players {
                    testrom::payload(slot, *k)
                } else {
                    testrom::UNATTACHED
                }
            })
        })
        .count()
}

fn keys_for_tick(t: u32, num_players: usize) -> Vec<u32> {
    // The test ROM never reads the joypad; feeding a varying schedule just
    // exercises key latching at tick boundaries.
    (0..num_players as u32).map(|p| (t * (0x11 + p * 6)) & 0x3ff).collect()
}

fn exchange_and_rollback_determinism(num_players: usize) {
    // Route mgba's C-side logging through the (backend-less, therefore
    // silent) Rust log facade instead of its printf stub.
    mgba::log::install_default_logger();

    let rom = testrom::build();
    let mut link = Link::new(vec![rom; num_players]).unwrap();

    for i in 0..num_players {
        assert_eq!(link.player_id(i), i as i32);
    }

    // Phase 1: the link comes up and data actually crosses it.
    let entry = testrom::LOG_ENTRY_HALFWORDS;
    let mut tick = 0u32;
    while (0..num_players).any(|i| exchanges(&read_log(&mut link, i, 16 * entry), num_players) < 16) {
        link.tick(&keys_for_tick(tick, num_players));
        tick += 1;
        assert!(
            tick < 1200,
            "link never came up: logs {:04x?} / {:04x?}",
            &read_log(&mut link, 0, 4 * entry),
            &read_log(&mut link, num_players - 1, 4 * entry)
        );
    }
    // Every side observed the identical exchange sequence.
    for i in 1..num_players {
        assert_eq!(
            read_log(&mut link, 0, 16 * entry),
            read_log(&mut link, i, 16 * entry)
        );
    }

    // Phase 2: per-tick snapshots along a trajectory...
    const SPAN: usize = 40;
    const RESTORE_AT: usize = 10;
    let mut snapshots = Vec::new();
    let mut digests = Vec::new();
    for k in 0..SPAN {
        snapshots.push(link.save().unwrap());
        digests.push(digest(&mut link));
        link.tick(&keys_for_tick(tick + k as u32, num_players));
    }
    let final_digest = digest(&mut link);

    // ...then rewind into the middle and replay the same key schedule: every
    // subsequent tick must land on the identical link state, bit for bit.
    link.load(&snapshots[RESTORE_AT]).unwrap();
    assert_eq!(
        digest(&mut link),
        digests[RESTORE_AT],
        "restore did not reproduce the snapshotted state"
    );
    for k in RESTORE_AT..SPAN {
        assert_eq!(
            digest(&mut link),
            digests[k],
            "replay diverged at tick {} of {}",
            k,
            SPAN
        );
        link.tick(&keys_for_tick(tick + k as u32, num_players));
    }
    assert_eq!(
        digest(&mut link),
        final_digest,
        "replay diverged at the end of the span"
    );

    // The exchange kept running after the rollback replay.
    let log = read_log(&mut link, 0, 64 * entry);
    assert!(
        exchanges(&log, num_players) > 16,
        "no further exchanges after rollback: {:04x?}",
        &log[..8 * entry]
    );
}

#[test]
fn exchange_and_rollback_determinism_2p() {
    exchange_and_rollback_determinism(2);
}

#[test]
fn exchange_and_rollback_determinism_3p() {
    exchange_and_rollback_determinism(3);
}

#[test]
fn exchange_and_rollback_determinism_4p() {
    exchange_and_rollback_determinism(4);
}
