//! The data-channel identity every implementation must reproduce: two
//! channels, pre-negotiated on fixed SCTP stream ids (no in-band DCEP).
//! The stream id is what's load-bearing on the wire in negotiated mode;
//! the labels are local names kept identical for sanity. Reliability
//! config stays with each backend (libdatachannel's `DataChannelInit`
//! on desktop, `RTCDataChannelInit` on web): control is
//! reliable/ordered defaults, in-match is unordered with zero
//! retransmits.

/// The reliable, ordered control/lobby channel.
pub const CONTROL_LABEL: &str = "tango";
pub const CONTROL_STREAM_ID: u16 = 0;

/// The unreliable, unordered in-match channel (`maxRetransmits: 0` —
/// rennet's redundancy window replaces retransmission).
pub const IN_MATCH_LABEL: &str = "tango-match";
pub const IN_MATCH_STREAM_ID: u16 = 1;
