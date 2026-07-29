//! One GBA booted alone, as the seam's [`Console`](tango_match::Console).

use crate::link::{GbaSide, JOYFLAGS_MASK};

/// One console on a one-side link — one GBA, not a pair with an idle
/// seat — which is still a *link*, so the cart sees its link hardware
/// from power-on and a future netplay handoff has a cable to plug
/// into.
pub struct SoloConsole {
    link: mgba_rollback::Link,
}

impl SoloConsole {
    /// Boot one console. `rtc` pins the cart clock exactly as a match's
    /// negotiated clock does; `None` leaves it on the real one.
    pub fn new(rom: &[u8], save: Option<&[u8]>, rtc: Option<std::time::SystemTime>) -> Result<Self, crate::Error> {
        crate::install_logger();
        let mut link = mgba_rollback::Link::with_options(mgba_rollback::LinkOptions {
            sides: vec![mgba_rollback::SideOptions {
                rom: rom.to_vec(),
                save: save.map(<[u8]>::to_vec),
            }],
            rtc,
            peripheral: mgba_rollback::Peripheral::Cable,
        })?;
        // This buffer *is* the session's audio queue: the stream leaves
        // its backlog here rather than pulling it out. It therefore has
        // to hold the stream's discard cap — 3x a 120 ms target — plus
        // what fast-forward piles up between fills, at BN4+'s 65536 Hz,
        // and with room to spare: mGBA's ring drops new writes when
        // full, so overflowing it loses audio silently. Same sizing as
        // the pair engine.
        link.core_mut(0).set_audio_buffer_size(32768);
        link.core_mut(0).audio_buffer().clear();
        Ok(SoloConsole { link })
    }
}

impl tango_match::Console for SoloConsole {
    fn tick(&mut self, input: tango_match::HostInput) -> Result<(), tango_match::Error> {
        // A GBA has no touch screen, so only the pad half applies —
        // masked to the pad exactly as a link sanitizes.
        self.link
            .try_tick(&[input.keys & JOYFLAGS_MASK])
            .map_err(|e| tango_match::Error::Backend(Box::new(crate::Error::from(e))))?;
        Ok(())
    }

    fn side(&mut self) -> Box<dyn tango_match::Side + '_> {
        Box::new(GbaSide {
            link: &mut self.link,
            player: 0,
        })
    }
}
