//! Reliable, ordered delivery over an unreliable, unordered datagram channel —
//! the live-match netplay protocol, split out as a transport-, engine-, and
//! packing-agnostic crate.
//!
//! Everything is generic over one [`Protocol`] impl — usually a zero-sized
//! marker — that names the element each seq slot carries and the per-frame meta
//! side-channel (each a [`Codec`]), so call sites read `Frame<MyProto>` rather
//! than carrying each type separately.
//!
//! Two layers:
//!
//! * [`frame`] — the on-wire [`Frame`]: a per-tick seq (`base`), a
//!   caller-defined meta side-channel, a run of elements, and a piggybacked
//!   cumulative ack, byte-minimized (LEB128 varints, ack as a signed delta).
//!   rennet owns the envelope and the run framing (it concatenates the elements
//!   and decodes them back until the datagram runs out); each element's [`Codec`]
//!   need only self-delimit, and the meta's likewise. *What* the meta means (a
//!   time-sync `tick_advantage`, …) and *how* a single element packs are the
//!   caller's business; with the `()` meta the frame carries no side-channel at
//!   all.
//! * [`stream`] — the reliability state machines: [`OutStream`] keeps a
//!   redundancy window and trims it on acks; [`InStream`] reassembles the peer's
//!   stream in strict seq order, dedups redundant copies, emits the cumulative
//!   ack, and bails past a configurable rollback horizon.
//!
//! Recovery is proactive — a lost element rides again in the next frame's
//! window, so single/short losses cost ~one frame rather than a round-trip. The
//! crate is pure: no async, no I/O, no transport. The caller pumps `Frame`
//! bytes over whatever datagram channel it has and maps elements to its own
//! event type.

pub mod frame;
pub mod stream;

pub use frame::{read_svarint, read_uvarint, write_svarint, write_uvarint, Ack, Codec, Frame, Protocol};
pub use stream::{HorizonExceeded, InStream, OutStream, Window, REDUNDANCY};

/// Test fixtures shared by the `frame` and `stream` unit tests: an example
/// element [`El`] (a [`Codec`]) and the [`Protocol`]s that pair it with a meta.
#[cfg(test)]
pub(crate) mod testutil {
    use crate::frame::{Codec, Protocol};
    use std::io;

    /// Cap on elements per datagram for the test protocols (stands in for the
    /// engine's rollback horizon).
    const MAX_RUN: usize = 300;

    /// Bit 14 marks a boundary; bits 0..=9 carry the payload — the packing the
    /// tango client uses (no continuation bit: the datagram delimits the run).
    const MARK: u16 = 0x4000;
    const PAYLOAD: u16 = 0x03ff;

    /// Example element: an input or a round/match boundary, packed as one
    /// little-endian `u16` — the tango client's packing, so the `frame`
    /// exact-byte tests built on it document the real wire form.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum El {
        Input(u16),
        EndOfRound,
        EndOfMatch,
    }

    impl Codec for El {
        fn encode<W: io::Write>(&self, w: &mut W) -> io::Result<()> {
            let v = match self {
                El::Input(j) => j & PAYLOAD,
                El::EndOfRound => MARK,
                El::EndOfMatch => MARK | 1,
            };
            w.write_all(&v.to_le_bytes())
        }

        fn decode<R: io::Read>(r: &mut R) -> io::Result<Option<Self>> {
            let mut b = [0u8; 2];
            // First byte: 0 bytes read = clean EOF at the run boundary.
            if r.read(&mut b[..1])? == 0 {
                return Ok(None);
            }
            // Second byte: EOF here is a truncated element.
            r.read_exact(&mut b[1..])?;
            let v = u16::from_le_bytes(b);
            let el = if v & MARK != 0 {
                match v & PAYLOAD {
                    0 => El::EndOfRound,
                    1 => El::EndOfMatch,
                    other => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("kind {other}"))),
                }
            } else {
                El::Input(v & PAYLOAD)
            };
            Ok(Some(el))
        }
    }

    /// A plain `i16` meta for the tests — stands in for a caller's time-sync
    /// field; rennet's own only [`Codec`] meta is the zero-width `()`.
    impl Codec for i16 {
        fn encode<W: io::Write>(&self, w: &mut W) -> io::Result<()> {
            crate::frame::write_svarint(w, *self as i64)
        }
        fn decode<R: io::Read>(r: &mut R) -> io::Result<Option<Self>> {
            // The meta is required: a short read errors (never a clean `None`).
            let v = i16::try_from(crate::frame::read_svarint(r)?)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "meta out of i16 range"))?;
            Ok(Some(v))
        }
    }

    /// Test [`Protocol`] pairing [`El`] with a plain `i16` meta. Markers derive
    /// the standard traits so [`Frame`](crate::Frame)/[`Window`](crate::Window)
    /// can derive theirs.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct RawProto;
    impl Protocol for RawProto {
        type Element = El;
        type Meta = i16;
        const MAX_RUN: usize = MAX_RUN;
    }

    /// Like [`RawProto`] but with the zero-width `()` meta, to exercise the
    /// no-side-channel path.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct RawUnitProto;
    impl Protocol for RawUnitProto {
        type Element = El;
        type Meta = ();
        const MAX_RUN: usize = MAX_RUN;
    }
}
