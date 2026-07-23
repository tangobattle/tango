/// Bit mask of a joyflags value: the GBA keypad is 10 bits (A, B, Select,
/// Start, →, ←, ↑, ↓, R, L), occupying bits 0..=9. The top 6 bits are unused by
/// the hardware, so callers are free to repurpose them — e.g. the live core's r4
/// high bits, or the netplay wire's CONT/MARK entry tags.
pub const JOYFLAGS_MASK: u16 = 0x03ff;
