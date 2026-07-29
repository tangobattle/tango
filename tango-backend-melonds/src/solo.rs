//! One DS booted alone, as the seam's [`Console`](tango_match::Console).

use crate::link::{input_of, rtc_parts, DsSide};

/// One DS booted alone, as the seam's [`Console`](tango_match::Console):
/// a single core, not a pair with an idle seat.
pub struct SoloConsole {
    inner: melonds_rollback::Solo,
}

impl SoloConsole {
    /// Boot one console. `rtc` pins the cart clock, exactly as a
    /// match's negotiated clock does.
    pub fn new(rom: &[u8], save: Option<&[u8]>, rtc: std::time::SystemTime) -> Result<Self, melonds::Error> {
        Ok(SoloConsole {
            inner: melonds_rollback::Solo::new(rom, save, rtc_parts(rtc))?,
        })
    }
}

impl tango_match::Console for SoloConsole {
    fn tick(&mut self, input: tango_match::HostInput) -> Result<(), tango_match::Error> {
        self.inner.tick(input_of(input));
        Ok(())
    }

    fn side(&mut self) -> Box<dyn tango_match::Side + '_> {
        Box::new(DsSide(self.inner.side()))
    }
}
