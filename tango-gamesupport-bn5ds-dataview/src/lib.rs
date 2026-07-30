//! The BN5 Double Team DS dataview.
//!
//! What the GBA games' dataview crates are to their saves and ROMs,
//! this is to the DS cart — as far as the format has been mapped. Save
//! recognition is complete; the save interior is a frontier that so
//! far reaches the chip folders, and the cart's chip assets (stats,
//! names, descriptions, icons) are mapped far enough to label them.

pub mod rom;
pub mod save;

/// Entries in the cart's chip table — the GBA game's 368 plus Double
/// Team's additions (both versions' navi chips and the DS-only ones).
/// The two name archives' entry counts (256 + 168) agree exactly.
pub const NUM_CHIPS: usize = 424;
