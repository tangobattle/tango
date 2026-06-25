//! Reliable, ordered delivery over an unreliable, unordered datagram channel —
//! the live-match netplay protocol, split out as a transport-, engine-, and
//! packing-agnostic crate.
//!
//! Two layers:
//!
//! * [`frame`] — the on-wire [`Frame`]: a per-tick seq (`base`), a time-sync
//!   `tick_advantage`, an opaque [`Body`], and a piggybacked cumulative ack,
//!   byte-minimized (LEB128 varints, ack as a signed delta). rennet owns only
//!   that envelope; the [`Body`] owns its own bytes and just has to
//!   self-delimit and expose its elements. *How* a body packs its elements
//!   (continuation-delimited, length-prefixed, …) is the caller's business.
//! * [`stream`] — the reliability state machines, generic over the element
//!   type: [`OutStream`] keeps a redundancy window and trims it on acks;
//!   [`InStream`] reassembles the peer's stream in strict seq order, dedups
//!   redundant copies, emits the cumulative ack, and bails past a configurable
//!   rollback horizon.
//!
//! Recovery is proactive — a lost element rides again in the next frame's
//! window, so single/short losses cost ~one frame rather than a round-trip. The
//! crate is pure: no async, no I/O, no transport. The caller pumps `Frame`
//! bytes over whatever datagram channel it has and maps elements to its own
//! event type.

pub mod frame;
pub mod stream;

pub use frame::{Ack, Body, Frame};
pub use stream::{HorizonExceeded, InStream, OutStream, Window, REDUNDANCY};

/// Example [`Body`] impls shared by the `frame` and `stream` unit tests. Two
/// different packings of the same element type, so the tests double as a
/// demonstration that the frame envelope and the reliability streams don't care
/// how a body is laid out.
#[cfg(test)]
pub(crate) mod testutil {
    use crate::frame::Body;
    use std::io;

    /// Bit 14 marks a boundary; bits 0..=9 carry the payload — the packing the
    /// tango client uses (no continuation bit: the datagram delimits the run).
    const MARK: u16 = 0x4000;
    const PAYLOAD: u16 = 0x03ff;
    /// Entry cap (the engine's rollback horizon), shared by both test bodies.
    pub const MAX_RUN: usize = 300;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum El {
        Input(u16),
        EndOfRound,
        EndOfMatch,
    }

    fn el_to_u16(e: &El) -> u16 {
        match e {
            El::Input(j) => j & PAYLOAD,
            El::EndOfRound => MARK,
            El::EndOfMatch => MARK | 1,
        }
    }

    fn el_from_u16(w: u16) -> io::Result<El> {
        if w & MARK != 0 {
            match w & PAYLOAD {
                0 => Ok(El::EndOfRound),
                1 => Ok(El::EndOfMatch),
                other => Err(io::Error::new(io::ErrorKind::InvalidData, format!("kind {other}"))),
            }
        } else {
            Ok(El::Input(w & PAYLOAD))
        }
    }

    fn invalid(msg: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
    }

    /// Raw body: one little-endian `u16` per entry, no internal framing — the
    /// run fills the datagram tail. This is the tango client's packing, so the
    /// `frame` exact-byte tests built on it document the real wire form.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct RawBody(pub Vec<El>);

    impl Body for RawBody {
        type Elem = El;

        fn encode(&self, out: &mut Vec<u8>) {
            for e in &self.0 {
                out.extend_from_slice(&el_to_u16(e).to_le_bytes());
            }
        }

        fn decode(bytes: &[u8]) -> io::Result<Self> {
            if !bytes.len().is_multiple_of(2) {
                return Err(invalid("body length is not a whole number of u16 entries"));
            }
            if bytes.len() / 2 > MAX_RUN {
                return Err(invalid("run exceeds cap"));
            }
            let v = bytes
                .chunks_exact(2)
                .map(|c| el_from_u16(u16::from_le_bytes([c[0], c[1]])))
                .collect::<io::Result<Vec<_>>>()?;
            Ok(RawBody(v))
        }

        fn elements(&self) -> &[El] {
            &self.0
        }
    }

    /// Length-prefixed body: a single leading byte counts the entries (test runs
    /// stay short), then the entries. A different packing of the same elements,
    /// to prove rennet is body-agnostic.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct LenBody(pub Vec<El>);

    impl Body for LenBody {
        type Elem = El;

        fn encode(&self, out: &mut Vec<u8>) {
            out.push(self.0.len() as u8);
            for e in &self.0 {
                out.extend_from_slice(&el_to_u16(e).to_le_bytes());
            }
        }

        fn decode(bytes: &[u8]) -> io::Result<Self> {
            let (&len, rest) = bytes.split_first().ok_or_else(|| invalid("missing length prefix"))?;
            let len = len as usize;
            if rest.len() != len * 2 {
                return Err(invalid("length prefix disagrees with body size"));
            }
            let v = rest
                .chunks_exact(2)
                .map(|c| el_from_u16(u16::from_le_bytes([c[0], c[1]])))
                .collect::<io::Result<Vec<_>>>()?;
            Ok(LenBody(v))
        }

        fn elements(&self) -> &[El] {
            &self.0
        }
    }
}
