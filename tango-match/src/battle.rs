//! The per-tick stats sample encoding: what the gamesupport pollers
//! report each simulated tick and the `analysis` fold consumes.
//! The trap-driven netplay engine that used to live here (`Match`/`Round`,
//! the shadow co-sim netcode) is gone — PvP runs on the SIO-lockstep
//! engine (see the engine) — and the host-side netcode sizing
//! that used to sit alongside it lives with the host's netcode now.

/// One simulated tick's level sample, oriented to this side of the match —
/// everything the stats fold consumes: both navis' HP and the custom-screen
/// flag. Chip uses are events, not samples — they arrive through the
/// telemetry event stream. `tick` is the tick that was simulated (not the
/// boundary it produced), so consecutive samples are dense except for ticks
/// the per-game reporting skipped (battle intro, before the unit structs
/// are live).
#[derive(Clone, Copy)]
pub struct RoundSample {
    pub tick: u32,
    pub local: u16,
    pub remote: u16,
    /// Whether the custom screen (chip select) was open this tick — false
    /// on games that don't report it.
    pub custom: bool,
}
