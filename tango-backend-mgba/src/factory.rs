//! This engine as a game registration holds it.
//!
//! A `Game` registration cannot name an emulator — that is the whole
//! point of [`tango_match::MatchFactory`] — so everything a host asks
//! of the mgba engine arrives through one object per registered
//! cartridge. [`GbaFactory`] is that object: it closes over the
//! cartridge's own [`GameSupport`](crate::GameSupport) and, for the
//! seat it does not own, resolves the peer's out of the family table
//! its crate hands it.
//!
//! Netplay, a single-player ride, and replay playback all come out of
//! here, so `tango-session` drives a GBA game through exactly the same
//! calls it drives a DS one through, and never learns which is which.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::r#match::playback;

/// One cartridge in a family, keyed as its ROM header names it.
pub type Seat = (&'static [u8; 4], u8, &'static (dyn crate::GameSupport + Send + Sync));

/// A GBA cartridge's engine support, as the engine-neutral factory its
/// registration holds.
///
/// A match needs *both* seats' support and a factory hangs off one
/// game, so the peer arrives as a [`PeerRom`](tango_match::PeerRom)
/// looked up in `family` — the table its own crate declares, which is
/// the only place that knows what its siblings are. Crossplay is why
/// this is a lookup rather than a constant: a Japanese cart links with
/// an American one, and each seat needs the support for the ROM
/// actually in it.
pub struct GbaFactory {
    local: &'static (dyn crate::GameSupport + Send + Sync),
    family: &'static [Seat],
}

impl GbaFactory {
    pub const fn new(local: &'static (dyn crate::GameSupport + Send + Sync), family: &'static [Seat]) -> Self {
        GbaFactory { local, family }
    }

    /// The peer's support, or the local cart's if the family doesn't
    /// list what the peer says it is running. Falling back rather than
    /// failing keeps a mismatched revision playable-if-desynced instead
    /// of unstartable, which is what the engine did before this lookup
    /// existed.
    fn peer(&self, peer: tango_match::PeerRom) -> &'static (dyn crate::GameSupport + Send + Sync) {
        self.family
            .iter()
            .find(|(code, revision, _)| **code == peer.code && *revision == peer.revision)
            .map(|(_, _, support)| *support)
            .unwrap_or(self.local)
    }

    /// Both seats' support in seat order, which is not the same as
    /// local-and-peer: seat 0 is player 0 whoever that is.
    fn seats(&self, config: &tango_match::StartConfig) -> [&'static (dyn crate::GameSupport + Send + Sync); 2] {
        let mut seats = [self.local, self.peer(config.peer_rom)];
        if config.local_player == 1 {
            seats.swap(0, 1);
        }
        seats
    }

    fn boot_config(&self, config: &tango_match::ReplayConfig) -> playback::BootConfig {
        let mut support = [self.local, self.peer(config.peer_rom)];
        if config.local_player == 1 {
            support.swap(0, 1);
        }
        playback::BootConfig {
            roms: config.roms.clone(),
            saves: config.saves.clone(),
            support,
            match_type: config.match_type,
            rng_seed: config.rng_seed,
            rtc: config.rtc,
            disable_bgm: false,
        }
    }
}

impl tango_match::MatchFactory for GbaFactory {
    fn screen_layout(&self) -> tango_match::ScreenLayout {
        crate::link::screen_layout()
    }

