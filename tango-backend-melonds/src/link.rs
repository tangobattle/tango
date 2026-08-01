//! The melonDS link: [`tango_match::Link`] over a pair of emulated
//! DSes on emulated local wireless.
//!
//! The sibling of `tango-backend-mgba`'s link. Both answer the same
//! questions — how does a pair tick, snapshot, restore and draw — for
//! very different hardware, which is what lets one
//! [`Match`](tango_match::Match) drive either.

use tango_match::telemetry::Telemetry;
use tango_match::{HostInput, Screen, ScreenLayout};

/// The rate the SPU hands samples out at.
///
/// Not the DS's own 32823.6328 Hz: melonDS resamples internally, and
/// what it resamples *to* is `NDSArgs::OutputSampleRate`, which the shim
/// leaves at its 48 kHz default. Claiming the console's native rate
/// instead stretches playback by 48000/32823.6 — about six semitones
/// flat — and drains the source queue half again as fast as it fills,
/// which underruns into a crackle on top of the wrong pitch.
pub const SAMPLE_RATE: f64 = 48_000.0;

/// The DS's video framerate, which is also the rate audio production
/// scales against when a host paces the simulation faster or slower.
pub const EXPECTED_FPS: f64 = 16756991.0 / 280095.0;

/// The DS's pad: the GBA's ten buttons plus X and Y, which is every
/// bit [`keys`](tango_match::keys) names.
pub const KEYS_MASK: u32 = tango_match::keys::MASK;

/// The DS presents two identically-sized screens. Listed in the order
/// [`Link::frame`](tango_match::Link::frame) lays them out, which is
/// the console's top screen then its bottom (touch) one — left to
/// right in the composed frame rather than the console's own physical
/// stack.
const SCREENS: [Screen; 2] = [
    Screen {
        width: 256,
        height: 192,
    },
    Screen {
        width: 256,
        height: 192,
    },
];

/// One of the console's physical screens, so a [`Screens`] selection
/// names what it picks instead of indexing it. Discriminants are
/// [`SCREENS`]' order, which is what makes the mapping a cast.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsScreen {
    Upper = 0,
    Touch = 1,
}

/// The screens a session composes, in the order its frames lay them
/// out: the console's whole display, or whatever subset of it the
/// game actually uses.
///
/// A subset because a cart can spend a whole mode on one screen —
/// EXE OSS's netbattle never leaves the upper one — and a pane
/// carrying the other is half dead menu. Deciding it here rather than
/// cropping downstream keeps one answer: the composed frame, the
/// [`ScreenLayout`] a session sizes its framebuffer from, and the
/// exports all come off the same selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Screens(pub &'static [DsScreen]);

impl Screens {
    /// The whole console, upper screen first — what a DS shows when
    /// nothing says otherwise.
    pub const BOTH: Screens = Screens(&[DsScreen::Upper, DsScreen::Touch]);
    /// The upper screen alone, for a mode that never uses the stylus.
    pub const UPPER: Screens = Screens(&[DsScreen::Upper]);
    /// The touch screen alone.
    pub const TOUCH: Screens = Screens(&[DsScreen::Touch]);

    /// This selection as a backend's
    /// [`screen_layout`](tango_match::Backend::screen_layout) — sizes
    /// in composed order, with the stylus target marked wherever the
    /// selection put it.
    pub fn layout(self) -> ScreenLayout {
        let layout = ScreenLayout::new(self.0.iter().map(|&s| SCREENS[s as usize]));
        match self.0.iter().position(|&s| s == DsScreen::Touch) {
            Some(i) => layout.with_touch(i),
            None => layout,
        }
    }
}

/// A whole-link capture stamped with the tick it was taken at, so a
/// restore can rewind the wrapper's own clock (and its telemetry) to
/// where the capture was made.
struct DsSnapshot {
    snap: melonds_rollback::Snapshot,
    tick: u32,
}

/// The raw whole-link bytes inside a seam snapshot this engine
/// produced — consoles, in-flight air frames, clock bounds. For the
/// determinism drills (bn5ds's `landing_probe`), which attribute a
/// divergence by diffing them; not part of the engine's surface.
#[doc(hidden)]
pub fn snapshot_bytes(snap: &tango_match::Snapshot) -> Option<Vec<u8>> {
    snap.downcast_ref::<DsSnapshot>().map(|s| s.snap.to_bytes())
}

