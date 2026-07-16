//! Match-level parameters shared by the SIO netplay session, the
//! replay/analysis machinery, and the host's netcode sizing. The
//! trap-driven netplay engine that used to live here (`Match`/`Round`,
//! the shadow co-sim netcode) is gone — PvP runs on the SIO-lockstep
//! engine (see [`crate::engine`]).

/// Picks the per-match local_player_index. Both peers must call this with
/// the same shared RNG state at the same point in the protocol so they end
/// up on opposite sides. Advances the RNG by one draw.
pub fn pick_local_player_index(rng: &mut rand_pcg::Mcg128Xsl64, is_offerer: bool) -> u8 {
    use rand::Rng;
    let did_polite_win = rng.gen::<bool>();
    if did_polite_win == is_offerer {
        0
    } else {
        1
    }
}

/// GBA video framerate in frames per second.
pub const EXPECTED_FPS: f32 = 16777216.0 / 280896.0;

/// In-match input-buffer budget — two coupled depths expressed as one.
///
/// The depth the session waits for before declaring a dead link and the
/// rollback horizon the engine bails at used to be tuned by hand (the former as
/// a silence *duration*) and could drift apart. [`RECONNECT_QUEUE_LENGTH`] is
/// now the single knob; [`MAX_QUEUE_LENGTH`] (the horizon) is *derived* from it,
/// so the horizon can't end up smaller than the depth it has to out-cover.
///
/// Why watch the queue and not elapsed silence: a dead link keeps the sim
/// committing ~one local input per displayed frame (the throttler caps its
/// slowdown, so it never fully stalls) with nothing from the peer to match them
/// against, so the local input queue climbs steadily. The session polls that
/// depth directly and pauses to reconnect once it reaches
/// [`RECONNECT_QUEUE_LENGTH`]. Measuring the very resource that overflows — not
/// a time proxy for it — means the trip can't drift from the bail no matter how
/// fast the throttled sim actually grows the queue: the watchdog always fires a
/// fixed margin below the horizon.
///
/// That margin is [`STALL_HEADROOM`]: it covers the watchdog's poll interval and
/// the frame or two the pause takes to land, plus a safety factor — sized so the
/// overflow bail can never beat the watchdog + pause to the punch.
///
/// The session reads [`RECONNECT_QUEUE_LENGTH`] back to drive its watchdog.
/// Lower it to trip reconnect sooner (the horizon shrinks with it); raise it to
/// ride out longer blips (the horizon grows). Nothing else to retune.
///
/// 180 frames ≈ 3 s of play (at 60 fps, just above [`EXPECTED_FPS`]).
pub const RECONNECT_QUEUE_LENGTH: usize = 180;

/// Slack between the reconnect trip depth and the hard overflow bail — see
/// [`RECONNECT_QUEUE_LENGTH`]. It need not match the trip depth itself; the slop
/// it has to cover is a handful of frames, far short of the depth's worth of
/// growth. 90 frames ≈ 1.5 s.
const STALL_HEADROOM: usize = 90;

/// Per-side input-queue capacity (the rollback horizon): how many local inputs
/// may sit unmatched against remote ones (and vice versa) before the engine
/// bails and cancels the match. Derived from [`RECONNECT_QUEUE_LENGTH`] — see
/// that constant for the budget. Public because it's the backpressure bound
/// other layers size against (the host's send pump, rennet's redundancy window
/// and reorder buffer); anything queueing inputs upstream can hold a bit more
/// and rely on this bail — or the session's earlier reconnect pause — firing
/// first.
pub const MAX_QUEUE_LENGTH: usize = RECONNECT_QUEUE_LENGTH + STALL_HEADROOM;

/// Inclusive bounds for a side's `frame_delay`, which is realized purely as
/// local frame delay (how far the display trails the netcode frontier).
/// Each side picks its own; there's no negotiation. The lobby slider and config
/// clamp to this range. 0 presents the frontier itself — pure rollback, every
/// misprediction visible immediately; the default (`default_frame_delay`, 2)
/// stays above it, and the ping-based suggestion never lands below 1.
pub const MIN_FRAME_DELAY: u32 = 0;
pub const MAX_FRAME_DELAY: u32 = 10;

pub fn suggest_frame_delay(rtt: std::time::Duration) -> u32 {
    let one_way_frames = (rtt.as_millis() * 60 / 2 / std::time::Duration::from_secs(1).as_millis()) as i32;
    (one_way_frames + 1).clamp(MIN_FRAME_DELAY as i32, MAX_FRAME_DELAY as i32) as u32
}

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
