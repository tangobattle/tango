//! The melonDS link: [`tango_match::Link`] over a pair of emulated
//! DSes on emulated local wireless.
//!
//! The sibling of `tango-backend-mgba`'s link. Both answer the same
//! questions — how does a pair tick, snapshot, restore and draw — for
//! very different hardware, which is what lets one
//! [`Match`](tango_match::Match) drive either.

use tango_match::telemetry::Telemetry;
use tango_match::{AudioSampleRate, HostInput, Screen, ScreenLayout};

/// The rate the SPU hands samples out at.
///
/// The DS master clock is 33,513,982 Hz and the SPU emits one sample
/// every 1,024 cycles. The melonDS shim configures its output to this
/// exact rate, so rate conversion belongs to the host audio path rather
/// than happening inside the emulator first.
pub const SAMPLE_RATE: AudioSampleRate = AudioSampleRate::new(
    melonds::AUDIO_SAMPLE_RATE.numerator,
    melonds::AUDIO_SAMPLE_RATE.denominator,
);

/// The DS's video framerate, which is also the rate audio production
/// scales against when a host paces the simulation faster or slower.
pub const EXPECTED_FPS: f64 = 16756991.0 / 280095.0;

/// The DS's whole input word: the GBA's ten buttons, X and Y, and the
/// mic — which is every bit [`keys`](tango_match::keys) names, all of
/// them reaching this console.
pub const KEYS_MASK: u32 = tango_match::keys::MASK;

/// The pad half of [`KEYS_MASK`] — what melonDS's own key word takes.
/// The mic rides above it and is handed to the console separately.
const PAD_MASK: u32 = KEYS_MASK & !tango_match::keys::MIC;

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
        // A screen this selection leaves out is never composed, so
        // nothing will ever look at the engine that draws it: say so
        // once here rather than waiting for a host to ask. That is what
        // gets replay playback, and any other boot, the saving without
        // having to know about screens at all.
        //
        // A host may narrow this further per tick
        // ([`tango_match::Side::set_displayed_screens`]); it can never
        // widen it, since that mask is indexed by this same selection.
        let mask = screens.0.iter().fold(0u8, |m, &s| m | 1 << s as u8);
        for player in 0..2 {
            self.inner.console(player).set_displayed_screens(mask);
        }
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
        // Observations are stamped with the just-simulated tick's own
        // index — the first session tick is tick 0, the mgba engine's
        // numbering and the input stream's. (They were stamped with the
        // post-increment count once, which shifted every DS recording's
        // telemetry one tick late — invisible until a first-round start
        // at tick 1 began reading as a setup section.)
        if let Some(telemetry) = self.telemetry.as_mut() {
            let obs0 = telemetry.poll(0, self.inner.console(0));
            let obs1 = telemetry.poll(1, self.inner.console(1));
            telemetry.observe(obs0, obs1, self.live_tick);
        }
        self.live_tick += 1;
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
            // A snapshot's tick is a COUNT — the state after ticks
            // 0..tick — so those are the observations that survive it:
            // revoke tick and up, the re-simulation re-reports them.
            // (`on_rewind` keeps ≤ its argument.) The saturated case —
            // restoring the pre-first-tick capture — over-keeps tick
            // 0's observation; the re-simulated tick 0 re-observes it
            // identically and the fold's same-tick dedup absorbs it.
            telemetry.on_rewind(snapshot.tick.saturating_sub(1));
        }
        Ok(())
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

    /// The mask arrives over the composition's order, which is this
    /// boot's [`Screens`] selection - so a touch-only mode's bit 0 is the
    /// console's *bottom* screen. Translate to the console's own order
    /// before handing it over.
    ///
    /// A screen the composition leaves out is therefore never displayed
    /// whatever the host asks for, which is what gets a cart that spends
    /// its netbattle on one screen the saving without anyone opting in.
    fn set_displayed_screens(&mut self, screens: u8) {
        let mut mask = 0u8;
        for (i, &screen) in self.1 .0.iter().enumerate() {
            if screens & (1 << i) != 0 {
                mask |= 1 << screen as u8;
            }
        }
        self.0.console().set_displayed_screens(mask);
    }

    fn export_save(&mut self) -> Option<Vec<u8>> {
        Some(self.0.console().save_memory())
    }

    fn audio_sample_rate(&mut self) -> AudioSampleRate {
        SAMPLE_RATE
    }

    /// Taken from the boot rather than straight off the SPU, which at
    /// ~43 ms overflows within a couple of frames of a re-simulation
    /// appending a span twice — destroying its own oldest audio to make
    /// room. The boot empties it every tick into a buffer of its own,
    /// and a session empties that every tick in turn, so neither ever
    /// has to hold more than the tick just finished.
    fn drain_audio(&mut self, out: &mut [i16]) -> usize {
        let (written, queued) = self.0.take_audio(out);
        written + queued
    }
}

