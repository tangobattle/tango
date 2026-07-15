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
pub mod throttler;

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
    /// Each core's SIO transfer-completion event (`GBASIO::completeEvent`):
    /// `Some(cycles until it fires)` when scheduled — negative when it has
    /// come due but not yet been processed — or `None` when idle. Captured
    /// from the timing list itself because the core savestate's own record
    /// of this event is lossy; see [`sio_complete_state`].
    sio_complete: [Option<i32>; 2],
    /// Each core's direct-sound FIFO channels (A, B), captured verbatim
    /// because the core savestate's encoding is lossy; see
    /// [`audio_fifo_state`].
    audio_fifos: [[FifoLane; 2]; 2],
    /// Each core's internal DMA control state (all four channels), captured
    /// verbatim because the core savestate reconstructs it from the io
    /// block, which diverges from the truth mid-FIFO-refill; see
    /// [`dma_state`].
    dmas: [[DmaLane; 4]; 2],
}

/// Raw image of one direct-sound FIFO channel (`GBAAudioFIFO`): ring
/// contents, absolute ring pointers, and the internal sample countdown.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FifoLane {
    fifo: [u32; 8],
    write: i32,
    read: i32,
    internal_remaining: i32,
}

/// Raw image of one DMA channel's control state (`GBADMA`): the internal
/// control register plus the values derived from it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct DmaLane {
    reg: u16,
    cycles: i32,
    source_offset: i32,
    dest_offset: i32,
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
            h.update(&[self.sio_complete[i].is_some() as u8]);
            h.update(&self.sio_complete[i].unwrap_or(0).to_le_bytes());
            for lane in &self.audio_fifos[i] {
                for w in &lane.fifo {
                    h.update(&w.to_le_bytes());
                }
                h.update(&lane.write.to_le_bytes());
                h.update(&lane.read.to_le_bytes());
                h.update(&lane.internal_remaining.to_le_bytes());
            }
            for dma in &self.dmas[i] {
                h.update(&dma.reg.to_le_bytes());
                h.update(&dma.cycles.to_le_bytes());
                h.update(&dma.source_offset.to_le_bytes());
                h.update(&dma.dest_offset.to_le_bytes());
            }
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
            mgba::core::Core::new_gba("mgba-siolink", &core_options)?,
            mgba::core::Core::new_gba("mgba-siolink", &core_options)?,
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

    /// Install instruction traps on core `i` (see `mgba::core::Core::set_traps`).
    /// The core owns the trapper, which is the only sound ownership: the
    /// trapper splices itself into the core's CPU component table and has
    /// no uninstall, so the core dereferences it right up through its own
    /// deinit — a trapper held anywhere else can be freed first and turn
    /// core teardown into a jump through reclaimed memory. `Core`'s drop
    /// order (deinit, then fields) keeps the trapper alive exactly long
    /// enough.
    pub fn set_traps(&mut self, i: usize, traps: Vec<(u32, Box<dyn Fn(mgba::core::CoreMutRef)>)>) {
        self.cores[i].set_traps(traps);
    }

    /// Set core `i`'s video frameskip: `i32::MAX` never renders, `0`
    /// renders every frame. Rendering is invisible to the emulated machine
    /// and frameskip is not serialized, so this is rollback-safe — it
    /// survives `load` and cannot perturb snapshot digests. Skip whichever
    /// cores nobody is watching: the remote side during live play, both
    /// sides while re-simulating.
    pub fn set_frameskip(&mut self, i: usize, frameskip: i32) {
        self.cores[i].as_mut().gba_mut().set_frameskip(frameskip);
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
        let mut cores = [
            self.cores[0].as_mut().save_state()?,
            self.cores[1].as_mut().save_state()?,
        ];
        for state in &mut cores {
            normalize_cpu_cycles(state);
        }
        Ok(Snapshot {
            cores,
            drivers: [self.drivers[0].save_state(), self.drivers[1].save_state()],
            sio_complete: [
                sio_complete_state(&mut self.cores[0]),
                sio_complete_state(&mut self.cores[1]),
            ],
            audio_fifos: [
                audio_fifo_state(&mut self.cores[0]),
                audio_fifo_state(&mut self.cores[1]),
            ],
            dmas: [dma_state(&mut self.cores[0]), dma_state(&mut self.cores[1])],
        })
    }

    /// Restore a snapshot taken from THIS pair (same attach configuration).
    /// Core states load first — a core load rebuilds its timing list, which
    /// the driver blob then re-schedules the lockstep event into. In
    /// between, each core's SIO completion event is forced back to the
    /// recorded truth (the core load's own restore of it is lossy; see
    /// [`sio_complete_state`]) so an exact tie with the lockstep event
    /// keeps the completion first in the timing list, matching both the
    /// vanilla restore order and the live scheduling order.
    pub fn load(&mut self, snapshot: &Snapshot) -> Result<(), mgba::Error> {
        for (i, (core, state)) in self.cores.iter_mut().zip(snapshot.cores.iter()).enumerate() {
            defuse_sio_mode_switch(core, state);
            core.as_mut().load_state(state)?;
            restore_sio_complete(core, snapshot.sio_complete[i]);
            restore_audio_fifos(core, &snapshot.audio_fifos[i]);
            restore_dmas(core, &snapshot.dmas[i]);
        }
        for (driver, blob) in self.drivers.iter_mut().zip(snapshot.drivers.iter()) {
            if !driver.load_state(blob) {
                return Err(mgba::Error::CallFailed("GBASIOLockstepDriver::loadState"));
            }
        }
        Ok(())
    }
}