    fn start(&self, config: tango_match::StartConfig) -> Result<tango_match::Match, tango_match::Error> {
        crate::r#match::engine::start(crate::r#match::engine::MatchConfig {
            roms: [config.roms[0].to_vec(), config.roms[1].to_vec()],
            saves: [
                config.saves[0].unwrap_or_default().to_vec(),
                config.saves[1].unwrap_or_default().to_vec(),
            ],
            support: self.seats(&config).map(|s| s as &dyn crate::GameSupport),
            match_type: config.match_type,
            rng_seed: config.rng_seed,
            rtc: config.rtc,
            local_player: config.local_player,
            present_delay: config.present_delay,
            disable_bgm: config.disable_bgm,
        })
    }

    fn chip_semantics(&self, rom: &[u8]) -> Option<(tango_match::analysis::ChipSemantics, bool)> {
        Some((self.local.chip_semantics(rom), self.local.counts_buster(rom)))
    }

    fn start_solo(
        &self,
        config: tango_match::SoloConfig,
    ) -> Result<Box<dyn tango_match::RunningSolo>, tango_match::Error> {
        Ok(Box::new(Solo::new(config).map_err(tango_match::Error::from)?))
    }

    fn open_replay(
        &self,
        config: tango_match::ReplayConfig,
    ) -> Result<Box<dyn tango_match::ReplaySet>, tango_match::Error> {
        let boot = self.boot_config(&config);
        // The pass reads chip use off the *local* seat's cart, which is
        // the one whose statistics a viewer is being shown.
        let semantics = config.want_stats.then(|| {
            let rom = &config.roms[config.local_player];
            (
                boot.support[config.local_player].chip_semantics(rom),
                boot.support[config.local_player].counts_buster(rom),
            )
        });
        // The recorded rows arrive in the seam's vocabulary; the GBA's
        // cores speak bare joypad words, so the conversion happens once
        // here rather than on every tick of both simulations.
        let inputs = Arc::new(config.inputs.iter().map(|row| row.map(|input| input.keys)).collect());
        Ok(Box::new(Set {
            boot,
            inputs,
            local_player: config.local_player,
            semantics,
            // Shared: the pass racing ahead lays the keyframes a seek
            // behind the playhead lands on.
            store: playback::SnapshotStore::new(),
            progress: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            round_marks: config.want_round_marks.then(|| Arc::new(Mutex::new(Vec::new()))),
            cancel: Arc::new(AtomicBool::new(false)),
        }))
    }
}

/// One recording's two simulations, each booted when its host thread
/// asks for it.
struct Set {
    boot: playback::BootConfig,
    inputs: Arc<Vec<[u32; 2]>>,
    local_player: usize,
    semantics: Option<(tango_match::analysis::ChipSemantics, bool)>,
    store: playback::SnapshotStore,
    progress: Arc<std::sync::atomic::AtomicU32>,
    round_marks: Option<Arc<Mutex<Vec<u32>>>>,
    cancel: Arc<AtomicBool>,
}

impl tango_match::ReplaySet for Set {
    fn playback(&self) -> Result<Box<dyn tango_match::RunningReplay>, tango_match::Error> {
        Ok(Box::new(
            Replay::new(&self.boot, self.inputs.clone(), self.store.clone(), &self.cancel)
                .map_err(tango_match::Error::from)?,
        ))
    }

    fn stats(&self) -> Result<Box<dyn tango_match::StatsPass>, tango_match::Error> {
        Ok(Box::new(
            Stats::new(
                &self.boot,
                self.inputs.clone(),
                self.local_player,
                self.store.clone(),
                self.progress.clone(),
                self.round_marks.clone(),
                self.cancel.clone(),
                self.semantics,
            )
            .map_err(tango_match::Error::from)?,
        ))
    }

    fn stats_progress(&self) -> Arc<std::sync::atomic::AtomicU32> {
        self.progress.clone()
    }

    fn round_marks(&self) -> Option<Arc<Mutex<Vec<u32>>>> {
        self.round_marks.clone()
    }

    fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// One console on a one-side link — which is what every session kind
/// here runs on, so the cart sees its link hardware from power-on and a
/// future netplay handoff has a cable to plug into.
struct Solo {
    link: Arc<Mutex<mgba_rollback::Link>>,
}

impl Solo {
    fn new(config: tango_match::SoloConfig) -> Result<Self, crate::Error> {
        crate::install_logger();
        let mut link = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
            sides: vec![mgba_rollback::SideOptions {
                rom: config.rom.to_vec(),
                save: config.save.map(<[u8]>::to_vec),
            }],
            rtc: config.rtc,
            peripheral: mgba_rollback::Peripheral::Cable,
        })?;
        // This buffer *is* the session's audio queue: the stream leaves
        // its backlog here rather than pulling it out, so a rollback can
        // still revoke what it speculated. It therefore has to hold the
        // stream's discard cap — 3x a 120 ms target — plus what
        // fast-forward piles up between fills, at BN4+'s 65536 Hz, and
        // with room to spare: mGBA's ring drops new writes when full, so
        // overflowing it loses audio silently. Same sizing as the pair
        // engine.
        link.core_mut(0).set_audio_buffer_size(32768);
        link.core_mut(0).audio_buffer().clear();
        Ok(Solo {
            link: Arc::new(Mutex::new(link)),
        })
    }
}

