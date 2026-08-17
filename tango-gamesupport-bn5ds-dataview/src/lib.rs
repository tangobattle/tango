//! The BN5 Double Team DS dataview.
//!
//! What the GBA games' dataview crates are to their saves and ROMs,
//! this is to the DS cart — as far as the format has been mapped. Save
//! recognition is complete, and the save interior now reaches
//! everything the editor edits: the chip folders and pack, the
//! NaviCust, the navi's HP and the auto-battle data. The cart's chip
//! assets (stats, names, descriptions, icons) and its NaviCust program
//! table are mapped far enough to label and lay out both.

pub mod build;
pub mod rom;
pub mod save;

/// Entries in the cart's chip table — the GBA game's 368 plus Double
/// Team's additions (both versions' navi chips and the DS-only ones).
/// The two name archives' entry counts (256 + 168) agree exactly.
pub const NUM_CHIPS: usize = 424;

/// Entries in the cart's NaviCust program table: the GBA game's 192 —
/// 48 programs in four colours each, byte for byte — and thirteen more
/// the port adds on the end, which is where the table stops decoding.
/// The name archive's 52 entries (48 + 4 templates) agree.
pub const NUM_NAVICUST_PARTS: usize = 205;

/// Entries in the cart's party program table — the programs the PARTY
/// CUSTOMIZER equips on a team navi. The cart's own four tables run
/// thirteen entries apiece before their padding, and the item name
/// archive names exactly thirteen (`P.HP+50` through `P.Spport`).
pub const NUM_PARTY_PROGRAMS: usize = 13;

/// How many chips the auto-battle data counts uses of. The cart keeps
/// the GBA game's two 368-entry arrays rather than growing them to
/// [`NUM_CHIPS`], so Double Team's own chips have no use count: the two
/// arrays sit 0x2e0 apart in the save image, which is exactly 368 u16s.
pub const NUM_AUTO_BATTLE_DATA_CHIPS: usize = 368;