/// Pre-set `GBASIO::mode` to the mode the incoming state derives, so the
/// core load's own SIO touch-up cannot fire the lockstep driver mid-load.
///
/// `GBAIODeserialize` ends with `GBASIOWriteRCNT(&gba->sio, io[RCNT])`,
/// whose `_switchMode` compares the LOADED registers' mode against the
/// PRE-LOAD `sio->mode` and, when they differ (i.e. the rollback window
/// spans a link-mode switch — routine for bn1, which hops between NORMAL
/// and MULTI), calls the lockstep driver's `setMode` while the pair is
/// half-restored: the core's clock has been rewound but the coordinator's
/// has not. For player 0 that path runs
/// `GBASIOLockstepCoordinatorWaitOnPlayers` → `_advanceCycle`, whose
/// `newCycle - coordinator->cycle >= 0` assert fires (the crash at
/// lockstep.c:700); it also force-sleeps the player and zeroes
/// `cpu->nextEvent` AFTER the state restored it. For player 1 it enqueues
/// a phantom `SIO_EV_MODE_SET` into the other player's queue. All of it is
/// spurious — the driver blobs loaded right after this carry the true
/// mode/queue/coordinator state — so make `_switchMode` see "no change"
/// and do nothing.
fn defuse_sio_mode_switch(core: &mut mgba::core::Core, state: &mgba::state::State) {
    const OFF_IO_SIOCNT: usize = 0x400 + 0x128;
    const OFF_IO_RCNT: usize = 0x400 + 0x134;
    let st = state.as_slice();
    let io16 = |off: usize| u16::from_le_bytes(st[off..off + 2].try_into().unwrap());
    // Mode derivation per sio.c's _switchMode.
    let mode = ((io16(OFF_IO_RCNT) & 0xc000) | (io16(OFF_IO_SIOCNT) & 0x3000)) >> 12;
    let mode = if mode < 8 { mode & 0x3 } else { mode & 0xc };
    unsafe {
        (*gba_ptr(core)).sio.mode = mode as mgba_sys::GBASIOMode;
    }
}

/// Rebase a serialized core state so `cpu.cycles` is non-negative.
///
/// A core parked by the lockstep protocol mid-event-batch can end its slice
/// with `cpu->cycles == cpu->nextEvent < 0` (`GBAProcessEvents` copies the
/// overdue next-event offset into `cycles` while the CPU is DMA-blocked),
/// which is a perfectly healthy live state — but `GBADeserialize` rejects
/// any negative `cpu.cycles` as a corrupted savestate, so the snapshot
/// would refuse to load (and in a netplay session that half-applies the
/// pair restore: one core loaded, the other left at the pre-load tick —
/// the lockstep clocks then disagree and the coordinator's
/// `_advanceCycle` assert fires).
///
/// Only the SUM `masterCycles + cycles` (the current time) and the DISTANCE
/// `nextEvent - cycles` are meaningful — every serialized event offset is
/// relative to the sum, and the run loop only compares `cycles` against
/// `nextEvent`. Folding `cycles` into `masterCycles` (and `globalCycles`,
/// its debugger twin) is therefore behaviorally exact: the restored core
/// executes the same instructions and processes the same events at the
/// same absolute times as the live one.
fn normalize_cpu_cycles(state: &mut mgba::state::State) {
    // GBASerializedState offsets (gba/serialize.h): masterCycles, cpu.cycles,
    // cpu.nextEvent, globalCycles.
    const OFF_MASTER_CYCLES: usize = 0x00c;
    const OFF_CPU_CYCLES: usize = 0x068;
    const OFF_CPU_NEXT_EVENT: usize = 0x06c;
    const OFF_GLOBAL_CYCLES: usize = 0x310;
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(
            state as *mut mgba::state::State as *mut u8,
            std::mem::size_of::<mgba::state::State>(),
        )
    };
    let read_i32 = |b: &[u8], off: usize| i32::from_le_bytes(b[off..off + 4].try_into().unwrap());
    let cycles = read_i32(bytes, OFF_CPU_CYCLES);
    if cycles >= 0 {
        return;
    }
    let master = read_i32(bytes, OFF_MASTER_CYCLES).wrapping_add(cycles);
    let next_event = read_i32(bytes, OFF_CPU_NEXT_EVENT).wrapping_sub(cycles);
    let global = i64::from_le_bytes(bytes[OFF_GLOBAL_CYCLES..OFF_GLOBAL_CYCLES + 8].try_into().unwrap())
        .wrapping_add(cycles as i64);
    bytes[OFF_MASTER_CYCLES..OFF_MASTER_CYCLES + 4].copy_from_slice(&master.to_le_bytes());
    bytes[OFF_CPU_CYCLES..OFF_CPU_CYCLES + 4].copy_from_slice(&0i32.to_le_bytes());
    bytes[OFF_CPU_NEXT_EVENT..OFF_CPU_NEXT_EVENT + 4].copy_from_slice(&next_event.to_le_bytes());
    bytes[OFF_GLOBAL_CYCLES..OFF_GLOBAL_CYCLES + 8].copy_from_slice(&global.to_le_bytes());
}

