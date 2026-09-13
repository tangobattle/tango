//! The mgba link: [`tango_match::Link`] over a pair of emulated GBAs
//! on an emulated link cable.
//!
//! The sibling of `tango-backend-melonds`. Both answer the same
//! questions — how does a pair tick, snapshot, restore and draw — for
//! very different hardware, which is what lets one
//! [`Match`](tango_match::Match) drive either.
//!
//! One thing rides along that `mgba_rollback::Link` itself doesn't
//! carry: **telemetry**. The per-tick RAM pollers need the cores and
//! the tick number, both of which live here; the shared collector gets
//! driven from inside [`tick`](tango_match::Link::tick) and rewound
//! from inside [`restore`](tango_match::Link::restore), so the engine
//! above never learns what a game is.
//!
//! Audio revocation used to ride along too — the cores' mixed-output
//! rings are playback state, not machine state, so a rollback had to
//! take back what the speculation voiced in them. It doesn't any more:
//! the session empties both cores into its own ring
//! ([`audio`](tango_match::audio)) on its way past every tick, so by
//! the time a rewind lands there is nothing in a core to take back and
//! the ring answers for it instead.

use num_rational::Ratio;
use tango_match::telemetry::stream;
use tango_match::telemetry::Telemetry;
use tango_match::{HostInput, Screen, ScreenLayout};

#[cfg(test)]
mod gamemode_tests;

/// Bit mask of a joyflags value: the GBA keypad is 10 bits (A, B, Select,
/// Start, →, ←, ↑, ↓, R, L), occupying bits 0..=9. The top 6 bits are unused by
/// the hardware, so callers are free to repurpose them — e.g. the live core's r4
/// high bits, or the netplay wire's CONT/MARK entry tags.
pub const JOYFLAGS_MASK: u32 = 0x03ff;

/// Native ticks per second: one video frame spans 280,896 GBA clock cycles.
pub const TPS: Ratio<u32> = Ratio::new_raw(16_777_216, 280_896);

/// The GBA's single screen.
const SCREEN: Screen = Screen {
    width: 240,
    height: 160,
};

/// The screens this console presents, for a backend's
/// [`screen_layout`](tango_match::Backend::screen_layout).
pub fn screen_layout() -> ScreenLayout {
    ScreenLayout::new([SCREEN])
}

/// A whole-link capture stamped with the tick it was taken at, so a
/// restore can rewind the wrapper's own clock (and its telemetry) to
/// where the capture was made.
struct GbaSnapshot {
    snap: mgba_rollback::Snapshot,
    tick: u32,
    records: Option<stream::Snapshot>,
    gamemodes: Option<crate::gamemode::Snapshot>,
    telemetry: Option<tango_match::telemetry::Snapshot>,
}

/// Engine state for deterministic-boot diagnostics; scripted and lifecycle
/// checkpoints remain opaque and are restored through the link interface.
pub fn emulator_snapshot(snapshot: &tango_match::Snapshot) -> Option<&mgba_rollback::Snapshot> {
    snapshot.downcast_ref::<GbaSnapshot>().map(|snapshot| &snapshot.snap)
}

struct Records {
    collector: stream::Collector,
    initial: stream::Snapshot,
}

struct Memory<'a>(&'a mgba::core::Core);
impl stream::Memory for Memory<'_> {
    fn read(&mut self, space: &str, address: u32, output: &mut [u8]) -> Result<(), stream::Error> {
        if space != "main" {
            return Err("unknown GBA memory space".into());
        }
        self.0.raw_read_range(address, -1, output);
        Ok(())
    }
}

/// The linked pair: two GBAs on an emulated link cable, as the seam's
/// [`Link`](tango_match::Link).
pub struct Link {
    inner: mgba_rollback::Link,
    /// Ticks simulated since the session started. Priming isn't
    /// counted: the wrapper is built over an already-primed pair, so
    /// tick 1 is the session's first simulated tick — the numbering
    /// telemetry and the confirmed-input record agree on.
    live_tick: u32,
    /// The RAM-poll collector, when this pair runs one. Polled after
    /// every tick, rewound on every restore; the store it feeds is the
    /// handle the backend installs on the match.
    telemetry: Option<Telemetry<mgba::core::Core>>,
    records: Option<Records>,
    pub(crate) gamemodes: Option<crate::gamemode::Pair>,
}

