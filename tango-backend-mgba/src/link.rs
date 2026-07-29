//! The mgba link: [`tango_match::Link`] over a pair of emulated GBAs
//! on an emulated link cable.
//!
//! The sibling of `tango-backend-melonds`. Both answer the same
//! questions — how does a pair tick, snapshot, restore and draw — for
//! very different hardware, which is what lets one
//! [`Match`](tango_match::Match) drive either.
//!
//! Two things ride along that `mgba_rollback::Link` itself doesn't
//! carry:
//!
//! * **Audio revocation.** The cores' mixed-output rings are playback
//!   state, not machine state — a savestate doesn't cover them — so
//!   the wrapper counts what each tick kept and takes speculation back
//!   by hand when the engine rewinds (see
//!   [`revoke_audio`](tango_match::Link::revoke_audio)).
//! * **Telemetry.** The per-tick RAM pollers need the cores and the
//!   tick number, both of which live here; the shared collector gets
//!   driven from inside [`tick`](tango_match::Link::tick) and rewound
//!   from inside [`restore`](tango_match::Link::restore), so the
//!   engine above never learns what a game is.

use tango_match::{Drained, HostInput, Screen, ScreenLayout};

/// Bit mask of a joyflags value: the GBA keypad is 10 bits (A, B, Select,
/// Start, →, ←, ↑, ↓, R, L), occupying bits 0..=9. The top 6 bits are unused by
/// the hardware, so callers are free to repurpose them — e.g. the live core's r4
/// high bits, or the netplay wire's CONT/MARK entry tags.
pub const JOYFLAGS_MASK: u32 = 0x03ff;

/// The GBA's single screen.
const SCREEN: Screen = Screen {
    width: 240,
    height: 160,
};

/// The screens this console presents, for a factory's
/// [`screen_layout`](tango_match::MatchFactory::screen_layout).
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
    /// handle the factory installs on the match.
    telemetry: Option<crate::r#match::telemetry::Telemetry>,
    /// Per-core cumulative sample frames appended to the mixed-output
    /// audio ring AND kept (net of revocation drops and re-sim drains) —
    /// the coordinate system rollback revocation math runs in. Only
    /// meaningful for rings a consumer drains: an undrained ring pegs at
    /// capacity, its appends stop landing, and its counter stalls with
    /// them (harmless — nobody is listening to it).
    audio_produced: [u64; 2],
    /// Sample frames of re-simulated audio still to swallow during the
    /// current rollback catch-up, per core: the corrected regeneration
    /// of spans whose speculative version already played. It cannot be
    /// unplayed, so queuing the regeneration would replay audio the
    /// listener already heard — an echo. Swallowed oldest-first out of
    /// each catch-up tick's fresh production instead.
    audio_resim_drain: [u64; 2],
    /// Scratch for [`remove_span`].
    audio_scratch: Vec<i16>,
}

