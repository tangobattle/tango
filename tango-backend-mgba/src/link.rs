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

use tango_match::telemetry::Telemetry;
use tango_match::{HostInput, Screen, ScreenLayout};

/// Bit mask of a joyflags value: the GBA keypad is 10 bits (A, B, Select,
/// Start, →, ←, ↑, ↓, R, L), occupying bits 0..=9. The top 6 bits are unused by
/// the hardware, so callers are free to repurpose them — e.g. the live core's r4
/// high bits, or the netplay wire's CONT/MARK entry tags.
pub const JOYFLAGS_MASK: u32 = 0x03ff;

pub const EXPECTED_FPS: f64 = 16777216.0 / 280896.0;

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
    /// Training's write-side hook and the control it honors, when this
    /// pair runs one (see [`tango_match::trainer`]). Driven each tick
    /// BEFORE the telemetry poll, so the pollers read post-write state.
    /// Deliberately untouched by [`restore`](tango_match::Link::restore):
    /// a trainer is lockstep-only by contract, so a pair carrying one
    /// never rewinds.
    trainer: Option<TrainerHook>,
}

/// The per-game trainer plus the shared control it reads — see
/// [`tango_match::trainer`].
type TrainerHook = (
    Box<dyn tango_match::trainer::Trainer<mgba::core::Core>>,
    std::sync::Arc<tango_match::trainer::TrainerControl>,
);

impl Link {
    /// Wrap an already-booted, already-primed pair. `telemetry` is the
    /// collector whose pollers read this pair's games, if the session
    /// runs one; `trainer` is training's write-side hook, if this pair
    /// honors one.
    pub fn new(
        pair: mgba_rollback::Link,
        telemetry: Option<Telemetry<mgba::core::Core>>,
        trainer: Option<TrainerHook>,
    ) -> Self {
        Link {
            inner: pair,
            live_tick: 0,
            telemetry,
            trainer,
        }
    }
}

impl tango_match::Link for Link {
    fn sanitize(&self, input: HostInput) -> HostInput {
        // The GBA has no touch screen and no X/Y, so only the 10
        // hardware bits survive.
        HostInput::keys(input.keys & JOYFLAGS_MASK)
    }

    fn tick(&mut self, inputs: [HostInput; 2]) {
        let keys = inputs.map(|input| input.keys & JOYFLAGS_MASK);
        self.inner.tick(&keys);
        self.live_tick += 1;
        if let Some((trainer, control)) = self.trainer.as_mut() {
            trainer.tick(self.inner.core_mut(0), 0, control);
            trainer.tick(self.inner.core_mut(1), 1, control);
        }
        if let Some(telemetry) = self.telemetry.as_mut() {
            let obs0 = telemetry.poll(0, self.inner.core_mut(0));
            let obs1 = telemetry.poll(1, self.inner.core_mut(1));
            telemetry.observe(obs0, obs1, self.live_tick);
        }
    }

    fn snapshot(
        &mut self,
        _recycled: Option<tango_match::Snapshot>,
    ) -> Result<tango_match::Snapshot, tango_match::Error> {
        // GBA states are small enough (~0.5 MB against the DS's ~6 MB)
        // that recycling buffers has never been worth the plumbing.
        let snap = self.inner.save().map_err(|e| crate::Error::from(e))?;
        Ok(Box::new(GbaSnapshot {
            snap,
            tick: self.live_tick,
        }))
    }

    fn restore(&mut self, snapshot: &tango_match::Snapshot) -> Result<(), tango_match::Error> {
        let snapshot = snapshot
            .downcast_ref::<GbaSnapshot>()
            .expect("an mgba link can only restore its own snapshots");
        self.inner.load(&snapshot.snap).map_err(|e| crate::Error::from(e))?;
        self.live_tick = snapshot.tick;
        if let Some(telemetry) = self.telemetry.as_mut() {
            // Everything observed past the restored tick is revoked;
            // the re-simulation re-reports it.
            telemetry.on_rewind(snapshot.tick);
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

    fn audio_sample_rate(&mut self) -> f64 {
        self.link.core(self.player).audio_sample_rate() as f64
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
