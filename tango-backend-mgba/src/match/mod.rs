//! Running a GBA match: the engine, its replay playback, its RAM-poll
//! telemetry and the offline analysis that re-simulates one.
//!
//! Everything here is mgba-shaped. The parts that are not — the rollback
//! loop itself, the telemetry fold, the sample encoding — live in
//! `tango-match`, which is why this module is as small as it is.

pub mod analysis;
pub mod engine;
pub mod playback;
pub mod telemetry;
