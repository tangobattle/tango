//! Booting a recorded match's pair.
//!
//! The playback, seeking, and stats machinery is the seam's
//! ([`tango_match::replay`]): this module contributes [`Boot`] — the
//! engine's [`tango_match::ReplayBoot`], booting + priming a pair for
//! a recording — plus [`Playback`], the bare-pair linear re-sim that
//! `tango-replay-renderer` and the probe harnesses drive by hand for
//! the raw core access (savestate digests, the C resampler) the seam
//! deliberately doesn't expose.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::telemetry::LifecycleSink;
use crate::{GameSupport, PrimeConfig};

/// Cap on priming ticks, mirroring the live engine's bound.
const MAX_PRIME_TICKS: u32 = 3600;

/// Everything needed to boot a playback pair. All fields are in
/// **absolute** player order (core 0 runs player 0's game) — see
/// [`crate::analysis::AnalyzeConfig`] for the orientation contract.
pub struct BootConfig {
    pub roms: [Vec<u8>; 2],
    pub saves: [Vec<u8>; 2],
    pub support: [&'static (dyn GameSupport + Send + Sync); 2],
    pub match_type: (u8, u8),
    pub rng_seed: [u8; 16],
    pub rtc: std::time::SystemTime,
    /// Silence the battle BGM (see [`PrimeConfig::disable_bgm`]) — the
    /// export path's "disable BGM" render option. Doesn't need to match
    /// the recorded session's setting: the sound driver's state never
    /// feeds battle logic, so the input stream re-simulates identically.
    pub disable_bgm: bool,
}

/// Boot and prime a pair per `config`. With `render` unset, both cores
/// skip rasterization (the replay paths render anyway — their captures
/// feed thumbnails — but callers can opt out).
fn boot_and_prime(
    config: &BootConfig,
    render: bool,
    cancel: Option<&AtomicBool>,
    lifecycle: &LifecycleSink,
) -> Result<mgba_rollback::Link, crate::Error> {
    crate::install_logger();
    let mut pair = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
        sides: vec![
            mgba_rollback::SideOptions {
                rom: config.roms[0].clone(),
                save: Some(config.saves[0].clone()),
            },
            mgba_rollback::SideOptions {
                rom: config.roms[1].clone(),
                save: Some(config.saves[1].clone()),
            },
        ],
        rtc: Some(config.rtc),
        peripheral: mgba_rollback::Peripheral::Cable,
    })?;
    if !render {
        pair.set_frameskip(0, i32::MAX);
        pair.set_frameskip(1, i32::MAX);
    }

    let prime_config = PrimeConfig {
        match_type: config.match_type,
        rng_seed: config.rng_seed,
        disable_bgm: config.disable_bgm,
    };
    let primed = [crate::PrimedLatch::new(), crate::PrimedLatch::new()];
    // Cores own their primer traps — see [`mgba_rollback::Link::set_traps`]
    // for why any other ownership dangles at core teardown.
    pair.set_traps(
        0,
        config.support[0].primer_traps(&prime_config, 0, lifecycle, &primed[0]),
    );
    pair.set_traps(
        1,
        config.support[1].primer_traps(&prime_config, 1, lifecycle, &primed[1]),
    );

    let mut prime_ticks = 0;
    while !(primed[0].is_set() && primed[1].is_set()) {
        if prime_ticks >= MAX_PRIME_TICKS {
            return Err(crate::Error::PrimeTimeout(MAX_PRIME_TICKS));
        }
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(crate::Error::Cancelled);
        }
        pair.tick(&[0, 0]);
        prime_ticks += 1;
    }
    Ok(pair)
}

/// The engine's replay boot ([`tango_match::ReplayBoot`]): prime a pair
/// per the boot configuration and hand it over as the seam's link, with
/// the game's telemetry wired when the stats pass asks for it. This is
/// all the engine contributes to replays — the machinery above it is
/// [`tango_match::ReplaySet`]'s.
pub struct Boot(pub BootConfig);