/// Raw C-side view of a core's GBA, for the completion-event surgery below.
/// The binding crate exposes no timing-event API and must not be modified
/// (nor may mgba's C side); `GBAMutRef` is `#[repr(transparent)]` over
/// `*mut mgba_sys::GBA`, which makes the transmute layout-sound, and the
/// `mgba-sys` dependency comes from the same git source as `mgba` itself so
/// the types are the same crate's.
fn gba_ptr(core: &mut mgba::core::Core) -> *mut mgba_sys::GBA {
    unsafe { std::mem::transmute(core.as_mut().gba_mut()) }
}

/// Capture the true scheduling of a core's SIO transfer-completion event
/// (`GBASIO::completeEvent`): `Some(cycles until fire)` or `None`.
///
/// This must ride in the pair snapshot because the core savestate is lossy
/// here. mgba serializes the event as `hw.sioNextEvent = when - now` with
/// no "scheduled" bit and restores it in `GBAHardwareDeserialize`
/// (gba/cart/gpio.c) behind the legacy heuristic
/// `(SIOCNT & 0x0080) && (uint32_t)stored < 0x20000`, which is wrong in
/// both directions under the cooperative lockstep interleave:
///
/// - A completion that has come due but not yet run — relative `when`
///   slightly NEGATIVE — is dropped: `GBASIOLockstepPlayerSleep` force-
///   exits a run slice mid-event-batch (`cpu->nextEvent = 0` +
///   `GBAInterrupt`), so tick boundaries regularly park a core with the
///   completion pending-overdue. Stored negative, it reads back as a huge
///   unsigned value and the restored machine never finishes the in-flight
///   transfer: the re-simulation forks from the original run on the very
///   next slice. bn1/bn2/bn3 keep MULTI/NORMAL transfers in flight across
///   nearly every tick boundary, which is why they trip this constantly
///   while bn4-6 idle through most boundaries.
/// - Conversely a stale `hw.sioNextEvent` that happens to land in
///   `[0, 0x20000)` while the busy/start bit is set (a secondary can hold
///   START without the driver ever scheduling a completion) would be
///   restored SPURIOUSLY, conjuring a finish the live machine never had.
///
/// Recording the truth at save and forcing it at load sidesteps the
/// heuristic entirely.
fn sio_complete_state(core: &mut mgba::core::Core) -> Option<i32> {
    unsafe {
        let gba = gba_ptr(core);
        let timing = std::ptr::addr_of_mut!((*gba).timing);
        let event = std::ptr::addr_of_mut!((*gba).sio.completeEvent);
        if mgba_sys::mTimingIsScheduled(timing, event) {
            Some(mgba_sys::mTimingUntil(timing, event))
        } else {
            None
        }
    }
}

/// Force a core's SIO completion event to the recorded scheduling, exactly
/// reproducing the live machine: an overdue completion fires first thing
/// next slice with the same `cyclesLate` the live run saw (or stays frozen
/// until the lockstep protocol wakes a parked core), and a spurious restore
/// is removed. Call after the owning core's `load_state` (which rebuilt the
/// timing list) and before the driver blob load, so an exact-timestamp tie
/// with the lockstep event resolves the same way the C restore path orders
/// them (completion first — also the live order, since the lockstep event
/// re-schedules itself at the end of every `_lockstepEvent` firing).
fn restore_sio_complete(core: &mut mgba::core::Core, scheduled: Option<i32>) {
    unsafe {
        let gba = gba_ptr(core);
        let timing = std::ptr::addr_of_mut!((*gba).timing);
        let event = std::ptr::addr_of_mut!((*gba).sio.completeEvent);
        // Descheduling an unscheduled event is a harmless no-op; scheduling
        // a scheduled one corrupts the list, so always deschedule first.
        mgba_sys::mTimingDeschedule(timing, event);
        if let Some(when) = scheduled {
            mgba_sys::mTimingSchedule(timing, event, when);
        }
    }
}

