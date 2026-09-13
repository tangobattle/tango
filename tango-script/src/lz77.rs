//! Bounded BIOS LZ77 (type 0x10) decoding. Address mapping stays in packages.
use std::io::{BufReader, Read};

use crate::Result;

/// Invalid/truncated streams return None; resource exhaustion remains an error.
/// Read only the compressed stream, never copy an entire cartridge buffer.
pub(crate) fn decode(source: &mut impl Read, mut charge: impl FnMut(usize) -> Result<()>) -> Result<Option<Vec<u8>>> {
    charge(64)?;
    let mut header = [0; 4];
    if source.read_exact(&mut header).is_err() || header[0] != 0x10 {
        return Ok(None);
    }
    let length = (u32::from_le_bytes(header) >> 8) as usize;
    // The format's 24-bit length caps each output below 16 MiB. Account for
    // compressed reads, expansion, the VM output copy and reader storage
    // before allocating, including streams that turn out to be malformed.
    charge(length * 4 + 8192)?;
    let mut source = BufReader::with_capacity(8192, source);
    let mut output = Vec::with_capacity(length);
    while output.len() < length {
        let Some([flags]) = read(&mut source) else {
            return Ok(None);
        };
        for bit in (0..8).rev() {
            if output.len() == length {
                break;
            }
            if flags & (1 << bit) == 0 {
                let Some([byte]) = read(&mut source) else {
                    return Ok(None);
                };
                output.push(byte);
            } else {
                let Some([hi, lo]) = read(&mut source) else {
                    return Ok(None);
                };
                let distance = ((usize::from(hi) & 15) << 8 | usize::from(lo)) + 1;
                if distance > output.len() {
                    return Ok(None);
                }
                let count = ((usize::from(hi) >> 4) + 3).min(length - output.len());
                for _ in 0..count {
                    // Copies may overlap, including runs with distance one.
                    output.push(output[output.len() - distance]);
                }
            }
        }
    }
    Ok(Some(output))
}

fn read<const N: usize>(source: &mut impl Read) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    source.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_overlapping_runs_final_truncation_and_invalid_streams() {
        for (source, expected) in [
            (&b"\x10\0\0\0"[..], &b""[..]),
            (&b"\x10\x03\0\0\0abc"[..], &b"abc"[..]),
            (&b"\x10\x0c\0\0\x10abc\x60\x02"[..], &b"abcabcabcabc"[..]),
            (&b"\x10\x05\0\0\x40a\xf0\0trailing"[..], &b"aaaaa"[..]),
            (&b"\x10\x09\0\0\0abcdefgh\0i"[..], &b"abcdefghi"[..]),
        ] {
            assert_eq!(decode(&mut &source[..], |_| Ok(())).unwrap().unwrap(), expected);
        }
        let complete = b"\x10\x0c\0\0\x10abc\x60\x02";
        for end in 0..complete.len() {
            assert!(decode(&mut &complete[..end], |_| Ok(())).unwrap().is_none());
        }
        for source in [
            &b"\x11\x03\0\0\0abc"[..],
            &b"\x10\x03\0\0\x80\0\0"[..],
            &b"\x10\x04\0\0\x40a\0\x01"[..],
        ] {
            assert!(decode(&mut &source[..], |_| Ok(())).unwrap().is_none());
        }
    }

    #[test]
    fn maximum_distance_and_length_are_bounded_before_allocation() {
        let mut source = vec![0x10, 3, 16, 0]; // 4096 literals, then a 3-byte copy.
        let mut expected = Vec::new();
        for i in 0..512 {
            source.push(0);
            for j in 0..8 {
                let byte = (i * 8 + j) as u8;
                source.push(byte);
                expected.push(byte);
            }
        }
        source.extend_from_slice(&[0x80, 0x0f, 0xff]);
        expected.extend_from_slice(&[0, 1, 2]);
        assert_eq!(decode(&mut source.as_slice(), |_| Ok(())).unwrap().unwrap(), expected);

        // No allocation or payload reads when the advertised output exceeds
        // the remaining operation budget, even with no payload supplied.
        let mut charges = Vec::new();
        let error = decode(&mut &b"\x10\xff\xff\xff"[..], |n| {
            charges.push(n);
            if n > 64 {
                Err(crate::invalid("quota"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "quota");
        assert_eq!(charges, [64, 0xffffff * 4 + 8192]);
    }
}