/// The linked pair: two DSes on emulated local wireless, as the seam's
/// [`Link`](tango_match::Link).
pub struct Link {
    inner: melonds_rollback::Link,
    /// Ticks simulated since the session started. Priming isn't
    /// counted: [`set_telemetry`](Link::set_telemetry) zeroes the clock
    /// when it arms, so tick 1 is the session's first simulated tick —
    /// the numbering telemetry and the confirmed-input record agree on.
    live_tick: u32,
    /// The RAM-poll collector, when this pair runs one. Polled after
    /// every tick, rewound on every restore; the store it feeds is the
    /// handle the backend installs on the match. Lifecycle rides the
    /// polls on this engine: console 0's poller reads where the match
    /// stands and reports the transitions into the collector's sink.
    telemetry: Option<Telemetry<crate::Nds>>,
    /// The screens this pair's frames carry. Both until the backend
    /// says otherwise ([`set_screens`](Link::set_screens)), which is
    /// what a probe harness booting a pair of its own wants.
    screens: Screens,
}

impl Link {
    /// Boot a pair. Both consoles run the same cart — a DS link is one
    /// game, two consoles — so the pair takes one image; the saves
    /// still differ, since each player brings their own. `rtc` is the
    /// negotiated match clock, pinned into both consoles so both peers
    /// reach the same state from the same inputs.
    pub fn new(rom: &[u8], saves: [Option<&[u8]>; 2], rtc: std::time::SystemTime) -> Result<Self, melonds::Error> {
        Ok(Link {
            inner: melonds_rollback::Link::new(rom, saves, rtc_parts(rtc))?,
            live_tick: 0,
            telemetry: None,
            screens: Screens::BOTH,
        })
    }

    /// Compose only these screens from here on. The backend sets it
    /// from the game's own selection before the session starts, so
    /// every frame this pair publishes matches the
    /// [`ScreenLayout`](tango_match::ScreenLayout) the host sized its
    /// framebuffer from. Priming runs dark, so a walk never sees the
    /// difference.
    pub fn set_screens(&mut self, screens: Screens) {
        self.screens = screens;
    }

    /// Arm the telemetry collector and zero the tick clock. Called
    /// between priming and the session, so the boot's ticks aren't
    /// counted and the boot's own screens predate the phase watch.
    pub fn set_telemetry(&mut self, telemetry: Telemetry<crate::Nds>) {
        self.live_tick = 0;
        self.telemetry = Some(telemetry);
    }

    /// Zero the tick clock without arming telemetry — what
    /// [`set_telemetry`](Link::set_telemetry) does for an observed
    /// pair, for one that runs without a collector. The walk drives
    /// this pair through the seam's own tick, so without this an
    /// unobserved pair's captures would carry boot-inflated ticks — and
    /// a telemetry-armed pair landing on one (the stats pass reusing
    /// the display pair's primed state) would stamp its observations
    /// off the session's numbering.
    pub(crate) fn zero_clock(&mut self) {
        self.live_tick = 0;
    }

    /// One console of the pair. A game crate needs this to reach past
    /// the link when priming: execution traps are installed per
    /// console.
    pub fn console(&mut self, player: usize) -> &mut crate::Nds {
        self.inner.console(player)
    }

    /// Whether the two consoles' wireless has associated — the probe
    /// harnesses' readout for whether a walk actually reached a link.
    pub fn connected(&self) -> bool {
        self.inner.connected()
    }
}

impl tango_match::Link for Link {
    fn sanitize(&self, input: HostInput) -> HostInput {
        sanitize(input)
    }

    fn tick(&mut self, inputs: [HostInput; 2]) {
        self.inner.tick(inputs.map(input_of));
        self.live_tick += 1;
        if let Some(telemetry) = self.telemetry.as_mut() {
            let obs0 = telemetry.poll(0, self.inner.console(0));
            let obs1 = telemetry.poll(1, self.inner.console(1));
            telemetry.observe(obs0, obs1, self.live_tick);
        }
    }

