use std::io::Read;

use byteorder::ReadBytesExt;

#[derive(thiserror::Error, Debug)]
pub enum DecodeError {
    #[error("invalid format")]
    InvalidHeader,

    #[error("unexpected patch eof")]
    UnexpectedPatchEOF,

    #[error("invalid patch checksum, expected {0}")]
    InvalidPatchChecksum(u32),
}

#[derive(thiserror::Error, Debug)]
pub enum ApplyError {
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

    #[error("invalid action, got {0}")]
    InvalidAction(u8),
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

pub struct Patch<'a> {
    pub source_checksum: u32,
    pub target_checksum: u32,
    pub patch_checksum: u32,
    pub source_size: usize,
    pub target_size: usize,
    pub metadata: &'a [u8],
    pub body: &'a [u8],
}

impl<'a> Patch<'a> {
    pub fn decode(mut patch: &'a [u8]) -> Result<Self, DecodeError> {
        let actual_patch_checksum = crc32fast::hash(&patch[..patch.len() - 4]);

        // string "BPS1"
        let mut header = [0u8; 4];
        patch
            .read_exact(&mut header)
            .map_err(|_| DecodeError::UnexpectedPatchEOF)?;
        if &header != b"BPS1" {
            return Err(DecodeError::InvalidHeader);
        }

        // (trailer)
        let mut footer = &patch[patch.len() - 12..];

        // uint32 source-checksum
        let source_checksum = footer.read_u32::<byteorder::LittleEndian>().unwrap();

        // uint32 target-checksum
        let target_checksum = footer.read_u32::<byteorder::LittleEndian>().unwrap();

        // uint32 patch-checksum
        let patch_checksum = footer.read_u32::<byteorder::LittleEndian>().unwrap();
        if patch_checksum != actual_patch_checksum {
            return Err(DecodeError::InvalidPatchChecksum(patch_checksum));
        }

        // number source-size
        let source_size = read_vlq(&mut patch).ok_or(DecodeError::UnexpectedPatchEOF)?;

        // number target-size
        let target_size = read_vlq(&mut patch).ok_or(DecodeError::UnexpectedPatchEOF)?;

        // number metadata-size
        let metadata_size = read_vlq(&mut patch).ok_or(DecodeError::UnexpectedPatchEOF)?;

        // string metadata[metadata-size]
        let metadata = &patch[..metadata_size];

        let body = patch
            .get(metadata_size..patch.len() - 12)
            .ok_or(DecodeError::UnexpectedPatchEOF)?;

        Ok(Self {
            source_checksum,
            target_checksum,
            patch_checksum,
            source_size,
            target_size,
            metadata,
            body,
        })
    }

    pub fn apply(&self, src: &[u8]) -> Result<Vec<u8>, ApplyError> {
        if self.source_checksum != crc32fast::hash(src) {
            return Err(ApplyError::InvalidSourceChecksum(self.source_checksum));
        }

        if self.source_size != src.len() {
            return Err(ApplyError::InvalidLength(self.source_size));
        }

        let mut tgt = vec![0u8; self.target_size];

        let mut tgt_offset = 0;
        let mut src_rel_offset = 0;
        let mut tgt_rel_offset = 0;

        // repeat {
        let mut r = self.body;
        while !r.is_empty() {
            let instr = read_vlq(&mut r).ok_or(ApplyError::UnexpectedPatchEOF)?;
            let action = (instr & 3) as u8;
            let len = (instr >> 2) + 1;
            match action {
                0 => {
                    // source read
                    tgt.get_mut(tgt_offset..tgt_offset + len)
                        .ok_or(ApplyError::UnexpectedTargetEOF)?
                        .copy_from_slice(
                            src.get(tgt_offset..tgt_offset + len)
                                .ok_or(ApplyError::UnexpectedPatchEOF)?,
                        );
                }
                1 => {
                    // target read
                    let mut buf = vec![0u8; len];
                    r.read_exact(&mut buf).map_err(|_| ApplyError::UnexpectedPatchEOF)?;
                    tgt.get_mut(tgt_offset..tgt_offset + len)
                        .ok_or(ApplyError::UnexpectedTargetEOF)?
                        .copy_from_slice(&buf);
                }
                2 => {
                    // source copy
                    src_rel_offset = (src_rel_offset as isize
                        + read_signed_vlq(&mut r).ok_or(ApplyError::UnexpectedPatchEOF)?)
                        as usize;
                    tgt.get_mut(tgt_offset..tgt_offset + len)
                        .ok_or(ApplyError::UnexpectedTargetEOF)?
                        .copy_from_slice(
                            src.get(src_rel_offset..src_rel_offset + len)
                                .ok_or(ApplyError::UnexpectedSourceEOF)?,
                        );
                    src_rel_offset += len;
                }
                3 => {
                    // target copy
                    tgt_rel_offset = (tgt_rel_offset as isize
                        + read_signed_vlq(&mut r).ok_or(ApplyError::UnexpectedPatchEOF)?)
                        as usize;

                    // This has to be done byte by byte, because newer output bytes may refer to older ones.
                    for i in tgt_offset..tgt_offset + len {
                        tgt[i] = tgt[tgt_rel_offset];
                        tgt_rel_offset += 1;
                    }
                }
                action => {
                    return Err(ApplyError::InvalidAction(action));
                }
            }
            tgt_offset += len;
        }
        // }

        if self.target_checksum != crc32fast::hash(&tgt) {
            return Err(ApplyError::InvalidTargetChecksum(self.target_checksum));
        }

        Ok(tgt)
    }
}
