//! The per-tick stats sample encoding: what the gamesupport pollers
//! report each simulated tick and the [`crate::analysis`] fold consumes.
//! The trap-driven netplay engine that used to live here (`Match`/`Round`,
//! the shadow co-sim netcode) is gone — PvP runs on the SIO-lockstep
//! engine (see [`crate::engine`]) — and the host-side netcode sizing
//! that used to sit alongside it lives with the host's netcode now.

/// One simulated tick's event sample, oriented to this side of the match —
/// everything the stats fold consumes: both navis' HP, the custom-screen
/// flag, the A/B button states, and the loaded-chip reports. `tick` is the
/// tick that was simulated (not the boundary it produced), so consecutive
/// samples are dense except for ticks the per-game reporting skipped (battle
/// intro, before the unit structs are live).
#[derive(Clone, Copy)]
pub struct RoundSample {
    pub tick: u32,
    pub local: u16,
    pub remote: u16,
    /// Whether the custom screen (chip select) was open this tick — false
    /// on games that don't report it.
    pub custom: bool,
    /// Both players' A/B button state this tick (see the `BUTTON_*` bit
    /// constants) — the raw held bits from the tick's confirmed input
    /// pair, from which buster usage events are derived downstream.
    pub buttons: u8,
    /// `[local, remote]` loaded chip ids this tick ([`NO_CHIP`] = none or
    /// not reported) — chip-use events are their departures downstream.
    pub chips: [u16; 2],
}

/// Sentinel for "no chip loaded" in [`RoundSample::chips`] — the games' own
/// in-memory sentinel.
pub const NO_CHIP: u16 = 0xffff;

/// Bits of [`RoundSample::buttons`].
pub const BUTTON_LOCAL_A: u8 = 1 << 0;
pub const BUTTON_LOCAL_B: u8 = 1 << 1;
pub const BUTTON_REMOTE_A: u8 = 1 << 2;
pub const BUTTON_REMOTE_B: u8 = 1 << 3;