impl tango_match::ReplayBoot for Boot {
    fn boot(&self, observe: bool, cancel: &AtomicBool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        let lifecycle = LifecycleSink::new();
        let pair = boot_and_prime(&self.0, true, Some(cancel), &lifecycle).map_err(tango_match::Error::from)?;
        let (telemetry, handle) = if observe {
            let (telemetry, handle) = crate::telemetry::Telemetry::new(
                [self.0.support[0].core_poller(0), self.0.support[1].core_poller(1)],
                lifecycle,
            );
            (Some(telemetry), Some(handle))
        } else {
            // The display pair pays for no pollers; its lifecycle sink
            // is a write-only stub.
            (None, None)
        };
        Ok(tango_match::BootedReplay {
            link: Box::new(crate::Link::new(pair, telemetry)),
            telemetry: handle,
        })
    }
}

/// A whole-pair snapshot poised at a tick (= input pairs consumed),
/// carrying both cores' rendered frames — the raw-pair counterpart of
/// the seam's [`tango_match::Capture`], for harnesses that need the
/// mgba state itself back (savestate digests, jump-started renders).
pub struct Snapshot {
    pub state: mgba_rollback::Snapshot,
    /// Both cores' rendered frames at this tick, seam-ready (RGBA8).
    pub frames: tango_match::LiveFrames,
}

impl Snapshot {
    /// The tick this snapshot is poised at.
    pub fn tick(&self) -> u32 {
        self.frames.tick
    }
}

/// A bare playback pair: booted, primed, and fed the recorded stream by
/// hand. The player rides the seam's machinery instead — this exists
/// for the video renderer and the probe harnesses, which reach past the
/// seam into the cores.
pub struct Playback {
    pair: mgba_rollback::Link,
    inputs: Arc<Vec<[u32; 2]>>,
    cursor: u32,
}

impl Playback {
    /// Boot + prime a rendering pair poised at tick 0. Takes seconds of
    /// wall clock (a few hundred priming ticks) — call off the UI
    /// thread. `lifecycle` receives the pair's trap-fired round events;
    /// callers with no observer pass a fresh write-only stub.
    pub fn new(config: &BootConfig, inputs: Arc<Vec<[u32; 2]>>, lifecycle: &LifecycleSink) -> Result<Self, crate::Error> {
        let pair = boot_and_prime(config, true, None, lifecycle)?;
        Ok(Self {
            pair,
            inputs,
            cursor: 0,
        })
    }

    /// Input pairs consumed so far = the playhead tick.
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    pub fn total(&self) -> u32 {
        self.inputs.len() as u32
    }

    pub fn at_end(&self) -> bool {
        self.cursor >= self.total()
    }

    /// Feed the next recorded input pair. Returns false at end-of-stream.
    pub fn step(&mut self) -> bool {
        let Some(&keys) = self.inputs.get(self.cursor as usize) else {
            return false;
        };
        self.pair.tick(&keys);
        self.cursor += 1;
        true
    }

    /// Capture a whole-pair snapshot (with both frames) at the current
    /// cursor.
    pub fn capture(&mut self) -> Result<Arc<Snapshot>, crate::Error> {
        let state = self.pair.save()?;
        let frames = [
            self.pair.video_buffer(0).map(crate::link::to_rgba).unwrap_or_default(),
            self.pair.video_buffer(1).map(crate::link::to_rgba).unwrap_or_default(),
        ];
        Ok(Arc::new(Snapshot {
            state,
            frames: tango_match::LiveFrames {
                tick: self.cursor,
                frames,
            },
        }))
    }

    /// Restore the pair to `snap` and move the cursor there.
    pub fn load(&mut self, snap: &Snapshot) -> Result<(), crate::Error> {
        self.pair.load(&snap.state)?;
        self.cursor = snap.tick();
        Ok(())
    }

    /// Direct pair access, for video/audio readout.
    pub fn pair_mut(&mut self) -> &mut mgba_rollback::Link {
        &mut self.pair
    }
}
