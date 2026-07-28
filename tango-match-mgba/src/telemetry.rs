//! Driving the shared telemetry from an mgba pair.
//!
//! The fold itself lives in the seam — it is the same arithmetic for any
//! console. All that is engine-specific is being an mgba tick observer
//! and knowing where its two cores are, and the orphan rule wants that
//! impl on a local type.

pub use tango_match::telemetry::*;

/// This engine's telemetry: the shared collector, reading mgba cores.
pub type Telemetry = tango_match::telemetry::Telemetry<mgba::core::Core>;

/// A poller over an mgba core.
pub type MgbaPoller = dyn tango_match::telemetry::CorePoller<mgba::core::Core>;

/// The collector plus the observer impl that drives it.
pub struct MgbaTelemetry(pub Telemetry);

impl std::ops::Deref for MgbaTelemetry {
    type Target = Telemetry;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl mgba_rollback::session::TickObserver for MgbaTelemetry {
    fn on_tick(&mut self, pair: &mut mgba_rollback::Link, tick: u32) {
        let obs0 = self.0.poll(0, pair.core_mut(0));
        let obs1 = self.0.poll(1, pair.core_mut(1));
        self.0.observe(obs0, obs1, tick);
    }

    fn on_rewind(&mut self, tick: u32) {
        self.0.on_rewind(tick);
    }
}
