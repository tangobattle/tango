mod abilities;
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
pub const NUM_STYLES: usize = 0x40;

/// Whether `id` is a real Style. The id packs element in bits 0..=2 (only
/// the five elements 0..=4 exist) and type in bits 3..=5; bits 6-7 are
/// unused. Anything outside that — bits 6-7 set, or element 5..=7 (which
/// would alias another type's entry in the name table) — isn't a style.
pub(crate) fn is_valid_style(id: u8) -> bool {
    id & 0xc0 == 0 && id & 0x07 <= 4
}
