pub mod rom;
pub mod save;

pub const NUM_CHIPS: usize = 351;
/// Number of chips the save's pack count table tracks (ids 0..this).
/// The chip-data table has more entries (Program Advances, etc.) past it
/// with no pack slot; reading `pack_count` for them would hit an adjacent
/// save structure and surface chips the player doesn't own.
pub const NUM_PACK_CHIPS: usize = 320;
pub const NUM_NAVICUST_PARTS: usize = 204;
// Style id is bit-packed as `(typ << 3) | element` (see `RawStyle` in
// rom.rs), so valid ids span the full 6-bit range read from the save.
// The previous value (40) cut off Shadow/Bug styles for elements ≥ 3
// (WoodShadow=43, HeatBug=48, etc.). `Style::name()` and
// `extra_ncp_color()` already return None for gaps in the table.
pub const NUM_STYLES: usize = 64;
