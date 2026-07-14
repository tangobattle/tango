//! Experimental generic rollback netplay over emulated SIO (link cable).
//!
//! Instead of per-game traps that replace a game's link protocol with
//! memory-level input exchange, both GBAs run locally as a *pair* connected
//! through mgba's lockstep SIO driver, and the pair is the rollback unit:
//! the only true inputs are the two joypads, everything on the wire is
//! derived deterministically. A netplay session runs the same `Pair` on both
//! peers, feeds confirmed local + predicted remote keys into `tick`, and
//! restores a `Snapshot` to re-simulate when a prediction turns out wrong.
//!
//! The cores are interleaved cooperatively on ONE thread (see
//! `mgba::sio`): a tick runs whichever cores the lockstep protocol has not
//! parked, one `run_loop` timing slice at a time, until the reference core
//! (index 0) finishes one video frame. The peer core floats inside the
//! lockstep drift window and may be mid-frame or parked at a tick boundary;
//! that partial progress is exactly captured by the pair snapshot, so the
//! interleave replays identically after a restore.

pub mod replay;
pub mod session;
pub mod testrom;

/// Which core a tick treats as the frame-boundary reference. Player 0 is
/// also the lockstep clock owner (primary).
const REFERENCE: usize = 0;

/// Upper bound on run_loop slices per tick, to turn a lockstep deadlock
/// (which would otherwise spin forever) into a loud failure. A frame is
/// ~70 lockstep intervals per core; 2M slices is orders of magnitude past
/// any legitimate tick.
const MAX_SLICES_PER_TICK: usize = 2_000_000;

pub struct Pair {
    // Declaration order is drop order, and it matters: a core's deinit
    // calls back into its SIO driver, and detaching a driver touches the
    // coordinator.
    cores: [mgba::core::Core; 2],
    drivers: [mgba::sio::Driver; 2],
    #[allow(dead_code)]
    coordinator: mgba::sio::Coordinator,
}

/// A consistent snapshot of the whole linked system: both cores plus both
/// lockstep driver blobs (the coordinator's shared state rides in player
/// 0's blob). Core savestates alone are NOT sufficient — the lockstep
/// event queues, sleep flags, and in-flight transfer data live outside
/// them.
pub struct Snapshot {
    cores: [Box<mgba::state::State>; 2],
    drivers: [Vec<u8>; 2],
}

impl Snapshot {
    pub fn core_state(&self, i: usize) -> &mgba::state::State {
        &self.cores[i]
    }

    pub fn driver_blob(&self, i: usize) -> &[u8] {
        &self.drivers[i]
    }

    /// Digest of the rollback-relevant state, comparable across peers
    /// simulating the same pair (the desync canary). Deliberately built
    /// from discrete savestate fields rather than raw state bytes: mgba
    /// serializes into an uninitialized buffer without touching reserved
    /// regions, so whole-struct bytes are not comparable. CPU registers
    /// plus both RAMs plus the lockstep blobs expose any trajectory
    /// divergence within a tick or two.
    pub fn digest(&self) -> u32 {
        let mut h = crc32fast::Hasher::new();
        for i in 0..2 {
            let s = self.core_state(i);
            for r in 0..16 {
                h.update(&s.gpr(r).to_le_bytes());
            }
            h.update(&s.cpsr().to_le_bytes());
            h.update(s.wram());
            h.update(s.iwram());
            h.update(self.driver_blob(i));
        }
        h.finalize()
    }
}

/// Per-core boot configuration beyond the ROM itself.
#[derive(Default)]
pub struct SideOptions {
    pub rom: Vec<u8>,
    /// SRAM/flash image, if resuming from an existing save.
    pub save: Option<Vec<u8>>,
}

#[derive(Default)]
pub struct PairOptions {
    pub sides: [SideOptions; 2],
    /// Pin both carts' RTC to a fixed clock. Mandatory for netplay/replay
    /// of RTC-bearing games (e.g. BN4.5): both peers must negotiate the
    /// same match clock or the pair diverges on the first RTC read.
    pub rtc: Option<std::time::SystemTime>,
}

impl Pair {
    /// Boot a linked pair from two ROM images. Core 0 requests lockstep
    /// player 0 (primary/master side), core 1 requests player 1.
    pub fn new(roms: [Vec<u8>; 2]) -> Result<Self, mgba::Error> {
        let [rom0, rom1] = roms;
        Self::with_options(PairOptions {
            sides: [
                SideOptions { rom: rom0, save: None },
                SideOptions { rom: rom1, save: None },
            ],
            rtc: None,
        })
    }