impl tango_match::RunningSolo for Solo {
    fn tick(&mut self, input: tango_match::HostInput) -> Result<(), tango_match::Error> {
        // A GBA has no touch screen, so only the pad half applies.
        self.link
            .lock()
            .unwrap()
            .try_tick(&[input.keys])
            .map_err(|e| tango_match::Error::Backend(Box::new(crate::Error::from(e))))?;
        Ok(())
    }

    fn frame(&mut self) -> Option<Vec<u8>> {
        let link = self.link.lock().unwrap();
        link.video_buffer(0).map(to_rgba)
    }

    fn export_save(&self) -> Option<Vec<u8>> {
        self.link.lock().unwrap().export_save(0)
    }

    fn audio(&self) -> Option<Box<dyn tango_match::AudioDrain>> {
        Some(Box::new(PairAudio::new(
            PairSource::Solo(self.link.clone()),
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        )))
    }
}

/// A recording being re-simulated, with the seek machinery that serves
/// it: keyframes across the whole replay, a rewind ring around the
/// playhead, and the chase that walks between them.
struct Replay {
    playback: Arc<Mutex<playback::Playback>>,
    store: playback::SnapshotStore,
    rewind: playback::RewindRing,
    chase: playback::SeekChase,
    len: u32,
}

impl Replay {
    fn new(
        boot: &playback::BootConfig,
        inputs: Arc<Vec<[u32; 2]>>,
        store: playback::SnapshotStore,
        cancel: &AtomicBool,
    ) -> Result<Self, crate::Error> {
        let len = inputs.len() as u32;
        // The display pair runs no telemetry observer — its lifecycle
        // sink is a write-only stub.
        let pb = playback::Playback::new_cancellable(
            boot,
            inputs,
            Some(cancel),
            &crate::r#match::telemetry::LifecycleSink::new(),
        )?;
        Ok(Replay {
            playback: Arc::new(Mutex::new(pb)),
            store,
            rewind: playback::RewindRing::new(),
            chase: playback::SeekChase::default(),
            len,
        })
    }
}

impl tango_match::RunningReplay for Replay {
    fn step(&mut self) -> bool {
        let mut pb = self.playback.lock().unwrap();
        if pb.at_end() {
            return false;
        }
        pb.step();
        match pb.capture() {
            Ok(snap) => {
                if self.store.snapshot_needed(snap.tick) {
                    self.store.push(snap.clone());
                }
                self.rewind.insert(snap);
            }
            Err(e) => log::warn!("replay: frame capture failed: {e:?}"),
        }
        true
    }

    fn cursor(&self) -> u32 {
        self.playback.lock().unwrap().cursor()
    }

    fn len(&self) -> u32 {
        self.len
    }

    fn seek_step(
        &mut self,
        ctrl: &tango_match::seek::SeekController,
        budget: u32,
        on_progress: &mut dyn FnMut(u32),
        publish: &mut dyn FnMut(&dyn tango_match::ReplayFrames),
        on_resume: &mut dyn FnMut(),
    ) -> tango_match::SeekStep {
        let step = self.chase.step(
            ctrl,
            &self.playback,
            &self.store,
            &self.rewind,
            budget,
            on_progress,
            &mut |snap| publish(snap),
            on_resume,
        );
        match step {
            playback::ChaseStep::Idle => tango_match::SeekStep::Idle,
            playback::ChaseStep::Working => tango_match::SeekStep::Working,
            playback::ChaseStep::Landed => tango_match::SeekStep::Landed,
        }
    }