impl Link {
    /// Wrap an already-booted, already-primed pair. `telemetry` is the
    /// collector whose pollers read this pair's games, if the session
    /// runs one.
    pub fn new(pair: mgba_rollback::Link, telemetry: Option<crate::r#match::telemetry::Telemetry>) -> Self {
        Link {
            inner: pair,
            live_tick: 0,
            telemetry,
            audio_produced: [0; 2],
            audio_resim_drain: [0; 2],
            audio_scratch: Vec::new(),
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
        let mut before = [0u64; 2];
        for i in 0..2 {
            before[i] = self.inner.core_mut(i).audio_buffer().available() as u64;
        }
        self.inner.tick(&keys);
        for i in 0..2 {
            // Nothing consumes between the two reads (the host's audio
            // callback takes the engine's link mutex), so the delta is
            // exactly this tick's kept production.
            let buf = self.inner.core_mut(i).audio_buffer();
            let mut delta = (buf.available() as u64).saturating_sub(before[i]);
            if self.audio_resim_drain[i] > 0 && delta > 0 {
                // Catch-up regeneration of already-played audio: swallow
                // it oldest-first out of this tick's fresh span, so the
                // seam lands exactly where playback left off.
                let drain = self.audio_resim_drain[i].min(delta) as usize;
                let avail = buf.available();
                remove_span(buf, &mut self.audio_scratch, avail - delta as usize, drain);
                self.audio_resim_drain[i] -= drain as u64;
                delta -= drain as u64;
            }
            self.audio_produced[i] += delta;
        }
        self.live_tick += 1;
        if let Some(telemetry) = self.telemetry.as_mut() {
            let obs0 = telemetry.poll(0, self.inner.core_mut(0));
            let obs1 = telemetry.poll(1, self.inner.core_mut(1));
            telemetry.observe(obs0, obs1, self.live_tick);
        }
    }

    fn snapshot(&mut self, _recycled: Option<tango_match::Snapshot>) -> Result<tango_match::Snapshot, tango_match::Error> {
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

    fn audio_mark(&mut self) -> [u64; 2] {
        self.audio_produced
    }

    fn revoke_audio(&mut self, mark: [u64; 2]) {
        // The mixed-output audio rings are playback state, not machine
        // state: rewind them by exactly the revoked span. Everything
        // appended since the mark voices the speculation being revoked.
        // The part still queued is dropped from the write end — the
        // settled backlog beneath it is final audio a full clear would
        // skip over (the audible per-rollback crunch, and the
        // queue-level collapse behind the underruns that follow). The
        // part the host already played cannot be unplayed, so its
        // corrected regeneration is swallowed during the catch-up
        // instead of queuing as an echo. By determinism the catch-up
        // regenerates the revoked span sample-for-sample (barring an
        // input-driven SOUNDBIAS rate change inside the window — rare,
        // and off by at most that window), so playback resumes exactly
        // where it left off.
        for i in 0..2 {
            let revoked = self.audio_produced[i] - mark[i];
            let buf = self.inner.core_mut(i).audio_buffer();
            let queued = buf.available() as u64;
            let dropped = revoked.min(queued);
            remove_span(
                buf,
                &mut self.audio_scratch,
                (queued - dropped) as usize,
                dropped as usize,
            );
            self.audio_resim_drain[i] = revoked - dropped;
            self.audio_produced[i] = mark[i];
        }
    }

    fn frame(&mut self, player: usize) -> Option<Vec<u8>> {
        // Cores render BGR555; expanding here is what keeps the console's
        // native pixel format from leaking out to hosts.
        let native = self.inner.video_buffer(player)?;
        let mut rgba = vec![0u8; native.len() * 2];
        mgba::gba::bgr555_to_rgba8(native, &mut rgba);
        Some(rgba)
    }

    fn set_render(&mut self, player: usize, on: bool) {
        self.inner.set_frameskip(player, if on { 0 } else { i32::MAX });
    }

    fn audio_sample_rate(&mut self, player: usize) -> f64 {
        self.inner.core(player).audio_sample_rate() as f64
    }

    fn audio_framerate_ratio(&mut self, player: usize, fps_target: f64) -> f64 {
        self.inner.core(player).calculate_framerate_ratio(fps_target)
    }

    fn drain_audio(&mut self, player: usize, out: &mut [i16]) -> Drained {
        let buf = self.inner.core_mut(player).audio_buffer();
        // `out` holds interleaved samples, so it fits half as many
        // frames. Reading consumes, which is what stops a session
        // replaying audio it already played after a rollback; what
        // stays here stays revocable.
        let frames = (out.len() / 2).min(buf.available());
        let written = buf.read(out, frames);
        Drained {
            written,
            queued: buf.available(),
        }
    }
}

/// Remove `frames` sample frames starting `start` frames from the OLDEST
/// end of a core's mixed-output audio ring, closing the gap — the
/// surgical complement of [`read`](mgba::audio::AudioBuffer::read),
/// which can only consume from the oldest end: read everything out and
/// write both remnants back.
fn remove_span(buf: &mut mgba::audio::AudioBuffer, scratch: &mut Vec<i16>, start: usize, frames: usize) {
    let avail = buf.available();
    let frames = frames.min(avail.saturating_sub(start));
    if frames == 0 {
        return;
    }
    let channels = buf.channels() as usize;
    scratch.resize(avail * channels, 0);
    buf.read(scratch, avail);
    buf.write(scratch, start);
    buf.write(&scratch[(start + frames) * channels..], avail - start - frames);
}

#[cfg(test)]
mod tests {
    /// The shared rollback loop accepts this link — the point of the
    /// seam. A GBA match and a DS match are the same
    /// [`tango_match::Match`], and neither engine reimplements the
    /// loop.
    #[test]
    fn the_shared_engine_accepts_this_link() {
        let _: fn(super::Link, usize, u32) -> Result<tango_match::Match, tango_match::Error> =
            tango_match::Match::new::<super::Link>;
    }
}
