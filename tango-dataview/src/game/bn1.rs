pub mod rom;
pub mod save;

pub const NUM_CHIPS: usize = 238;
/// Number of chips the save's pack count table tracks (ids 0..this).
/// Reading `pack_count` past it would hit an adjacent save structure.
pub const NUM_PACK_CHIPS: usize = 238;
