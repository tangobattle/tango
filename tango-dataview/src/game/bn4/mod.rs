pub mod rom;
pub mod save;

pub const NUM_CHIPS: usize = 350;
/// Number of chips the save's pack/library table tracks (ids 0..this).
/// Entries past it (Program Advances, etc.) have no pack slot.
pub const NUM_PACK_CHIPS: usize = 321;
pub const NUM_PATCH_CARD4S: usize = 134;
pub const NUM_NAVICUST_PARTS: usize = 188;
