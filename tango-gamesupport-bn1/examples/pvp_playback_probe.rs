//! Headless probe for the SIO replay-playback machinery: a reference
//! pass steps a pair linearly and records savestate digests at
//! checkpoint ticks; a playback pass re-runs the same stream through a
//! second pair with the seam's snapshot store/rewind ring — holding
//! the probe's own whole-pair snapshots, the richer kind the generic
//! store was built for, because digests need the raw mgba state back —
//! and must reproduce every digest, including after backward and
//! forward snapshot-loaded seeks.
//!
//! Usage: pvp_playback_probe <rom> <save>

use std::collections::HashMap;
use std::sync::Arc;

use tango_match::telemetry::EventSink;
use tango_backend_mgba::{GameSupport, PrimeConfig, PrimedLatch};
use tango_gamesupport_bn1::pvp;
use tango_match::replay::{RewindRing, SnapshotStore};

fn pvp_for(rom: &[u8]) -> &'static pvp::Pvp {
    match &rom[0xac..0xb0] {
        b"AREE" => &pvp::PVP_AREE_00,
        b"AREJ" => &pvp::PVP_AREJ_00,
        code => panic!("not a bn1 rom (code {:02x?})", code),
    }
}

const TOTAL: u32 = 1800;
const CHECKPOINTS: &[u32] = &[300, 900, 1799, 1800];

/// The probe's keyframe: a whole-pair savestate poised at a tick (=
/// input pairs consumed).
struct Snap {
    state: mgba_rollback::Snapshot,
    tick: u32,
}

/// A bare playback pair, booted and primed exactly as the engine boots
/// one, fed the recorded stream by hand.
struct Playback {
    pair: mgba_rollback::Link,
    inputs: Arc<Vec<[u32; 2]>>,
    cursor: u32,
}

impl Playback {
    fn boot(rom: &[u8], save: &[u8], support: &'static pvp::Pvp, inputs: Arc<Vec<[u32; 2]>>) -> Self {
        let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
            sides: (0..2)
                .map(|_| mgba_rollback::SideOptions {
                    rom: rom.to_vec(),
                    save: Some(save.to_vec()),
                })
                .collect(),
            rtc: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000)),
            peripheral: mgba_rollback::Peripheral::Cable,
        })
        .expect("boot pair");

        let config = PrimeConfig {
            match_type: (0, 0),
            rng_seed: *b"sio-probe-seed!!",
            disable_bgm: false,
        };
        let events_sink = EventSink::new();
        let primed = [PrimedLatch::new(), PrimedLatch::new()];
        pair.set_traps(0, support.primer_traps(&config, 0, &events_sink, &primed[0]));
        pair.set_traps(1, support.primer_traps(&config, 1, &events_sink, &primed[1]));
        let mut prime_ticks = 0;
        while !(primed[0].is_set() && primed[1].is_set()) {
            assert!(prime_ticks < 3600, "priming wedged");
            pair.tick(&[0, 0]);
            prime_ticks += 1;
        }

        Playback { pair, inputs, cursor: 0 }
    }

    /// Feed the next recorded input pair. Returns false at end-of-stream.
    fn step(&mut self) -> bool {
        let Some(&keys) = self.inputs.get(self.cursor as usize) else {
            return false;
        };
        self.pair.tick(&keys);
        self.cursor += 1;
        true
    }

    fn capture(&mut self) -> Arc<Snap> {
        Arc::new(Snap {
            state: self.pair.save().expect("capture"),
            tick: self.cursor,
        })
    }

    fn load(&mut self, snap: &Snap) {
        self.pair.load(&snap.state).expect("restore");
        self.cursor = snap.tick;
    }

    fn digest(&mut self) -> u32 {
        self.pair.save().unwrap().digest()
    }
}

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let save = std::fs::read(&args[1]).expect("save unreadable");
    let s = pvp_for(&rom);

    // Idle through the intro, then a deterministic mash distinct per core.
    let inputs: Arc<Vec<[u32; 2]>> = Arc::new(
        (0..TOTAL)
            .map(|t| {
                let mut keys = [0u32; 2];
                if t > 600 {
                    for (i, k) in keys.iter_mut().enumerate() {
                        *k = ((t / 5).wrapping_mul(2654435761) >> i) & 0x3f3;
                    }
                }
                keys
            })
            .collect(),
    );

    // Reference pass: plain linear stepping.
    let mut reference = Playback::boot(&rom, &save, s, inputs.clone());
    let mut digests: HashMap<u32, u32> = HashMap::new();
    while reference.step() {
        if CHECKPOINTS.contains(&reference.cursor) {
            digests.insert(reference.cursor, reference.digest());
        }
    }
    assert_eq!(reference.cursor, TOTAL);
    println!("reference digests: {digests:08x?}");

    // Playback pass: step through with the drive loop's capture policy.
    let mut pb = Playback::boot(&rom, &save, s, inputs);
    let store: SnapshotStore<Snap> = SnapshotStore::new();
    let ring: RewindRing<Snap> = RewindRing::new();
    store.push(0, pb.capture());
    while pb.step() {
        let snap = pb.capture();
        if store.snapshot_needed(snap.tick) {
            store.push(snap.tick, snap.clone());
        }
        ring.insert(snap.tick, snap);
        if CHECKPOINTS.contains(&pb.cursor) {
            assert_eq!(pb.digest(), digests[&pb.cursor], "linear divergence at {}", pb.cursor);
        }
    }
    println!("linear playback: OK (cursor {})", pb.cursor);

    // Exact-hit backward step: 1799 must be in the rewind ring.
    let snap = ring.best_at_or_before(1799).expect("ring hit");
    assert_eq!(snap.tick, 1799, "rewind ring should hold the last frames exactly");
    pb.load(&snap);
    assert_eq!(pb.digest(), digests[&1799]);
    println!("rewind-ring exact backward step: OK");

    // Deep backward seek: keyframe at/below 900, catch up, digest match.
    let snap = store.best_at_or_before(900).expect("keyframe at/below 900");
    assert!(
        snap.tick <= 900 && 900 - snap.tick <= 60,
        "keyframe interval violated: {}",
        snap.tick
    );
    pb.load(&snap);
    while pb.cursor < 900 {
        pb.step();
    }
    assert_eq!(pb.digest(), digests[&900]);
    println!("deep backward seek to 900: OK (from keyframe {})", snap.tick);

    // Forward seek: jump via keyframe, catch up to 1799.
    let snap = store.best_in_range(pb.cursor, 1799).expect("forward keyframe");
    pb.load(&snap);
    while pb.cursor < 1799 {
        pb.step();
    }
    assert_eq!(pb.digest(), digests[&1799]);
    println!("forward keyframe seek to 1799: OK (via {})", snap.tick);

    println!("SUCCESS");
}
