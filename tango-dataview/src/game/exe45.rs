pub mod rom;
pub mod save;

pub const NUM_CHIPS: usize = 350;
/// Number of chips the save's pack count table tracks (ids 0..this).
/// The chip-data table has more entries (Program Advances, etc.) past it
/// with no pack slot; reading `pack_count` for them would hit an adjacent
/// save structure and surface chips the player doesn't own.
pub const NUM_PACK_CHIPS: usize = 320;
pub const NUM_NAVIS: usize = 23;
