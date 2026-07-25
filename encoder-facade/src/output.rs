//! Writing a session's bytes to a seekable file.
//!
//! A convenience for native callers: a [`Session`](crate::Session)
//! deliberately does no I/O, and this is the whole of what a synchronous
//! caller needs to do with what it produces — append the bytes as they
//! come, then apply the closing [`Fixup`]s.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::mux::Fixup;

/// How much media is moved at a time when a fixup inserts bytes.
const SHIFT_CHUNK: usize = 1 << 20;

pub struct Output<W> {
    inner: W,
}

impl<W: Write + Seek> Output<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    /// Append container bytes from [`Session::take_output`](crate::Session::take_output).
    pub fn append(&mut self, bytes: &[u8]) -> crate::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.inner.write_all(bytes)?;
        Ok(())
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Read + Write + Seek> Output<W> {
    /// Apply the fixups from [`Session::poll_finish`](crate::Session::poll_finish),
    /// in order, and flush. Append any final bytes first — the fixups
    /// refer to positions in the complete file.
    pub fn finish(&mut self, fixups: &[Fixup]) -> crate::Result<()> {
        for fixup in fixups {
            match fixup {
                Fixup::Overwrite { position, bytes } => {
                    self.inner.seek(SeekFrom::Start(*position))?;
                    self.inner.write_all(bytes)?;
                }
                Fixup::Insert { position, bytes } => self.insert(*position, bytes)?,
            }
        }
        self.inner.flush()?;
        Ok(())
    }

    /// Make room at `position` and put `bytes` there.
    ///
    /// The tail is moved a chunk at a time from the end backwards, so a
    /// region never overwrites part of itself before it has been read.
    /// Costs one pass over everything past `position`.
    fn insert(&mut self, position: u64, bytes: &[u8]) -> crate::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        // Ask the stream how long it is rather than trusting a running
        // count: an output can arrive with content already in it.
        let end = self.inner.seek(SeekFrom::End(0))?;
        if position > end {
            return Err(crate::Error::internal(format!(
                "an insert at {position} is past the end of a {end}-byte output"
            )));
        }
        let shift = bytes.len() as u64;
        let mut remaining = end - position;
        let mut buf = vec![0u8; SHIFT_CHUNK.min(remaining.max(1) as usize)];
        while remaining > 0 {
            let take = buf.len().min(remaining as usize);
            let from = position + remaining - take as u64;
            self.inner.seek(SeekFrom::Start(from))?;
            self.inner.read_exact(&mut buf[..take])?;
            self.inner.seek(SeekFrom::Start(from + shift))?;
            self.inner.write_all(&buf[..take])?;
            remaining -= take as u64;
        }
        self.inner.seek(SeekFrom::Start(position))?;
        self.inner.write_all(bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> Output<std::io::Cursor<Vec<u8>>> {
        Output::new(std::io::Cursor::new(Vec::new()))
    }

    #[test]
    fn overwrites_land_where_they_are_addressed() {
        let mut output = output();
        output.append(b"0123456789").unwrap();
        output
            .finish(&[Fixup::Overwrite {
                position: 2,
                bytes: b"ab".to_vec(),
            }])
            .unwrap();
        assert_eq!(output.into_inner().into_inner(), b"01ab456789");
    }

    #[test]
    fn an_insert_moves_the_tail_along() {
        let mut output = output();
        output.append(b"HEADTAIL").unwrap();
        output
            .finish(&[Fixup::Insert {
                position: 4,
                bytes: b"MID".to_vec(),
            }])
            .unwrap();
        assert_eq!(output.into_inner().into_inner(), b"HEADMIDTAIL");
    }

    /// The tail is moved in chunks, so a tail longer than one chunk is
    /// the case that catches a copy going the wrong way.
    #[test]
    fn a_long_tail_survives_being_moved() {
        let mut output = output();
        let tail: Vec<u8> = (0..(SHIFT_CHUNK * 2 + 12345)).map(|i| (i % 251) as u8).collect();
        output.append(b"HEAD").unwrap();
        output.append(&tail).unwrap();
        output
            .finish(&[Fixup::Insert {
                position: 4,
                bytes: vec![0xAA; 4096],
            }])
            .unwrap();
        let file = output.into_inner().into_inner();
        assert_eq!(&file[..4], b"HEAD");
        assert_eq!(&file[4..4100], &[0xAA; 4096]);
        assert_eq!(&file[4100..], &tail[..], "every byte of the tail must survive");
    }

    #[test]
    fn fixups_apply_in_order() {
        let mut output = output();
        output.append(b"0123456789").unwrap();
        output
            .finish(&[
                // Addressed in the pre-insert layout...
                Fixup::Overwrite {
                    position: 8,
                    bytes: b"xy".to_vec(),
                },
                // ...which this then moves along.
                Fixup::Insert {
                    position: 0,
                    bytes: b"--".to_vec(),
                },
            ])
            .unwrap();
        assert_eq!(output.into_inner().into_inner(), b"--01234567xy");
    }

    #[test]
    fn an_insert_past_the_end_is_refused() {
        let mut output = output();
        output.append(b"0123").unwrap();
        assert!(output
            .finish(&[Fixup::Insert {
                position: 99,
                bytes: b"x".to_vec(),
            }])
            .is_err());
    }
}
