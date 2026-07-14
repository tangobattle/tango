//! End-to-end netplay simulation: two `Session`s (one per peer, each
//! running its own copy of the pair) exchange input packets over a wire
//! with more latency than the present delay covers, forcing real
//! mispredictions and rollbacks. The peers must stay bit-identical on
//! every commonly-settled tick, and the recorded replay must re-simulate
//! to the same digests.

use std::collections::{HashMap, VecDeque};

use mgba_siolink::session::Session;
use mgba_siolink::{replay, testrom, Pair};

/// Present delay — purely local now, but kept equal on both peers so the
/// simulation is symmetric.
const DELAY: u32 = 2;
/// One-way wire latency in frames — deliberately larger than DELAY so
/// predictions run ahead of confirmations and corrections roll back.
const LATENCY: u32 = 5;
const FRAMES: u32 = 240;

fn keys_for(player: usize, frame: u32) -> u32 {
    // Deterministic schedule that changes every few frames so repeat-last
    // prediction is wrong regularly.
    let phase = frame / 7 + player as u32;
    (phase.wrapping_mul(2654435761)) & 0x3ff
}

struct Wire {
    queue: VecDeque<(u32, mgba_siolink::session::Outgoing)>,
}

impl Wire {
    fn new() -> Self {
        Wire { queue: VecDeque::new() }
    }

    fn send(&mut self, now: u32, packet: mgba_siolink::session::Outgoing) {
        self.queue.push_back((now + LATENCY, packet));
    }

    fn deliver(&mut self, now: u32, to: &mut Session) {
        while self.queue.front().is_some_and(|(at, _)| *at <= now) {
            let (_, p) = self.queue.pop_front().unwrap();
            to.add_remote_input(p.keys, p.tick_advantage);
        }
    }
}

#[test]
fn two_peer_convergence_and_replay() {
    mgba::log::install_default_logger();
    let rom = testrom::build();

    let mut peer_a = Session::new(Pair::new([rom.clone(), rom.clone()]).unwrap(), 0, DELAY).unwrap();
    let mut peer_b = Session::new(Pair::new([rom.clone(), rom.clone()]).unwrap(), 1, DELAY).unwrap();

    let mut wire_ab = Wire::new();
    let mut wire_ba = Wire::new();

    let mut rollbacks = [0u32; 2];
    // tick -> (digest, which peers reported it)
    let mut checkpoints: HashMap<u32, (u32, [bool; 2])> = HashMap::new();
    let mut compared = 0;
    let mut recorder = replay::Writer::new(&replay::Metadata {
        rtc_unix_micros: None,
        sides: Default::default(),
    });
    let mut recorded = 0u32;

    for frame in 0..FRAMES {
        let (out_a, rep_a) = peer_a.advance(keys_for(0, frame)).unwrap();
        let (out_b, rep_b) = peer_b.advance(keys_for(1, frame)).unwrap();
        assert_eq!(out_a.tick, frame);
        assert_eq!(out_b.tick, frame);
        wire_ab.send(frame, out_a);
        wire_ba.send(frame, out_b);
        wire_ab.deliver(frame, &mut peer_b);
        wire_ba.deliver(frame, &mut peer_a);
        rollbacks[0] += rep_a.rolled_back;
        rollbacks[1] += rep_b.rolled_back;

        for (who, peer) in [(0usize, &peer_a as &Session), (1, &peer_b)] {
            if let Some((tick, digest)) = peer.checkpoint() {
                let entry = checkpoints.entry(tick).or_insert((digest, [false; 2]));
                assert_eq!(
                    entry.0, digest,
                    "desync at settled tick {tick} (frame {frame}, peer {who})"
                );
                if !entry.1[who] {
                    entry.1[who] = true;
                    if entry.1 == [true, true] {
                        compared += 1;
                    }
                }
            }
        }

        for (tick, keys) in peer_a.drain_confirmed() {
            recorded += 1;
            assert_eq!(tick, recorded, "confirmed ticks are 1-based like on_tick's");
            recorder.push(keys);
        }
    }

    // The engine never simulates past the present target of the newest
    // advance (frontier before that advance, minus the delay).
    assert_eq!(peer_a.with_pair(|p| p.core(0).frame_counter()), FRAMES - 1 - DELAY);
    assert!(
        rollbacks[0] > 0 && rollbacks[1] > 0,
        "latency > present delay must force rollbacks, got {rollbacks:?}"
    );
    assert!(
        compared > FRAMES / 4,
        "expected checkpoint overlap on many ticks, got {compared}"
    );

    // The replay must land on the same states the live sessions agreed on.
    let parsed = replay::Replay::parse(&recorder.finish()).unwrap();
    assert_eq!(parsed.inputs.len(), recorded as usize);
    let mut pair = Pair::new([rom.clone(), rom]).unwrap();
    for (tick, keys) in parsed.inputs.iter().enumerate() {
        if let Some((digest, seen)) = checkpoints.get(&(tick as u32)) {
            if *seen == [true, true] {
                let snap = pair.save().unwrap();
                assert_eq!(
                    snap.digest(),
                    *digest,
                    "replay diverged from the live sessions at tick {tick}"
                );
            }
        }
        pair.tick(*keys);
    }
}
