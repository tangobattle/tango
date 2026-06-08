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
