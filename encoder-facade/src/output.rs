//! Writing a session's bytes to a seekable file.
//!
//! A convenience for native callers: a [`Session`](crate::Session)
//! deliberately does no I/O, and this is the whole of what a
//! synchronous caller needs to do with what it produces — append the
//! bytes as they come, then write the closing patches back over the
//! positions they belong to.

use std::io::{Seek, SeekFrom, Write};

use crate::mux::Patch;

pub struct Output<W: Write + Seek> {
    inner: W,
}

impl<W: Write + Seek> Output<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    /// Append container bytes from [`Session::take_output`](crate::Session::take_output).
    pub fn append(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.inner.write_all(bytes)
    }

    /// Apply the patches from [`Session::poll_finish`](crate::Session::poll_finish)
    /// and flush. Append any final bytes first — the patches refer to
    /// positions in the complete file.
    pub fn finish(&mut self, patches: &[Patch]) -> std::io::Result<()> {
        for patch in patches {
            self.inner.seek(SeekFrom::Start(patch.position))?;
            self.inner.write_all(&patch.bytes)?;
        }
        self.inner.flush()
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_land_where_they_are_addressed() {
        let mut output = Output::new(std::io::Cursor::new(Vec::new()));
        output.append(b"0123456789").unwrap();
        output
            .finish(&[Patch {
                position: 2,
                bytes: b"ab".to_vec(),
            }])
            .unwrap();
        assert_eq!(output.into_inner().into_inner(), b"01ab456789");
    }
}
