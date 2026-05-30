pub mod rom;
pub mod save;

pub const NUM_CHIPS: usize = 368;
/// Number of chips the save's pack/library table tracks (ids 0..this).
/// Entries past it (Program Advances, etc.) have no pack slot.
pub const NUM_PACK_CHIPS: usize = 321;
pub const NUM_PATCH_CARD56S: usize = 112;
pub const NUM_NAVICUST_PARTS: usize = 192;
pub const NUM_NAVIS: usize = 13;