    pub fn with_options(options: PairOptions) -> Result<Self, mgba::Error> {
        let mut coordinator = mgba::sio::Coordinator::new();
        let core_options = mgba::core::Options::default();

        let mut cores = [
            mgba::core::Core::new_gba("tango-siolink", &core_options)?,
            mgba::core::Core::new_gba("tango-siolink", &core_options)?,
        ];
        let mut drivers = [
            mgba::sio::Driver::new(&mut coordinator, 0),
            mgba::sio::Driver::new(&mut coordinator, 1),
        ];

        let mut sides = options.sides.into_iter();
        for (core, driver) in cores.iter_mut().zip(drivers.iter_mut()) {
            let side = sides.next().unwrap();
            core.enable_video_buffer();
            core.as_mut().load_rom(mgba::vfile::VFile::from_vec(side.rom))?;
            if let Some(save) = side.save {
                core.as_mut().load_save(mgba::vfile::VFile::from_vec(save))?;
            }
            if let Some(rtc) = options.rtc {
                core.set_rtc_fixed(rtc);
            }
            driver.install(&mut core.as_mut());
            core.as_mut().reset();
        }

        Ok(Pair {
            cores,
            drivers,
            coordinator,
        })
    }

    pub fn core(&self, i: usize) -> mgba::core::CoreRef<'_> {
        self.cores[i].as_ref()
    }

    pub fn core_mut(&mut self, i: usize) -> mgba::core::CoreMutRef<'_> {
        self.cores[i].as_mut()
    }

    pub fn player_id(&self, i: usize) -> i32 {
        self.drivers[i].player_id()
    }

    /// Core `i`'s rendered frame (240x160, mgba's native 16-bit XBGR1555),
    /// for frontends.
    pub fn video_buffer(&self, i: usize) -> Option<&[u8]> {
        self.cores[i].video_buffer()
    }

    /// Advance the pair by one frame of the reference core, interleaving
    /// run_loop slices between whichever cores the lockstep protocol
    /// currently allows to run. `keys[i]` is latched for core `i` at the
    /// start of the tick — the fixed sequence point that makes the key
    /// schedule (and therefore the whole pair) deterministic and
    /// replayable.
    ///
    /// Returns the number of slices run (diagnostic only).
    pub fn tick(&mut self, keys: [u32; 2]) -> usize {
        for (core, &k) in self.cores.iter_mut().zip(keys.iter()) {
            core.as_mut().set_keys(k);
        }

        let target = self.cores[REFERENCE].as_ref().frame_counter().wrapping_add(1);
        let mut slices = 0;
        while self.cores[REFERENCE].as_ref().frame_counter() != target {
            let mut progressed = false;
            for i in 0..2 {
                if self.drivers[i].asleep() {
                    continue;
                }
                if i == REFERENCE && self.cores[REFERENCE].as_ref().frame_counter() == target {
                    continue;
                }
                self.cores[i].as_mut().run_loop();
                progressed = true;
                slices += 1;
            }
            if !progressed {
                // _verifyAwake on the C side guarantees not everyone sleeps;
                // reaching this means the pair state is corrupt.
                panic!("sio lockstep pair deadlocked: all cores asleep");
            }
            if slices > MAX_SLICES_PER_TICK {
                panic!(
                    "sio lockstep pair livelocked: {} slices without finishing a reference frame",
                    slices
                );
            }
        }
        slices
    }

    /// Snapshot the full pair. Valid at any tick boundary, including with a
    /// transfer in flight or either core parked by the lockstep protocol.
    pub fn save(&mut self) -> Result<Snapshot, mgba::Error> {
        Ok(Snapshot {
            cores: [
                self.cores[0].as_mut().save_state()?,
                self.cores[1].as_mut().save_state()?,
            ],
            drivers: [self.drivers[0].save_state(), self.drivers[1].save_state()],
        })
    }

    /// Restore a snapshot taken from THIS pair (same attach configuration).
    /// Core states load first — a core load rebuilds its timing list, which
    /// the driver blob then re-schedules the lockstep event into.
    pub fn load(&mut self, snapshot: &Snapshot) -> Result<(), mgba::Error> {
        for (core, state) in self.cores.iter_mut().zip(snapshot.cores.iter()) {
            core.as_mut().load_state(state)?;
        }
        for (driver, blob) in self.drivers.iter_mut().zip(snapshot.drivers.iter()) {
            if !driver.load_state(blob) {
                return Err(mgba::Error::CallFailed("GBASIOLockstepDriver::loadState"));
            }
        }
        Ok(())
    }
}