    fn frames(&mut self, publish: &mut dyn FnMut(&dyn tango_match::ReplayFrames)) {
        let mut pb = self.playback.lock().unwrap();
        let tick = pb.cursor();
        let pair = pb.pair_mut();
        let live = LiveFrames {
            tick,
            framebuffers: [
                pair.video_buffer(0).map(<[u8]>::to_vec).unwrap_or_default(),
                pair.video_buffer(1).map(<[u8]>::to_vec).unwrap_or_default(),
            ],
        };
        publish(&live);
    }

    fn audio(&self, seat: Arc<std::sync::atomic::AtomicUsize>) -> Option<Box<dyn tango_match::AudioDrain>> {
        Some(Box::new(PairAudio::new(
            PairSource::Playback(self.playback.clone()),
            seat,
        )))
    }

    fn nearest_capture(&self, tick: u32, publish: &mut dyn FnMut(&dyn tango_match::ReplayFrames)) -> bool {
        match self.best_at_or_before(tick) {
            Some(snap) => {
                publish(&*snap);
                true
            }
            None => false,
        }
    }

    fn capture_before(&self, tick: u32) -> Option<u32> {
        self.best_at_or_before(tick.checked_sub(1)?).map(|s| s.tick)
    }
}

impl Replay {
    /// The latest capture at or before `tick`, from either store — the
    /// ring holds the last second or so exactly, the keyframes cover
    /// the whole recording sparsely.
    fn best_at_or_before(&self, tick: u32) -> Option<Arc<playback::Snapshot>> {
        [self.store.best_at_or_before(tick), self.rewind.best_at_or_before(tick)]
            .into_iter()
            .flatten()
            .max_by_key(|s| s.tick)
    }
}

/// Both seats' displays right now, as the seam publishes them.
struct LiveFrames {
    tick: u32,
    framebuffers: [Vec<u8>; 2],
}

impl tango_match::ReplayFrames for LiveFrames {
    fn tick(&self) -> u32 {
        self.tick
    }

    fn frame(&self, player: usize) -> Vec<u8> {
        to_rgba(&self.framebuffers[player])
    }
}

impl tango_match::ReplayFrames for playback::Snapshot {
    fn tick(&self) -> u32 {
        self.tick
    }

    fn frame(&self, player: usize) -> Vec<u8> {
        to_rgba(&self.framebuffers[player])
    }
}

/// The statistics pass: a second pair running ahead of the viewer.
struct Stats(playback::Prefetch);

impl Stats {
    #[allow(clippy::too_many_arguments)]
    fn new(
        boot: &playback::BootConfig,
        inputs: Arc<Vec<[u32; 2]>>,
        local_player: usize,
        store: playback::SnapshotStore,
        progress: Arc<std::sync::atomic::AtomicU32>,
        round_marks: Option<Arc<Mutex<Vec<u32>>>>,
        cancel: Arc<AtomicBool>,
        semantics: Option<(tango_match::analysis::ChipSemantics, bool)>,
    ) -> Result<Self, crate::Error> {
        Ok(Stats(playback::Prefetch::open(
            boot,
            inputs,
            local_player,
            store,
            progress,
            round_marks,
            cancel,
            semantics,
        )?))
    }
}

impl tango_match::StatsPass for Stats {
    fn step(&mut self, budget: u32) -> Result<bool, tango_match::Error> {
        self.0.step(budget, None).map_err(tango_match::Error::from)
    }

    fn preview(&self) -> Option<tango_match::analysis::MatchStats> {
        self.0.preview()
    }

    fn finish(self: Box<Self>) -> Option<tango_match::analysis::MatchStats> {
        self.0.finish()
    }
}

/// Where a pull reads its samples from — the two shapes this engine
/// hands out, both behind the same mutex discipline.
enum PairSource {
    Solo(Arc<Mutex<mgba_rollback::Link>>),
    Playback(Arc<Mutex<playback::Playback>>),
}

