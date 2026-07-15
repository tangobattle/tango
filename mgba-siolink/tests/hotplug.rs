//! Hot-plug proof: solo machines that ran independently rebuild into one
//! linked system from their captures (the cable plugs in), the rebuild is
//! bit-identical across peers, rollback works on the plugged-in link, and
//! a side extracted from the link continues alone (the cable unplugs).

use mgba_siolink::{testrom, BootSide, Link};

fn read_log(link: &mut Link, core: usize, halfwords: usize) -> Vec<u16> {
    let mut buf = vec![0u8; halfwords * 2];
    link.core_mut(core).raw_read_range(testrom::LOG_ADDR, -1, &mut buf);
    buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

/// Number of recorded exchanges at the head of a log (a zeroed entry is
/// unwritten EWRAM).
fn entries(log: &[u16]) -> usize {
    log.chunks_exact(testrom::LOG_ENTRY_HALFWORDS)
        .take_while(|entry| entry.iter().any(|&h| h != 0))
        .count()
}

#[test]
fn plug_in_exchange_and_unplug() {
    mgba::log::install_default_logger();
    let rom = testrom::build();

    // Two solo machines (1-player links: no SIO driver — an unplugged
    // GBA), running for different lengths like two real players' machines
    // would have by the time they meet in a lobby.
    let mut solo_a = Link::new(vec![rom.clone()]).unwrap();
    let mut solo_b = Link::new(vec![rom.clone()]).unwrap();
    for t in 0..180u32 {
        solo_a.tick(&[t & 0x3ff]);
    }
    for t in 0..117u32 {
        solo_b.tick(&[(t * 3) & 0x3ff]);
    }
    // A unit alone on the cable records nothing.
    assert_eq!(entries(&read_log(&mut solo_a, 0, 8)), 0);

    let states = [
        solo_a.capture_boot_state(0).unwrap(),
        solo_b.capture_boot_state(0).unwrap(),
    ];
    let saves = [solo_a.export_save(0), solo_b.export_save(0)];

    // Every peer rebuilds the link from the same captures in the same
    // order; the machines must be bit-identical from the first tick on.
    let build = || {
        Link::from_states(
            (0..2)
                .map(|i| BootSide {
                    rom: rom.clone(),
                    save: saves[i].clone(),
                    state: states[i].clone(),
                })
                .collect(),
            None,
        )
        .unwrap()
    };
    let mut peer_a = build();
    let mut peer_b = build();
    assert_eq!(
        peer_a.save().unwrap().digest(),
        peer_b.save().unwrap().digest(),
        "rebuilds from identical captures differ at tick 0"
    );

    let keys = |t: u32| [t & 0x3ff, (t * 7) & 0x3ff];
    for t in 0..120u32 {
        let keys = keys(t);
        peer_a.tick(&keys);
        peer_b.tick(&keys);
        if t % 30 == 29 {
            assert_eq!(
                peer_a.save().unwrap().digest(),
                peer_b.save().unwrap().digest(),
                "peers diverged by tick {t}"
            );
        }
    }

    // Data crossed the fresh cable, and both cores of the link observed
    // the identical exchange sequence.
    let entry = testrom::LOG_ENTRY_HALFWORDS;
    let log = read_log(&mut peer_a, 0, 16 * entry);
    assert!(
        entries(&log) >= 16,
        "too few exchanges after plug-in: {:04x?}",
        &log[..4 * entry]
    );
    assert_eq!(log, read_log(&mut peer_a, 1, 16 * entry));

    // Rollback works on the plugged-in link: rewind and replay the same
    // key schedule, landing on the identical trajectory.
    let snap = peer_a.save().unwrap();
    let mut digests = Vec::new();
    for k in 0..20u32 {
        peer_a.tick(&keys(1000 + k));
        digests.push(peer_a.save().unwrap().digest());
    }
    peer_a.load(&snap).unwrap();
    for k in 0..20u32 {
        peer_a.tick(&keys(1000 + k));
        assert_eq!(
            peer_a.save().unwrap().digest(),
            digests[k as usize],
            "post-plug-in rollback replay diverged at tick {k}"
        );
    }

    // Unplug: one side extracted from the link continues alone, and the
    // emulation keeps advancing (the game sees its partner vanish, which
    // is the game's problem, not the machine's).
    let state = peer_b.capture_boot_state(1).unwrap();
    let save = peer_b.export_save(1);
    let mut alone = Link::from_states(
        vec![BootSide {
            rom: rom.clone(),
            save,
            state,
        }],
        None,
    )
    .unwrap();
    let before = alone.core(0).frame_counter();
    for t in 0..60u32 {
        alone.tick(&[t & 0x3ff]);
    }
    assert_eq!(alone.core(0).frame_counter(), before.wrapping_add(60));
}