/// Capture a core's direct-sound FIFO channels verbatim.
///
/// This must ride in the pair snapshot because the core savestate's
/// encoding is lossy: `GBAAudioSerialize` packs each channel's
/// `internalRemaining` — which counts 4..0 samples left in the popped
/// word — into a TWO-bit legacy field (`FIFOInternalSamplesA/B`), so the
/// common value 4 aliases to 0. A restored core then pops its next FIFO
/// word up to 4 sample-events early, drains the FIFO faster than the live
/// machine did, and crosses the DMA refill threshold (fill < 4) at a
/// different timer overflow — the refill DMA steals ~10 bus cycles at a
/// point in time where the live run had none, and the whole pair's
/// interleave forks from there. (The serializer also normalizes the ring
/// to `read == 0`, which is behaviorally invisible except for the
/// open-bus-ish value `GBAAudioWriteFIFO` returns from the next slot;
/// carrying the raw ring makes the round trip exact rather than merely
/// equivalent.)
fn audio_fifo_state(core: &mut mgba::core::Core) -> [FifoLane; 2] {
    unsafe {
        let gba = gba_ptr(core);
        let lane = |ch: *const mgba_sys::GBAAudioFIFO| FifoLane {
            fifo: (*ch).fifo,
            write: (*ch).fifoWrite,
            read: (*ch).fifoRead,
            internal_remaining: (*ch).internalRemaining,
        };
        [
            lane(std::ptr::addr_of!((*gba).audio.chA)),
            lane(std::ptr::addr_of!((*gba).audio.chB)),
        ]
    }
}

/// Force a core's direct-sound FIFO channels back to the recorded truth,
/// after the core's `load_state` applied the lossy serialized version.
fn restore_audio_fifos(core: &mut mgba::core::Core, lanes: &[FifoLane; 2]) {
    unsafe {
        let gba = gba_ptr(core);
        for (ch, lane) in [
            std::ptr::addr_of_mut!((*gba).audio.chA),
            std::ptr::addr_of_mut!((*gba).audio.chB),
        ]
        .into_iter()
        .zip(lanes.iter())
        {
            (*ch).fifo = lane.fifo;
            (*ch).fifoWrite = lane.write;
            (*ch).fifoRead = lane.read;
            (*ch).internalRemaining = lane.internal_remaining;
        }
    }
}

/// Capture a core's internal DMA control state verbatim.
///
/// This must ride in the pair snapshot because the core savestate
/// reconstructs `GBADMA::reg` (and the `sourceOffset`/`destOffset`/`cycles`
/// values derived from it) from the io block's DMAxCNT_HI — but
/// `GBAAudioScheduleFifoDma` rewrites `reg` in place (dest control forced
/// to FIXED, width forced to 32-bit) WITHOUT updating the io block when a
/// FIFO refill is dispatched. A snapshot that lands while a refill is
/// pending or mid-block (routine here: `GBASIOLockstepPlayerSleep` parks a
/// core mid-event-batch, freezing an in-flight refill across the tick
/// boundary) restores the channel with the game's raw control instead: the
/// destination increments off the FIFO register, the width may be wrong,
/// and the re-simulated audio stream — and every bus cycle it steals —
/// forks from the original run.
fn dma_state(core: &mut mgba::core::Core) -> [DmaLane; 4] {
    unsafe {
        let gba = gba_ptr(core);
        std::array::from_fn(|i| {
            let dma = std::ptr::addr_of!((*gba).memory.dma[i]);
            DmaLane {
                reg: (*dma).reg,
                cycles: (*dma).cycles,
                source_offset: (*dma).sourceOffset,
                dest_offset: (*dma).destOffset,
            }
        })
    }
}

/// Force a core's internal DMA control state back to the recorded truth,
/// after the core's `load_state` reconstructed it from the io block.
fn restore_dmas(core: &mut mgba::core::Core, lanes: &[DmaLane; 4]) {
    unsafe {
        let gba = gba_ptr(core);
        for (i, lane) in lanes.iter().enumerate() {
            let dma = std::ptr::addr_of_mut!((*gba).memory.dma[i]);
            (*dma).reg = lane.reg;
            (*dma).cycles = lane.cycles;
            (*dma).sourceOffset = lane.source_offset;
            (*dma).destOffset = lane.dest_offset;
        }
    }
}
