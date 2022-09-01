use std::io::{Read, Write};

use byteorder::ReadBytesExt;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("invalid format")]
    InvalidHeader,

    #[error("unexpected patch eof")]
    UnexpectedPatchEOF,

    #[error("unexpected source eof")]
    UnexpectedSourceEOF,

    #[error("unexpected target eof")]
    UnexpectedTargetEOF,

    #[error("invalid length, expected {0}")]
    InvalidLength(usize),

    #[error("invalid source checksum, expected {0}")]
    InvalidSourceChecksum(u32),

    #[error("invalid target checksum, expected {0}")]
    InvalidTargetChecksum(u32),

    #[error("invalid patch checksum, expected {0}")]
    InvalidPatchChecksum(u32),
}

fn read_vlq(buf: &mut impl std::io::Read) -> Option<usize> {
    // uint64 data = 0, shift = 1;
    let mut data = 0;
    let mut shift = 1;

    // while(true) {
    loop {
        // uint8 x = read();
        let x = buf.read_u8().ok()? as usize;
        // data += (x & 0x7f) * shift;
        data += (x & 0x7f) * shift;
        // if(x & 0x80) break;
        if x & 0x80 != 0 {
            break;
        }
        // shift <<= 7;
        shift <<= 7;
        // data += shift;
        data += shift;
    }
    // }
    return Some(data);
    // return data;
}

fn read_signed_vlq(buf: &mut impl std::io::Read) -> Option<isize> {
    let v = read_vlq(buf)?;
    Some((if (v & 1) != 0 { -1 } else { 1 }) * (v >> 1) as isize)
}

pub fn apply(src: &[u8], mut patch: &[u8]) -> Result<Vec<u8>, Error> {
    let actual_patch_checksum = crc32fast::hash(&patch[..patch.len() - 4]);

    // string "BPS1"
    let mut header = [0u8; 4];
    patch
        .read_exact(&mut header)
        .map_err(|_| Error::UnexpectedPatchEOF)?;
    if &header != b"BPS1" {
        return Err(Error::InvalidHeader);
    }

    // (trailer)
    let mut footer = &patch[patch.len() - 12..];

    // uint32 source-checksum
    let source_checksum = footer.read_u32::<byteorder::LittleEndian>().unwrap();
    if source_checksum != crc32fast::hash(src) {
        return Err(Error::InvalidSourceChecksum(source_checksum));
    }

    // uint32 target-checksum
    let target_checksum = footer.read_u32::<byteorder::LittleEndian>().unwrap();

    // uint32 patch-checksum
    let patch_checksum = footer.read_u32::<byteorder::LittleEndian>().unwrap();
    if patch_checksum != actual_patch_checksum {
        return Err(Error::InvalidPatchChecksum(patch_checksum));
    }

    // number source-size
    let source_size = read_vlq(&mut patch).ok_or(Error::UnexpectedPatchEOF)?;
    if source_size != src.len() {
        return Err(Error::InvalidLength(source_size));
    }

    // number target-size
    let target_size = read_vlq(&mut patch).ok_or(Error::UnexpectedPatchEOF)?;
    let mut tgt = vec![0u8; target_size];

    // number metadata-size
    let metadata_size = read_vlq(&mut patch).ok_or(Error::UnexpectedPatchEOF)?;

    // string metadata[metadata-size]
    patch = patch
        .get(metadata_size..patch.len() - 12)
        .ok_or(Error::UnexpectedPatchEOF)?;

    let mut tgt_offset = 0;
    let mut src_rel_offset = 0;
    let mut tgt_rel_offset = 0;

    // repeat {
    while !patch.is_empty() {
        let instr = read_vlq(&mut patch).ok_or(Error::UnexpectedPatchEOF)?;
        let action = (instr & 3) as u8;
        let len = (instr >> 2) + 1;
        match action {
            0 => {
                // source read
                tgt.get_mut(tgt_offset..tgt_offset + len)
                    .ok_or(Error::UnexpectedTargetEOF)?
                    .copy_from_slice(
                        src.get(tgt_offset..tgt_offset + len)
                            .ok_or(Error::UnexpectedPatchEOF)?,
                    );
            }
            1 => {
                // target read
                let mut buf = vec![0u8; len];
                patch
                    .read_exact(&mut buf)
                    .map_err(|_| Error::UnexpectedPatchEOF)?;
                tgt.get_mut(tgt_offset..tgt_offset + len)
                    .ok_or(Error::UnexpectedTargetEOF)?
                    .copy_from_slice(&buf);
            }
            2 => {
                // source copy
                src_rel_offset = (src_rel_offset as isize
                    + read_signed_vlq(&mut patch).ok_or(Error::UnexpectedPatchEOF)?)
                    as usize;
                tgt.get_mut(tgt_offset..tgt_offset + len)
                    .ok_or(Error::UnexpectedTargetEOF)?
                    .copy_from_slice(
                        src.get(src_rel_offset..src_rel_offset + len)
                            .ok_or(Error::UnexpectedSourceEOF)?,
                    );
                src_rel_offset += len;
            }
            3 => {
                // target copy
                tgt_rel_offset = (tgt_rel_offset as isize
                    + read_signed_vlq(&mut patch).ok_or(Error::UnexpectedPatchEOF)?)
                    as usize;

                // This has to be done byte by byte, because newer output bytes may refer to older ones.
                for i in tgt_offset..tgt_offset + len {
                    tgt[i] = tgt[tgt_rel_offset];
                    tgt_rel_offset += 1;
                }
            }
            _ => {
                unreachable!();
            }
        }
        tgt_offset += len;
    }
    // }

    if target_checksum != crc32fast::hash(&tgt) {
        return Err(Error::InvalidTargetChecksum(target_checksum));
    }

    Ok(tgt)
}