    fn snapshot(
        &mut self,
        recycled: Option<tango_match::Snapshot>,
    ) -> Result<tango_match::Snapshot, tango_match::Error> {
        let recycled = recycled.and_then(|s| s.downcast::<DsSnapshot>().ok().map(|s| s.snap));
        let snap = self
            .inner
            .snapshot_into(recycled)
            .map_err(|e| tango_match::Error::Backend(Box::new(e)))?;
        Ok(Box::new(DsSnapshot {
            snap,
            tick: self.live_tick,
        }))
    }

    fn restore(&mut self, snapshot: &tango_match::Snapshot) -> Result<(), tango_match::Error> {
        let snapshot = snapshot
            .downcast_ref::<DsSnapshot>()
            .expect("a melonDS link can only restore its own snapshots");
        self.inner
            .restore(&snapshot.snap)
            .map_err(|e| tango_match::Error::Backend(Box::new(e)))?;
        self.live_tick = snapshot.tick;
        if let Some(telemetry) = self.telemetry.as_mut() {
            // Everything observed past the restored tick is revoked;
            // the re-simulation re-reports it.
            telemetry.on_rewind(snapshot.tick);
        }
        Ok(())
    }

    fn audio_mark(&mut self) -> [u64; 2] {
        self.inner.audio_produced()
    }

    fn revoke_audio(&mut self, mark: [u64; 2]) {
        self.inner.revoke_audio_to(mark)
    }

    fn side(&mut self, player: usize) -> Box<dyn tango_match::Side + '_> {
        Box::new(DsSide(self.inner.side(player), self.screens))
    }
}

/// One DS of a boot — the seam's [`Side`](tango_match::Side) over the
/// per-console view `melonds_rollback` shares between its pair and its
/// solo boot, so this exists once for both. Carries the screens its
/// boot composes, since that differs by mode and not by console.
pub(crate) struct DsSide<'a>(pub(crate) melonds_rollback::Side<'a>, pub(crate) Screens);

impl tango_match::Side for DsSide<'_> {
    fn frame(&mut self) -> Option<Vec<u8>> {
        let (top, bottom) = self.0.console().framebuffers()?;
        Some(compose_frame(top, bottom, self.1))
    }

    fn set_render(&mut self, on: bool) {
        self.0.console().set_render(on);
    }

    fn export_save(&mut self) -> Option<Vec<u8>> {
        Some(self.0.console().save_memory())
    }

    fn audio_sample_rate(&mut self) -> f64 {
        SAMPLE_RATE
    }

    /// Taken from the boot rather than straight off the SPU. The boot
    /// empties the console's SPU every tick into a buffer of its own,
    /// because the SPU's ring cannot serve as one: a savestate does not
    /// cover it, so a rollback cannot take back what it speculated
    /// there, and at ~43 ms it overflows within a couple of frames of a
    /// re-simulation appending a span twice — destroying its own oldest
    /// audio to make room. What leaves here is already revocable and
    /// already deduplicated.
    fn drain_audio(&mut self, out: &mut [i16]) -> usize {
        let (written, queued) = self.0.take_audio(out);
        written + queued
    }
}

/// Compose `screens` into one RGBA8 frame, in the selection's own
/// order — which is [`Screens::layout`]'s, so the frame and the layout
/// describing it never disagree.
///
/// Side by side, so a row of the composite is a row of each selected
/// screen in turn. Stacked would be the cheaper concatenation — a
/// vertical stack is free when the widths match — but a 256x384 pane
/// wastes most of the width of any display it is drawn into.
fn compose_frame(top: &[u32], bottom: &[u32], screens: Screens) -> Vec<u8> {
    let sources = [top, bottom];
    let mut rgba = Vec::with_capacity(screens.layout().buffer_len());
    let (width, height) = (SCREENS[0].width as usize, SCREENS[0].height as usize);
    for row in 0..height {
        for &screen in screens.0 {
            for &pixel in &sources[screen as usize][row * width..(row + 1) * width] {
                // The core hands out BGRA words; hosts want RGBA bytes.
                let [b, g, r, _] = pixel.to_le_bytes();
                rgba.extend_from_slice(&[r, g, b, 0xff]);
            }
        }
    }
    rgba
}