impl PairSource {
    /// Run `f` on the pair if it is free. A pull never waits: a seek
    /// chase holds the pair for its whole walk, and blocking the sound
    /// callback on that stutters the device instead of the picture.
    fn try_with(&self, f: &mut dyn FnMut(&mut mgba_rollback::Link)) -> bool {
        match self {
            PairSource::Solo(link) => match link.try_lock() {
                Ok(mut link) => {
                    f(&mut link);
                    true
                }
                Err(_) => false,
            },
            PairSource::Playback(pb) => match pb.try_lock() {
                Ok(mut pb) => {
                    f(pb.pair_mut());
                    true
                }
                Err(_) => false,
            },
        }
    }
}

struct PairAudio {
    link: PairSource,
    /// Read per fill, so a perspective swap moves the sound without the
    /// resampler above it being rebuilt.
    player: Arc<std::sync::atomic::AtomicUsize>,
    /// Last successfully read sample rate and framerate ratio, as f64
    /// bits — served whenever a read lands while the drive thread holds
    /// the pair mid-tick. The stream re-reads both every fill, and it
    /// watches the ratio for the jump that means a deliberate speed
    /// change; a fast-forwarded drive loop holds the pair for a large
    /// fraction of wall time, so a placeholder 1.0 on those fills would
    /// read as the user toggling speed off and on again, snapping
    /// playback rate back and forth. A stale reading is only ever one
    /// fill old at a real transition.
    last_rate: AtomicU64,
    last_ratio: AtomicU64,
    /// Last readable buffer level and size — served when a reading
    /// lands mid-tick.
    last_queued: std::sync::atomic::AtomicUsize,
}

impl PairAudio {
    fn new(link: PairSource, player: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        PairAudio {
            link,
            player,
            // The GBA's own defaults, until the pair is first free.
            last_rate: AtomicU64::new(32768.0f64.to_bits()),
            last_ratio: AtomicU64::new(1.0f64.to_bits()),
            last_queued: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn player(&self) -> usize {
        self.player.load(Ordering::Relaxed)
    }
}


impl tango_match::AudioDrain for PairAudio {
    fn sample_rate(&self) -> f64 {
        let mut rate = 0.0;
        self.link
            .try_with(&mut |link| rate = link.core_mut(self.player()).audio_sample_rate() as f64);
        // Zero also covers a core that reports no rate yet: either way
        // the resampler must not divide by it.
        if rate > 0.0 {
            self.last_rate.store(rate.to_bits(), Ordering::Relaxed);
            rate
        } else {
            f64::from_bits(self.last_rate.load(Ordering::Relaxed))
        }
    }

    fn framerate_ratio(&self, fps_target: f64) -> f64 {
        let mut ratio = 1.0;
        if self
            .link
            .try_with(&mut |link| ratio = link.core_mut(self.player()).calculate_framerate_ratio(fps_target))
        {
            self.last_ratio.store(ratio.to_bits(), Ordering::Relaxed);
            ratio
        } else {
            f64::from_bits(self.last_ratio.load(Ordering::Relaxed))
        }
    }

    fn drain(&mut self, out: &mut [i16]) -> tango_match::Drained {
        let player = self.player();
        let mut drained = tango_match::Drained::default();
        if self.link.try_with(&mut |link| {
            let core = link.core_mut(player);
            let buffer = core.audio_buffer();
            let frames = (out.len() / 2).min(buffer.available());
            drained.written = buffer.read(out, frames);
            drained.queued = buffer.available();
        }) {
            self.last_queued.store(drained.queued, Ordering::Relaxed);
            drained
        } else {
            // Mid-tick, so unreachable this fill. Nothing was taken, and
            // the last level is the honest answer for what is sitting
            // there: only the sim's own additions since are missing, and
            // they land in the next fill's reading.
            tango_match::Drained {
                written: 0,
                queued: self.last_queued.load(Ordering::Relaxed),
            }
        }
    }
}

/// Expand mgba's native BGR555 to the RGBA8 the seam promises hosts.
fn to_rgba(src: &[u8]) -> Vec<u8> {
    let mut rgba = vec![0u8; src.len() * 2];
    mgba::gba::bgr555_to_rgba8(src, &mut rgba);
    rgba
}
