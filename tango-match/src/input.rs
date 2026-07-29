/// One tick of input as a host collects it for the local player: the
/// engine-neutral joypad word plus the stylus only a touch-screen
/// console reads.
///
/// This is the host-side vocabulary, distinct from a backend's own
/// input type — each engine derives its own from it and ignores what
/// its console has no word for (a GBA drops the stylus outright).
/// The netplay wire is narrower still: it exchanges bare joyflag
/// words, so a stylus only exists on the rides a host feeds directly
/// ([`RunningSolo`](crate::RunningSolo) today).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct HostInput {
    /// Held joypad bits (see [`keys`](crate::keys)).
    pub keys: u32,
    /// Stylus position on the console's touch screen, in that screen's
    /// own pixels, or `None` for a lifted stylus.
    pub touch: Option<(u16, u16)>,
}

impl HostInput {
    /// Input with nothing but the joypad held — what every console
    /// without a touch screen takes.
    pub fn keys(keys: u32) -> Self {
        HostInput { keys, touch: None }
    }
}