impl Link {
    /// Wrap an already-booted, already-primed pair. `telemetry` is the
    /// collector whose pollers read this pair's games, if the session
    /// runs one.
    pub fn new(pair: mgba_rollback::Link, telemetry: Option<Telemetry<mgba::core::Core>>) -> Self {
        Link {
            inner: pair,
            live_tick: 0,
            telemetry,
            records: None,
            gamemodes: None,
        }
    }

    /// Install samplers before the first tick. Their state becomes part of
    /// every checkpoint, including captures shared between replay workers.
    pub fn set_records(&mut self, collector: stream::Collector) {
        assert_eq!(self.live_tick, 0);
        let initial = collector.snapshot();
        self.records = Some(Records { collector, initial });
    }
}

impl tango_match::Link for Link {
    fn sanitize(&self, input: HostInput) -> HostInput {
        // The GBA has no touch screen and no X/Y, so only the 10
        // hardware bits survive.
        HostInput::keys(input.keys & JOYFLAGS_MASK)
    }

    fn tick(&mut self, inputs: [HostInput; 2]) -> Result<(), tango_match::Error> {
        if let Some(gamemodes) = &self.gamemodes {
            gamemodes.begin_tick()?;
        }
        let keys = inputs.map(|input| input.keys & JOYFLAGS_MASK);
        self.inner.try_tick(&keys).map_err(crate::Error::from)?;
        if let Some(gamemodes) = &self.gamemodes {
            gamemodes.end_tick()?;
        }
        self.live_tick += 1;
        if let Some(records) = &mut self.records {
            let round = self.telemetry.as_ref().map_or(0, Telemetry::round);
            for player in 0..2 {
                records
                    .collector
                    .poll(player, &mut Memory(self.inner.core_mut(player)), self.live_tick, round);
            }
        }
        if let Some(telemetry) = self.telemetry.as_mut() {
            let obs0 = telemetry.poll(0, self.inner.core_mut(0));
            let obs1 = telemetry.poll(1, self.inner.core_mut(1));
            telemetry.observe(obs0, obs1, self.live_tick);
        }
        Ok(())
    }

    fn snapshot(
        &mut self,
        _recycled: Option<tango_match::Snapshot>,
    ) -> Result<tango_match::Snapshot, tango_match::Error> {
        // GBA states are small enough (~0.5 MB against the DS's ~6 MB)
        // that recycling buffers has never been worth the plumbing.
        let gamemodes = self.gamemodes.as_ref().map(|m| m.snapshot()).transpose()?;
        let snap = self.inner.save().map_err(|e| crate::Error::from(e))?;
        Ok(Box::new(GbaSnapshot {
            snap,
            tick: self.live_tick,
            records: self.records.as_ref().map(|records| records.collector.snapshot()),
            gamemodes,
            telemetry: self.telemetry.as_mut().map(Telemetry::snapshot),
        }))
    }

    fn restore(&mut self, snapshot: &tango_match::Snapshot) -> Result<(), tango_match::Error> {
        let snapshot = snapshot
            .downcast_ref::<GbaSnapshot>()
            .expect("an mgba link can only restore its own snapshots");
        match (&self.gamemodes, &snapshot.gamemodes) {
            (Some(program), Some(state)) => program.validate_snapshot(state)?,
            (None, None) => {}
            _ => {
                return Err(tango_match::Error::Unsupported(
                    "capture has a different gamemode configuration",
                ))
            }
        }
        let record_state = match (&self.records, &snapshot.records) {
            (Some(_), Some(state)) => Some(state),
            (Some(records), None) if snapshot.tick == 0 => Some(&records.initial),
            (Some(_), None) => return Err(tango_match::Error::Unsupported("capture has no telemetry state")),
            (None, _) => None,
        };
        if let (Some(telemetry), Some(state)) = (&self.telemetry, &snapshot.telemetry) {
            telemetry.validate_snapshot(state)?;
        }
        if let (Some(records), Some(state)) = (&self.records, record_state) {
            records.collector.validate_snapshot(state)?;
        }
        self.inner.load(&snapshot.snap).map_err(|e| crate::Error::from(e))?;
        if let (Some(program), Some(state)) = (&self.gamemodes, &snapshot.gamemodes) {
            program.restore(state)?;
        }
        self.live_tick = snapshot.tick;
        if let Some(records) = &mut self.records {
            let state = match &snapshot.records {
                Some(state) => state,
                None if snapshot.tick == 0 => &records.initial,
                None => return Err(tango_match::Error::Unsupported("capture has no telemetry state")),
            };
            records.collector.restore(state, snapshot.tick)?;
        }
        if let Some(telemetry) = self.telemetry.as_mut() {
            // Everything observed past the restored tick is revoked;
            // the re-simulation re-reports it.
            if let Some(state) = &snapshot.telemetry {
                telemetry.restore(state)?;
            } else {
                telemetry.on_rewind(snapshot.tick);
            }
        }
        Ok(())
    }