/// Compose the core's unpacked BGR666 screens into one RGBA8 frame, in
/// the selection's own order — which is [`Screens::layout`]'s, so the
/// frame and the layout describing it never disagree.
///
/// Side by side, so a row of the composite is a row of each selected
/// screen in turn. Stacked would be the cheaper concatenation — a
/// vertical stack is free when the widths match — but a 256x384 pane
/// wastes most of the width of any display it is drawn into.
fn compose_frame(
    top: &[melonds::UnpackedBgr666],
    bottom: &[melonds::UnpackedBgr666],
    screens: Screens,
) -> Vec<u8> {
    let sources = [top, bottom];
    let (width, height) = (SCREENS[0].width as usize, SCREENS[0].height as usize);
    let mut rgba = vec![0u8; screens.layout().buffer_len()];
    // A row of the composite is a row of each selected screen in turn,
    // so walk the destination in those spans and pair each with its
    // source row. Handing the inner loop a slice of known length and a
    // 4-byte destination is what keeps a bounds check off every pixel —
    // this runs on a whole frame every tick a session presents one.
    let span = width * 4;
    let mut at = 0usize;
    for row in 0..height {
        for &screen in screens.0 {
            let src = &sources[screen as usize][row * width..(row + 1) * width];
            let dst = &mut rgba[at..at + span];
            for (pixel, out) in src.iter().zip(dst.chunks_exact_mut(4)) {
                out.copy_from_slice(&unpacked_bgr666_to_rgba8(*pixel));
            }
            at += span;
        }
    }
    rgba
}

/// Expand one of melonDS's native six-bit compositor pixels to the
/// RGBA8 presentation format the backend seam promises hosts. Each
/// component is mapped proportionally with `value * 255 / 63`, the
/// six-bit counterpart of the mGBA backend's BGR555 conversion.
#[inline]
pub fn unpacked_bgr666_to_rgba8(pixel: melonds::UnpackedBgr666) -> [u8; 4] {
    let expand = |value: u8| (u16::from(value) * 0xff / 0x3f) as u8;
    [expand(pixel.red()), expand(pixel.green()), expand(pixel.blue()), 0xff]
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

/// The console's own input for one sanitized host input. The mic is a
/// bit of the seam's word and a field of the console's, so this is where
/// the two part company.
pub(crate) fn input_of(input: HostInput) -> crate::Input {
    let input = sanitize(input);
    crate::Input {
        keys: input.keys & PAD_MASK,
        touch: input.touch,
        mic: input.keys & tango_match::keys::MIC != 0,
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
    #[test]
    fn the_spu_rate_keeps_the_hardware_ratio() {
        assert_eq!(
            super::SAMPLE_RATE,
            tango_match::AudioSampleRate::new(33_513_982, 1_024)
        );
        assert_eq!(super::SAMPLE_RATE.as_f64(), 33_513_982.0 / 1_024.0);
    }

    /// The shared rollback loop accepts this link — the point of the
    /// seam. A DS match and a GBA match are the same
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

    /// The mic rides the seam's input word and the console takes it as
    /// a field, so [`input_of`](super::input_of) is where the two part.
    /// A pad bit left in the console's key word would be a key the DS
    /// does not have; a mic bit dropped here would be a binding that
    /// does nothing.
    #[test]
    fn the_mic_bit_reaches_the_console_without_reaching_its_pad() {
        use tango_match::keys;
        let held = super::input_of(tango_match::HostInput::keys(keys::MIC | keys::A));
        assert!(held.mic);
        assert_eq!(held.keys, keys::A);

        let lifted = super::input_of(tango_match::HostInput::keys(keys::A));
        assert!(!lifted.mic);
        assert_eq!(lifted.keys, keys::A);
    }

    /// The core hands out unpacked BGR666 and a host wants RGBA8, with
    /// the alpha the console has no opinion about forced opaque. Pin the
    /// proportional component expansion rather than trusting the
    /// composition loop around it.
    #[test]
    fn an_unpacked_bgr666_word_composes_to_opaque_rgba8() {
        let px = (super::SCREENS[0].width * super::SCREENS[0].height) as usize;
        // 0xXXBBGGRR: b=0x11, g=0x22, r=0x33. XX and each component's
        // upper two byte bits carry no color information.
        let top = vec![melonds::UnpackedBgr666::from_raw(0x44_d1_a2_f3); px];
        let bottom = vec![melonds::UnpackedBgr666::from_raw(0); px];
        let out = super::compose_frame(&top, &bottom, super::Screens::UPPER);
        assert_eq!(&out[..8], &[0xce, 0x89, 0x44, 0xff, 0xce, 0x89, 0x44, 0xff]);
        assert_eq!(&out[out.len() - 4..], &[0xce, 0x89, 0x44, 0xff]);
    }

    /// A red top screen and a blue bottom one, as the core hands them
    /// out: unpacked BGR666 words, so red is `0x0000003f`.
    fn framebuffers() -> (Vec<melonds::UnpackedBgr666>, Vec<melonds::UnpackedBgr666>) {
        let px = (super::SCREENS[0].width * super::SCREENS[0].height) as usize;
        (
            vec![melonds::UnpackedBgr666::from_raw(0x0000_003f); px],
            vec![melonds::UnpackedBgr666::from_raw(0x003f_0000); px],
        )
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
