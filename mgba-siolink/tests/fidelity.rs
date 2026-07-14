//! Restore fidelity: the two properties netplay stands on. Two `Pair`
//! instances must boot into identical states (peers construct their own
//! copies), and restore + replay with CORRECTED keys must be equivalent
//! to having run those keys all along — not just replay with the same
//! keys, which is blind to unserialized state that only diverging
//! speculation exposes. The corrected-keys case is exactly what caught
//! mgba's stale SIOCNT/RCNT shadows and unserialized SIOMLT_SEND (now
//! repaired in `mgba::sio::Driver::load_state`).

use mgba_siolink::{testrom, Pair};

fn schedule(t: u32) -> [u32; 2] {
    [(t / 3) & 0x3ff, ((t / 5) * 0x1b1) & 0x3ff]
}

#[test]
fn cross_instance_boot_identity() {
    mgba::log::install_default_logger();
    let rom = testrom::build();
    let mut a = Pair::new([rom.clone(), rom.clone()]).unwrap();
    let mut b = Pair::new([rom.clone(), rom.clone()]).unwrap();
    for t in 0..12 {
        let da = a.save().unwrap().digest();
        let db = b.save().unwrap().digest();
        assert_eq!(da, db, "fresh pairs diverged before tick {t}");
        a.tick(schedule(t));
        b.tick(schedule(t));
    }
}

#[test]
fn rollback_with_corrected_keys_matches_straight_run() {
    mgba::log::install_default_logger();
    let rom = testrom::build();

    // Straight run: schedule() all the way.
    let mut straight = Pair::new([rom.clone(), rom.clone()]).unwrap();
    let mut straight_digests = Vec::new();
    for t in 0..10 {
        straight_digests.push(straight.save().unwrap().digest());
        straight.tick(schedule(t));
    }

    // Speculative run: same until tick 4, then WRONG keys for 4..7,
    // then rollback to 4 and replay with the right ones.
    let mut spec = Pair::new([rom.clone(), rom.clone()]).unwrap();
    for t in 0..4 {
        assert_eq!(
            spec.save().unwrap().digest(),
            straight_digests[t as usize],
            "diverged before the speculation even started, tick {t}"
        );
        spec.tick(schedule(t));
    }
    let checkpoint = spec.save().unwrap();
    assert_eq!(checkpoint.digest(), straight_digests[4], "checkpoint digest mismatch");
    for t in 4..7 {
        spec.tick([0x2aa ^ t, 0x155 ^ t]); // garbage speculation
    }
    spec.load(&checkpoint).unwrap();
    for t in 4..10 {
        assert_eq!(
            spec.save().unwrap().digest(),
            straight_digests[t as usize],
            "restore+corrected-replay diverged from straight run at tick {t}"
        );
        spec.tick(schedule(t));
    }
}
