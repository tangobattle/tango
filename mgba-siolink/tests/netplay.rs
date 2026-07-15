//! End-to-end netplay simulation: one `Session` per peer, each running its
//! own copy of the link, exchanging input packets over a full mesh of
//! wires with more latency than the present delay covers, forcing real
//! mispredictions and rollbacks. The peers must stay bit-identical on
//! every commonly-settled tick — and for the two-player case, the recorded
//! replay must re-simulate to the same digests.

use std::collections::{HashMap, VecDeque};

use mgba_siolink::session::{Outgoing, Session};
use mgba_siolink::{replay, testrom, Link};

/// Present delay — purely local now, but kept equal on all peers so the
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

/// A one-way wire from one peer to another, delivering each packet
/// [`LATENCY`] frames after it was sent.
struct Wire {
    from: usize,
    queue: VecDeque<(u32, Outgoing)>,
}

impl Wire {
    fn new(from: usize) -> Self {
        Wire {
            from,
            queue: VecDeque::new(),
        }
    }

    fn send(&mut self, now: u32, packet: Outgoing) {
        self.queue.push_back((now + LATENCY, packet));
    }

    fn deliver(&mut self, now: u32, to: &mut Session) {
        while self.queue.front().is_some_and(|(at, _)| *at <= now) {
            let (_, p) = self.queue.pop_front().unwrap();
            to.add_remote_input(self.from, p.keys, p.tick_advantage);
        }
    }
}

struct MeshRun {
    peers: Vec<Session>,
    rollbacks: Vec<u32>,
    /// Ticks whose settled digest every peer reported (and agreed on).
    compared: u32,
    /// tick -> digest for every commonly-agreed settled boundary.
    checkpoints: HashMap<u32, (u32, Vec<bool>)>,
    /// Peer 0's confirmed input rows, in tick order (1-based ticks).
    confirmed: Vec<(u32, Box<[u32]>)>,
}

/// Drive `num_players` peers for [`FRAMES`] frames over a full mesh,
/// cross-checking every settled checkpoint digest between all of them.
fn run_mesh(num_players: usize) -> MeshRun {
    mgba::log::install_default_logger();
    let rom = testrom::build();

    let mut peers = (0..num_players)
        .map(|i| Session::new(Link::new(vec![rom.clone(); num_players]).unwrap(), i, DELAY).unwrap())
        .collect::<Vec<_>>();

    // wires[to][from_slot] — every ordered pair of peers gets a wire.
    let mut wires: Vec<Vec<Wire>> = (0..num_players)
        .map(|to| (0..num_players).filter(|&from| from != to).map(Wire::new).collect())
        .collect();

    let mut rollbacks = vec![0u32; num_players];
    let mut checkpoints: HashMap<u32, (u32, Vec<bool>)> = HashMap::new();
    let mut compared = 0;
    let mut confirmed = Vec::new();

    for frame in 0..FRAMES {
        // Advance every peer, broadcasting its outgoing packet to the rest.
        for i in 0..num_players {
            let (out, rep) = peers[i].advance(keys_for(i, frame)).unwrap();
            assert_eq!(out.tick, frame);
            rollbacks[i] += rep.rolled_back;
            for to in (0..num_players).filter(|&to| to != i) {
                let slot = if i < to { i } else { i - 1 };
                wires[to][slot].send(frame, out);
            }
        }
        // Deliver everything due this frame.
        for (to, peer) in peers.iter_mut().enumerate() {
            for wire in &mut wires[to] {
                wire.deliver(frame, peer);
            }
        }

        for (who, peer) in peers.iter().enumerate() {
            if let Some((tick, digest)) = peer.checkpoint() {
                let entry = checkpoints.entry(tick).or_insert((digest, vec![false; num_players]));
                assert_eq!(
                    entry.0, digest,
                    "desync at settled tick {tick} (frame {frame}, peer {who})"
                );
                if !entry.1[who] {
                    entry.1[who] = true;
                    if entry.1.iter().all(|&seen| seen) {
                        compared += 1;
                    }
                }
            }
        }

        confirmed.extend(peers[0].drain_confirmed());
    }

    // The engine never simulates past the present target of the newest
    // advance (frontier before that advance, minus the delay).
    for peer in &peers {
        assert_eq!(peer.with_link(|l| l.core(0).frame_counter()), FRAMES - 1 - DELAY);
    }
    assert!(
        rollbacks.iter().all(|&r| r > 0),
        "latency > present delay must force rollbacks on every peer, got {rollbacks:?}"
    );
    assert!(
        compared > FRAMES / 4,
        "expected checkpoint overlap on many ticks, got {compared}"
    );

    MeshRun {
        peers,
        rollbacks,
        compared,
        checkpoints,
        confirmed,
    }
}

#[test]
fn two_peer_convergence_and_replay() {
    let run = run_mesh(2);

    // Record peer 0's confirmed stream through the (two-sided) replay
    // container; ticks must come out 1-based and gapless, like on_tick's.
    let mut recorder = replay::Writer::new(&replay::Metadata {
        rtc_unix_micros: None,
        sides: Default::default(),
    });
    let mut recorded = 0u32;
    for (tick, keys) in &run.confirmed {
        recorded += 1;
        assert_eq!(*tick, recorded, "confirmed ticks are 1-based like on_tick's");
        recorder.push([keys[0], keys[1]]);
    }

    // The replay must land on the same states the live sessions agreed on.
    let rom = testrom::build();
    let parsed = replay::Replay::parse(&recorder.finish()).unwrap();
    assert_eq!(parsed.inputs.len(), recorded as usize);
    let mut link = Link::new(vec![rom.clone(), rom]).unwrap();
    for (tick, keys) in parsed.inputs.iter().enumerate() {
        if let Some((digest, seen)) = run.checkpoints.get(&(tick as u32)) {
            if seen.iter().all(|&s| s) {
                let snap = link.save().unwrap();
                assert_eq!(
                    snap.digest(),
                    *digest,
                    "replay diverged from the live sessions at tick {tick}"
                );
            }
        }
        link.tick(keys);
    }
}

#[test]
fn three_peer_convergence() {
    let run = run_mesh(3);
    // Confirmed rows are 1-based, gapless, and carry one key per player.
    for (i, (tick, keys)) in run.confirmed.iter().enumerate() {
        assert_eq!(*tick, i as u32 + 1);
        assert_eq!(keys.len(), 3);
    }
    assert!(!run.confirmed.is_empty());
    drop(run.peers);
    let _ = (run.rollbacks, run.compared);
}
