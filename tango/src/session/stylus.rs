//! The pointer standing in for a DS stylus over the emulator surface.

/// What the framebuffer widget's mouse area reports for a console with
/// a touch screen. Positions arrive already mapped into the touch
/// screen's own pixels (clamped to its edges), plus whether the raw
/// cursor was actually over that screen — a press only counts there,
/// but a drag in progress follows the clamped position off the edge the
/// way a real stylus scrapes along the bezel.
#[derive(Debug, Clone, Copy)]
pub enum StylusEvent {
    Moved { pos: (u16, u16), inside: bool },
    Pressed,
    Released,
}

/// Stylus interaction state: where the cursor last was over the
/// emulator surface, and whether a touch is in progress. Inert unless
/// the running console has a touch screen.
#[derive(Default)]
pub struct Stylus {
    /// Last reported position (clamped into the touch screen) and
    /// whether the cursor was truly inside it.
    hover: Option<((u16, u16), bool)>,
    /// A press landed inside the touch screen and hasn't lifted.
    down: bool,
}

impl Stylus {
    /// Fold one pointer event into the stylus.
    pub(super) fn apply(&mut self, event: StylusEvent) {
        match event {
            StylusEvent::Moved { pos, inside } => {
                self.hover = Some((pos, inside));
            }
            StylusEvent::Pressed => {
                // A touch starts only on the touch screen itself;
                // a press over the top screen is just a click.
                if let Some((_, true)) = self.hover {
                    self.down = true;
                }
            }
            StylusEvent::Released => {
                self.down = false;
            }
        }
    }

    /// The touch the session should see right now.
    pub(super) fn touch(&self) -> Option<(u16, u16)> {
        match (self.down, self.hover) {
            (true, Some((pos, _))) => Some(pos),
            _ => None,
        }
    }
}
