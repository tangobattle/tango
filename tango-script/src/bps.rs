//! Bounded BPS decoding for untrusted package assets. All lengths, offsets and
//! varints are checked before allocating or slicing, including on wasm32.
use crate::{invalid, Result};

fn number(input: &mut &[u8]) -> Result<usize> {
    let mut value = 0usize;
    let mut shift = 1usize;
    loop {
        let (&byte, rest) = input.split_first().ok_or_else(|| invalid("truncated BPS number"))?;
        *input = rest;
        value = shift
            .checked_mul(usize::from(byte & 127))
            .and_then(|n| value.checked_add(n))
            .ok_or_else(|| invalid("BPS number overflow"))?;
        if byte & 128 != 0 {
            return Ok(value);
        }
        shift = shift.checked_mul(128).ok_or_else(|| invalid("BPS number overflow"))?;
        value = value.checked_add(shift).ok_or_else(|| invalid("BPS number overflow"))?;
    }
}

fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
    if len > input.len() {
        return Err(invalid("truncated BPS data"));
    }
    let (bytes, rest) = input.split_at(len);
    *input = rest;
    Ok(bytes)
}

fn relative(input: &mut &[u8], offset: usize) -> Result<usize> {
    let n = number(input)?;
    if n & 1 == 0 {
        offset.checked_add(n >> 1)
    } else {
        offset.checked_sub(n >> 1)
    }
    .ok_or_else(|| invalid("BPS relative offset out of bounds"))
}

pub(crate) fn apply(source: &[u8], patch: &[u8], mut charge: impl FnMut(usize) -> Result<()>) -> Result<Vec<u8>> {
    let limit = crate::runtime::MAX_BUFFER;
    if source.len() > limit || patch.len() > limit {
        return Err(invalid("BPS buffer exceeds 32 MiB"));
    }
    charge(source.len() + patch.len())?;
    // Header, three single-byte numbers and three CRC32s are the minimum.
    if patch.len() < 19 || &patch[..4] != b"BPS1" {
        return Err(invalid("invalid BPS header"));
    }
    let footer = patch.len() - 12;
    let crc = |offset| u32::from_le_bytes(patch[offset..offset + 4].try_into().unwrap());
    if crc32fast::hash(&patch[..patch.len() - 4]) != crc(footer + 8) {
        return Err(invalid("BPS patch checksum mismatch"));
    }
    if crc32fast::hash(source) != crc(footer) {
        return Err(invalid("BPS source checksum mismatch"));
    }
    let mut input = &patch[4..footer];
    let source_len = number(&mut input)?;
    let target_len = number(&mut input)?;
    if source_len != source.len() || target_len > limit {
        return Err(invalid("BPS source or target length out of bounds"));
    }
    let metadata_len = number(&mut input)?;
    take(&mut input, metadata_len)?;
    charge(target_len)?;
    let mut output = Vec::with_capacity(target_len);
    let mut source_offset = 0usize;
    let mut target_offset = 0usize;
    while !input.is_empty() {
        let instruction = number(&mut input)?;
        let len = (instruction >> 2) + 1;
        if len > target_len - output.len() {
            return Err(invalid("BPS instruction exceeds target"));
        }
        match instruction & 3 {
            0 => {
                let end = output.len() + len;
                let bytes = source
                    .get(output.len()..end)
                    .ok_or_else(|| invalid("BPS source read out of bounds"))?;
                output.extend_from_slice(bytes);
            }
            1 => output.extend_from_slice(take(&mut input, len)?),
            2 => {
                source_offset = relative(&mut input, source_offset)?;
                let end = source_offset
                    .checked_add(len)
                    .ok_or_else(|| invalid("BPS source offset overflow"))?;
                let bytes = source
                    .get(source_offset..end)
                    .ok_or_else(|| invalid("BPS source copy out of bounds"))?;
                output.extend_from_slice(bytes);
                source_offset = end;
            }
            3 => {
                target_offset = relative(&mut input, target_offset)?;
                if target_offset >= output.len() {
                    return Err(invalid("BPS target copy must refer to existing output"));
                }
                // Overlap is intentional: later bytes can copy earlier bytes
                // of this same instruction (the format's run-length encoding).
                for _ in 0..len {
                    output.push(output[target_offset]);
                    target_offset += 1;
                }
            }
            _ => unreachable!(),
        }
    }
    if output.len() != target_len || crc32fast::hash(&output) != crc(footer + 4) {
        return Err(invalid("BPS target length or checksum mismatch"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(body: &[u8], source: &[u8], target: &[u8]) -> Vec<u8> {
        let mut bytes = b"BPS1".to_vec();
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&crc32fast::hash(source).to_le_bytes());
        bytes.extend_from_slice(&crc32fast::hash(target).to_le_bytes());
        bytes.extend_from_slice(&crc32fast::hash(&bytes).to_le_bytes());
        bytes
    }

    #[test]
    fn all_actions_and_overlapping_copies() {
        // SourceRead(2), TargetRead(1,'!'), SourceCopy(2,+2),
        // TargetCopy(4,+3), SourceCopy(1,-3).
        let bytes = patch(
            &[0x84, 0x8a, 0x80, 0x84, 0x81, b'!', 0x86, 0x84, 0x8f, 0x86, 0x82, 0x87],
            b"abcd",
            b"ab!cdcdcdb",
        );
        assert_eq!(apply(b"abcd", &bytes, |_| Ok(())).unwrap(), b"ab!cdcdcdb");
        assert!(apply(b"abce", &bytes, |_| Ok(())).is_err());
        assert!(apply(b"abcd", &bytes, |_| Err(invalid("budget"))).is_err());
    }

    #[test]
    fn malformed_assets_return_errors_without_panics() {
        for bytes in [
            vec![],
            b"BPS1".to_vec(),
            patch(&[0x80, 0x80, 0xff], b"", b""),
            patch(&[0; 20], b"", b""),
            patch(&[0x80, 0x81, 0x80, 0x83, 0x80], b"", b"x"),
            patch(&[0x80, 0x81, 0x80, 0x82, 0x81], b"", b"x"),
            patch(&[0x80, 0x80, 0x80, 0x81, b'x'], b"", b""),
        ] {
            assert!(apply(b"", &bytes, |_| Ok(())).is_err());
        }
    }
}
