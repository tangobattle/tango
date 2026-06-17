/// Bit mask of a joyflags value: the GBA keypad is 10 bits (A, B, Select,
/// Start, →, ←, ↑, ↓, R, L), occupying bits 0..=9. The top 6 bits are unused by
/// the hardware, so callers are free to repurpose them — e.g. the live core's r4
/// high bits, or the netplay wire's CONT/MARK entry tags.
pub const JOYFLAGS_MASK: u16 = 0x03ff;

/// A committed local-side input plus the matching outgoing packet for that
/// tick. Tick is positional — derived from the input's position in its
/// round / queue, never embedded in the struct.
#[derive(Clone, Debug)]
pub struct Input {
    pub joyflags: u16,
    pub packet: Vec<u8>,
}

/// A committed input without its outgoing packet. Local inputs upgrade to
/// [`Input`] once the fastforwarder pairs them with a packet via
/// [`PartialInput::with_packet`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartialInput {
    pub joyflags: u16,
}

impl PartialInput {
    pub fn with_packet(self, packet: Vec<u8>) -> Input {
        Input {
            joyflags: self.joyflags,
            packet,
        }
    }
}