    fn side(&mut self, player: usize) -> Box<dyn tango_match::Side + '_> {
        Box::new(GbaSide {
            link: &mut self.inner,
            player,
        })
    }
}

/// One GBA of a boot — the per-side surface over the raw
/// `mgba_rollback` pair. The linked pair above, the solo console, and
/// replay playback's audio all keep their consoles in that same inner
/// type, so this exists once for all three.
pub(crate) struct GbaSide<'a> {
    pub(crate) link: &'a mut mgba_rollback::Link,
    pub(crate) player: usize,
}

impl tango_match::Side for GbaSide<'_> {
    fn frame(&mut self) -> Option<Vec<u8>> {
        // Cores render BGR555; expanding here is what keeps the console's
        // native pixel format from leaking out to hosts.
        self.link.video_buffer(self.player).map(to_rgba)
    }

    fn set_render(&mut self, on: bool) {
        self.link.set_frameskip(self.player, if on { 0 } else { i32::MAX });
    }

    fn export_save(&mut self) -> Option<Vec<u8>> {
        self.link.export_save(self.player)
    }

    fn audio_sample_rate(&mut self) -> Ratio<u32> {
        Ratio::from_integer(self.link.core(self.player).audio_sample_rate())
    }

    fn drain_audio(&mut self, out: &mut [i16]) -> usize {
        let buf = self.link.core_mut(self.player).audio_buffer();
        // `out` holds interleaved samples, so it fits half as many
        // frames. Reading consumes: a session empties this ring every
        // tick, so what a core holds is never more than the tick it just
        // finished.
        let available = buf.available();
        buf.read(out, (out.len() / 2).min(available));
        available
    }
}

