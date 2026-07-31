//! The Rockman.EXE Operate Shooting Star dataview.
//!
//! What the GBA games' dataview crates are to their saves and ROMs, this
//! is to the OSS cart — as far as the format has been mapped. The cart
//! is a remake of BN1 with a Star Force crossover bolted on, and its
//! save calls itself `exe1ds` accordingly; recognition is settled, and
//! the interior now reaches the chip folder. The cart's chip assets
//! (stats, names, descriptions, icons, artwork) are mapped far enough
//! to label it.

pub mod rom;
pub mod save;

/// Entries in the cart's chip table — BN1's list carried forward, plus
/// the crossover's own (Star Force MegaMan and Rogue at 200/201) and
/// two Program Advances past where the GBA game's stop. The table ends
/// here: entry 240 is not a chip but whatever the linker put next.
pub const NUM_CHIPS: usize = 240;
