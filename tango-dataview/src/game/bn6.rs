pub mod rom;
pub mod save;

pub const NUM_CHIPS: usize = 411;
/// Number of chips the save's pack/library table tracks (ids 0..this).
/// The chip-data table has more entries (Program Advances, etc.) past
/// it, which have no pack slot — reading `pack_count` for them would hit
/// adjacent save data.
pub const NUM_PACK_CHIPS: usize = 321;
pub const NUM_PATCH_CARD56S: usize = 118;
pub const NUM_NAVICUST_PARTS: usize = 188;
pub const NUM_NAVIS: usize = 12;