/// Reduce the seam's input word to what this console could produce.
///
/// The DS pad is the GBA's plus X and Y, and Tango's own bit order
/// already matches the console's for the buttons both share, so the pad
/// half is a mask rather than a remap. The touch position clamps into
/// the bottom screen rather than trusting the host's mapping
/// arithmetic: an out-of-range sample is simulation state here, and
/// both peers must derive the same one.
fn sanitize(input: HostInput) -> HostInput {
    HostInput {
        keys: input.keys & KEYS_MASK,
        touch: input
            .touch
            .map(|(x, y)| (x.min(SCREENS[1].width as u16 - 1), y.min(SCREENS[1].height as u16 - 1))),
    }
}

/// The console's own input word for one sanitized host input.
pub(crate) fn input_of(input: HostInput) -> crate::Input {
    let input = sanitize(input);
    crate::Input {
        keys: input.keys,
        touch: input.touch,
    }
}

/// Split an instant into the fields a cart RTC takes. Both peers pass
/// the same one, so both consoles agree without a date library.
pub(crate) fn rtc_parts(rtc: std::time::SystemTime) -> (i32, i32, i32, i32, i32, i32) {
    let secs = rtc.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    // Civil-from-days, so this needs no date library and stays
    // identical on every platform.
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year as i32,
        month as i32,
        day as i32,
        (rem / 3_600) as i32,
        (rem % 3_600 / 60) as i32,
        (rem % 60) as i32,
    )
}

#[cfg(test)]
mod tests {
    /// The shared rollback loop accepts this link — the point of the
    /// seam. A DS match and a GBA match are the same
    /// [`tango_match::Match`], and neither engine reimplements the
    /// loop.
    #[test]
    fn the_shared_engine_accepts_this_link() {
        let _: fn(super::Link, usize, u32) -> Result<tango_match::Match, tango_match::Error> =
            tango_match::Match::new::<super::Link>;
    }

    /// A red top screen and a blue bottom one, as the core hands them
    /// out: BGRA words, so red is `0x00ff0000`.
    fn framebuffers() -> (Vec<u32>, Vec<u32>) {
        let px = (super::SCREENS[0].width * super::SCREENS[0].height) as usize;
        (vec![0x00ff_0000; px], vec![0x0000_00ff; px])
    }

    /// A selection's composed frame is exactly the size its layout
    /// claims. The session sizes its framebuffer from the layout and
    /// *silently drops* a frame of any other size, so a disagreement
    /// here is a permanently black pane rather than a crash.
    #[test]
    fn a_composed_frame_is_the_size_its_layout_claims() {
        let (top, bottom) = framebuffers();
        for screens in [super::Screens::BOTH, super::Screens::UPPER, super::Screens::TOUCH] {
            assert_eq!(
                super::compose_frame(&top, &bottom, screens).len(),
                screens.layout().buffer_len(),
                "{screens:?}"
            );
        }
    }

    /// A selection composes the screens it named, in the order it named
    /// them — including one that leaves the upper screen out, which is
    /// the case no two-variant "both or the top one" knob could state.
    #[test]
    fn a_selection_composes_the_screens_it_named() {
        let (top, bottom) = framebuffers();
        const RED: [u8; 4] = [0xff, 0, 0, 0xff];
        const BLUE: [u8; 4] = [0, 0, 0xff, 0xff];
        let first_pixel = |screens| super::compose_frame(&top, &bottom, screens)[..4].to_vec();

        assert_eq!(first_pixel(super::Screens::UPPER), RED);
        assert_eq!(first_pixel(super::Screens::TOUCH), BLUE);
        // Both starts on the upper screen and crosses to the touch one
        // partway along the first row, since the pair composes side by
        // side rather than stacked.
        let both = super::compose_frame(&top, &bottom, super::Screens::BOTH);
        let seam = super::SCREENS[0].width as usize * 4;
        assert_eq!(both[..4], RED);
        assert_eq!(both[seam..seam + 4], BLUE);
    }

    /// The stylus target travels with the selection: named wherever the
    /// composition put it, and absent when the composition left it out.
    /// A host places its stylus area from this, having no other way to
    /// find the screen.
    #[test]
    fn a_layout_names_where_its_touch_screen_landed() {
        assert_eq!(super::Screens::BOTH.layout().touch, Some(1));
        assert_eq!(super::Screens::TOUCH.layout().touch, Some(0));
        assert_eq!(super::Screens::UPPER.layout().touch, None);
    }
}
