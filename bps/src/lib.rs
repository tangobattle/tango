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
pub enum InstructionDecodeError {
    #[error("unexpected eof")]
    UnexpectedEOF,

    #[error("invalid action, got {0}")]
    InvalidAction(u8),
}

#[derive(thiserror::Error, Debug)]
pub enum ApplyError {
    #[error("instruction decode error: {0}")]
    InstructionDecodeError(#[from] InstructionDecodeError),

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
}

fn read_vlq(buf: &mut impl std::io::Read) -> Option<u64> {
    // uint64 data = 0, shift = 1;
    let mut data = 0u64;
    let mut shift = 1u64;

    // while(true) {
    loop {
        // uint8 x = read();
        let x = buf.read_u8().ok()?;
        // data += (x & 0x7f) * shift;
        data += ((x & 0x7f) as u64) * shift;
        // if(x & 0x80) break;
        if x & 0x80 != 0 {
            break;
        }
        // shift <<= 7;
        shift <<= 7;
        // data += shift;
        data += shift;
        // }
    }
    // return data;
    return Some(data);
}

fn read_signed_vlq(buf: &mut impl std::io::Read) -> Option<i64> {
    let v = read_vlq(buf)?;
    Some((if (v & 1) != 0 { -1 } else { 1 }) * (v >> 1) as i64)
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

pub struct Instruction<'a> {
    pub tgt_range: std::ops::Range<usize>,
    pub action: Action<'a>,
}

pub enum Action<'a> {
    SourceRead,
    TargetRead { buf: &'a [u8] },
    SourceCopy { offset: usize },
    TargetCopy { offset: usize },
}

struct InstructionIterator<'a> {
    buf: &'a [u8],
    tgt_offset: usize,
    src_rel_offset: usize,
    tgt_rel_offset: usize,
}

impl<'a> Iterator for InstructionIterator<'a> {
    type Item = Result<Instruction<'a>, InstructionDecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.is_empty() {
            return None;
        }

        Some((|| {
            let instr = read_vlq(&mut self.buf).ok_or(InstructionDecodeError::UnexpectedEOF)?;
            let action = (instr & 3) as u8;
            let len = ((instr >> 2) + 1) as usize;

            let tgt_offset = self.tgt_offset;
            self.tgt_offset += len;

            Ok(Instruction {
                action: match action {
                    0 => Action::SourceRead,
                    1 => {
                        if self.buf.len() < len {
                            return Err(InstructionDecodeError::UnexpectedEOF);
                        }

                        let (buf, rest) = self.buf.split_at(len);
                        self.buf = rest;

                        Action::TargetRead { buf }
                    }
                    2 => {
                        self.src_rel_offset = (self.src_rel_offset as isize
                            + read_signed_vlq(&mut self.buf).ok_or(InstructionDecodeError::UnexpectedEOF)? as isize)
                            as usize;
                        let src_rel_offset = self.src_rel_offset;
                        self.src_rel_offset += len;

                        Action::SourceCopy { offset: src_rel_offset }
                    }
                    3 => {
                        self.tgt_rel_offset = (self.tgt_rel_offset as isize
                            + read_signed_vlq(&mut self.buf).ok_or(InstructionDecodeError::UnexpectedEOF)? as isize)
                            as usize;
                        let tgt_rel_offset = self.tgt_rel_offset;
                        self.tgt_rel_offset += len;

                        Action::TargetCopy { offset: tgt_rel_offset }
                    }

                    action => {
                        return Err(InstructionDecodeError::InvalidAction(action));
                    }
                },
                tgt_range: tgt_offset..tgt_offset + len,
            })
        })())
    }
}

impl<'a> Patch<'a> {
    pub fn instructions(&self) -> impl Iterator<Item = Result<Instruction<'a>, InstructionDecodeError>> {
        InstructionIterator {
            buf: self.body,
            tgt_offset: 0,
            src_rel_offset: 0,
            tgt_rel_offset: 0,
        }
    }

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
        let source_size = read_vlq(&mut patch).ok_or(DecodeError::UnexpectedPatchEOF)? as usize;

        // number target-size
        let target_size = read_vlq(&mut patch).ok_or(DecodeError::UnexpectedPatchEOF)? as usize;

        // number metadata-size
        let metadata_size = read_vlq(&mut patch).ok_or(DecodeError::UnexpectedPatchEOF)? as usize;

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

        // repeat {
        for instruction in self.instructions() {
            let instruction = instruction?;
            match instruction.action {
                Action::SourceRead => {
                    tgt.get_mut(instruction.tgt_range.clone())
                        .ok_or(ApplyError::UnexpectedTargetEOF)?
                        .copy_from_slice(src.get(instruction.tgt_range).ok_or(ApplyError::UnexpectedSourceEOF)?);
                }
                Action::TargetRead { buf } => {
                    tgt.get_mut(instruction.tgt_range)
                        .ok_or(ApplyError::UnexpectedTargetEOF)?
                        .copy_from_slice(&buf);
                }
                Action::SourceCopy { offset } => {
                    let len = instruction.tgt_range.len();
                    tgt.get_mut(instruction.tgt_range)
                        .ok_or(ApplyError::UnexpectedTargetEOF)?
                        .copy_from_slice(src.get(offset..offset + len).ok_or(ApplyError::UnexpectedSourceEOF)?);
                }
                Action::TargetCopy { offset } => {
                    let len = instruction.tgt_range.len();
                    if tgt.get(instruction.tgt_range.clone()).is_none() || tgt.get(offset..offset + len).is_none() {
                        return Err(ApplyError::UnexpectedTargetEOF);
                    }
                    // This has to be done byte by byte, because newer output bytes may refer to older ones.
                    for (i, j) in std::iter::zip(instruction.tgt_range, offset..) {
                        tgt[i] = tgt[j]
                    }
                }
            }
        }
        // }

        if self.target_checksum != crc32fast::hash(&tgt) {
            return Err(ApplyError::InvalidTargetChecksum(self.target_checksum));
        }

        Ok(tgt)
    }
}
