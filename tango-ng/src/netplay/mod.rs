//! Netplay state machine — being ported from `tango/src/netplay/` per the
//! saved port guide. This stub carries the protocol constant the `net`
//! layer needs; the Phase/State machine, connect/handshake/lobby, and
//! compat modules land next.

/// The netplay protocol version, negotiated in both the signaling connect
/// and the per-peer control-channel Hello. 0x4c is the str0m/tango-rtc
/// cut, incompatible with the old libdatachannel 0x4b.
pub const PROTOCOL_VERSION: u32 = 0x4c;
