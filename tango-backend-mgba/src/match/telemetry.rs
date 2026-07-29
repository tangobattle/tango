//! Driving the shared telemetry from an mgba pair.
//!
//! The fold itself lives in the seam — it is the same arithmetic for any
//! console. All that is engine-specific is knowing where its two cores
//! are: the live path drives the collector from inside
//! [`crate::link::Link`]'s tick, and the offline paths
//! ([`analysis`](crate::r#match::analysis), replay prefetch) drive it
//! directly off the pair they step.

pub use tango_match::telemetry::*;

/// This engine's telemetry: the shared collector, reading mgba cores.
pub type Telemetry = tango_match::telemetry::Telemetry<mgba::core::Core>;

/// A poller over an mgba core.
pub type MgbaPoller = dyn tango_match::telemetry::CorePoller<mgba::core::Core>;

/// Drive the collector one tick off a bare pair — what the live link
/// does internally, for the offline paths (replay re-analysis, the
/// probe harnesses) that step a pair by hand.
pub fn observe_pair(telemetry: &mut Telemetry, pair: &mut mgba_rollback::Link, tick: u32) {
    let obs0 = telemetry.poll(0, pair.core_mut(0));
    let obs1 = telemetry.poll(1, pair.core_mut(1));
    telemetry.observe(obs0, obs1, tick);
}