/// Expand mgba's native BGR555 to the RGBA8 the seam promises hosts.
pub(crate) fn to_rgba(src: &[u8]) -> Vec<u8> {
    let mut rgba = vec![0u8; src.len() * 2];
    mgba::gba::bgr555_to_rgba8(src, &mut rgba);
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_match::Link as _;

    #[derive(Clone)]
    struct Sampler {
        source: stream::Source,
        total: u32,
    }
    impl stream::Sampler for Sampler {
        fn source(&self) -> &stream::Source {
            &self.source
        }
        fn sample(
            &mut self,
            memory: &mut dyn stream::Memory,
            context: stream::Context,
        ) -> Result<stream::Frame, stream::Error> {
            let mut value = [0];
            memory.read("main", 0x0203_ffff, &mut value)?;
            assert!(memory.read("unknown", 0, &mut value).is_err());
            self.total += u32::from(value[0]);
            Ok(stream::Frame {
                values: [("total".into(), stream::Value::Number(self.total as f64))].into(),
                events: vec![stream::Event {
                    name: "sample".into(),
                    fields: [("player".into(), stream::Value::Number(context.player as f64))].into(),
                }],
            })
        }
    }
    struct Factory(u8);
    impl stream::Factory for Factory {
        fn open(&self, _: &[u8], _: u16) -> Result<Option<Box<dyn stream::Sampler>>, stream::Error> {
            Ok(Some(Box::new(Sampler {
                source: stream::Source {
                    digest: [self.0; 32],
                    name: "test".into(),
                },
                total: 0,
            })))
        }
    }
    fn link(id: Option<u8>) -> (Link, Option<stream::Handle>) {
        let rom = mgba_rollback::testrom::build_idle();
        let pair = mgba_rollback::Link::new(vec![rom.clone(), rom.clone()]).unwrap();
        let mut link = Link::new(pair, None);
        let handle = id.map(|id| {
            let (collector, handle) = stream::Collector::new(&Factory(id), [&rom, &rom]);
            link.set_records(collector);
            handle
        });
        (link, handle)
    }
    fn step(link: &mut Link, value: u8) {
        for player in 0..2 {
            link.inner
                .core_mut(player)
                .raw_write_8(0x0203_ffff, -1, value + player as u8);
        }
        link.tick([HostInput::default(); 2]).unwrap();
    }

    #[test]
    fn named_telemetry_follows_emulator_checkpoints_and_shared_replay_captures() {
        let (mut first, handle) = link(Some(1));
        let handle = handle.unwrap();
        step(&mut first, 3);
        let capture = first.snapshot(None).unwrap();
        assert_eq!(handle.lock().unwrap().take_through(1).records.len(), 2);
        step(&mut first, 5);
        let expected = handle.lock().unwrap().take_through(2).records;
        assert_eq!(
            expected[0].frame.as_ref().unwrap().values["total"],
            stream::Value::Number(8.0)
        );
        first.restore(&capture).unwrap();
        step(&mut first, 5);
        let replayed = handle.lock().unwrap().take_through(2);
        assert_eq!(replayed.rewind_to, Some(1));
        assert_eq!(replayed.records, expected);

        let (mut other, other_handle) = link(Some(1));
        other.restore(&capture).unwrap();
        step(&mut other, 5);
        assert_eq!(other_handle.unwrap().lock().unwrap().take_through(2).records, expected);

        let (mut different, _) = link(Some(2));
        assert!(different.restore(&capture).is_err());
        assert_eq!(different.live_tick, 0);

        let (mut unobserved, _) = link(None);
        let initial = unobserved.snapshot(None).unwrap();
        first.restore(&initial).unwrap();
        step(&mut first, 5);
        let reset = handle.lock().unwrap().take_through(1);
        assert_eq!(reset.rewind_to, Some(0));
        assert_eq!(
            reset.records[0].frame.as_ref().unwrap().values["total"],
            stream::Value::Number(5.0)
        );
        step(&mut unobserved, 0);
        let missing = unobserved.snapshot(None).unwrap();
        assert!(first.restore(&missing).is_err());
    }

    struct Boot;
    impl tango_match::ReplayBoot for Boot {
        fn boot(
            &self,
            observe: bool,
            _: &std::sync::atomic::AtomicBool,
        ) -> Result<tango_match::BootedReplay, tango_match::Error> {
            self.boot_unprimed(observe)
        }
        fn boot_unprimed(&self, observe: bool) -> Result<tango_match::BootedReplay, tango_match::Error> {
            let (mut link, records) = link(observe.then_some(1));
            for player in 0..2 {
                link.inner
                    .core_mut(player)
                    .raw_write_8(0x0203_ffff, -1, 3 + player as u8);
            }
            Ok(tango_match::BootedReplay {
                link: Box::new(link),
                records,
                telemetry: None,
            })
        }
    }

    #[test]
    fn replay_analysis_keeps_named_records_when_the_viewer_seeks() {
        use std::sync::{Arc, Mutex};
        let config = tango_match::ReplayConfig {
            record_factory: None,
            roms: Default::default(),
            saves: Default::default(),
            inputs: Arc::new(vec![[HostInput::default(); 2]; 12]),
            rng_seed: [0; 16],
            rtc: std::time::UNIX_EPOCH,
            match_type: 0,
            local_player: 0,
            peer_rom: Some(tango_match::PeerRom {
                code: [0; 4],
                revision: 0,
            }),
            want_stats: true,
            disable_bgm: false,
        };
        let set = tango_match::ReplaySet::new(&config, Boot);
        let mut playback = set.playback().unwrap();
        let mut pass = set.stats_reusing_playback().unwrap();
        let timeline = Arc::new(Mutex::new(stream::Timeline::new([None, None])));
        pass.collect_records_into(timeline.clone());
        assert!(!pass.step(20).unwrap());
        let expected = timeline.lock().unwrap().events().to_vec();
        assert_eq!(expected.len(), 24);
        assert_eq!(timeline.lock().unwrap().series().len(), 2);
        while playback.step().unwrap() {}
        let ctrl = tango_match::seek::SeekController::new();
        for tick in [3, 10, 1, 12] {
            ctrl.request(tick, false);
            playback
                .seek_step(&ctrl, 20, &mut |_| {}, &mut |_| {}, &mut || {})
                .unwrap();
            assert_eq!(playback.cursor(), tick);
        }
        assert_eq!(timeline.lock().unwrap().events(), expected);
        assert!(timeline.lock().unwrap().errors().is_empty());
    }

    /// The shared rollback loop accepts this link — the point of the
    /// seam. A GBA match and a DS match are the same
    /// [`tango_match::Match`], and neither engine reimplements the
    /// loop.
    #[test]
    fn the_shared_engine_accepts_this_link() {
        let _: fn(
            super::Link,
            usize,
            u32,
            Option<tango_match::AudioIn>,
        ) -> Result<tango_match::Match, tango_match::Error> = tango_match::Match::new::<super::Link>;
    }
}
