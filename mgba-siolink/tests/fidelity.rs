//! Restore fidelity: the two properties netplay stands on. Two `Link`
//! instances must boot into identical states (peers construct their own
//! copies), and restore + replay with CORRECTED keys must be equivalent
//! to having run those keys all along — not just replay with the same
//! keys, which is blind to unserialized state that only diverging
//! speculation exposes. The corrected-keys case is exactly what caught
//! mgba's stale SIOCNT/RCNT shadows and unserialized SIOMLT_SEND (now
//! repaired in `mgba::sio::Driver::load_state`).

use mgba_siolink::{testrom, Link};

fn schedule(t: u32, num_players: usize) -> Vec<u32> {
    (0..num_players as u32)
        .map(|p| ((t / (3 + p)) * (0x1b1 * p + 1)) & 0x3ff)
        .collect()
}

fn cross_instance_boot_identity(num_players: usize) {
    mgba::log::install_default_logger();
    let rom = testrom::build();
    let mut a = Link::new(vec![rom.clone(); num_players]).unwrap();
    let mut b = Link::new(vec![rom; num_players]).unwrap();
    for t in 0..12 {
        let da = a.save().unwrap().digest();
        let db = b.save().unwrap().digest();
        assert_eq!(da, db, "fresh links diverged before tick {t}");
        a.tick(&schedule(t, num_players));
        b.tick(&schedule(t, num_players));
    }
}

#[test]
fn cross_instance_boot_identity_2p() {
    cross_instance_boot_identity(2);
}

#[test]
fn cross_instance_boot_identity_4p() {
    cross_instance_boot_identity(4);
}

fn rollback_with_corrected_keys_matches_straight_run(num_players: usize) {
    mgba::log::install_default_logger();
    let rom = testrom::build();

    // Straight run: schedule() all the way.
    let mut straight = Link::new(vec![rom.clone(); num_players]).unwrap();
    let mut straight_digests = Vec::new();
    for t in 0..10 {
        straight_digests.push(straight.save().unwrap().digest());
        straight.tick(&schedule(t, num_players));
    }

    // Speculative run: same until tick 4, then WRONG keys for 4..7,
    // then rollback to 4 and replay with the right ones.
    let mut spec = Link::new(vec![rom; num_players]).unwrap();
    for t in 0..4 {
        assert_eq!(
            spec.save().unwrap().digest(),
            straight_digests[t as usize],
            "diverged before the speculation even started, tick {t}"
        );
        spec.tick(&schedule(t, num_players));
    }
    let checkpoint = spec.save().unwrap();
    assert_eq!(checkpoint.digest(), straight_digests[4], "checkpoint digest mismatch");
    for t in 4..7u32 {
        // Garbage speculation.
        let keys: Vec<u32> = (0..num_players as u32).map(|p| (0x2aa ^ t ^ (p * 0x155)) & 0x3ff).collect();
        spec.tick(&keys);
    }
    spec.load(&checkpoint).unwrap();
    for t in 4..10 {
        assert_eq!(
            spec.save().unwrap().digest(),
            straight_digests[t as usize],
            "restore+corrected-replay diverged from straight run at tick {t}"
        );
        spec.tick(&schedule(t, num_players));
    }
}

#[test]
fn rollback_with_corrected_keys_matches_straight_run_2p() {
    rollback_with_corrected_keys_matches_straight_run(2);
}

#[test]
fn rollback_with_corrected_keys_matches_straight_run_3p() {
    rollback_with_corrected_keys_matches_straight_run(3);
}
