use std::io::Read;

use byteorder::{ByteOrder, ReadBytesExt};

#[derive(Clone, Debug)]
pub enum Part {
    Literal(usize),
    Command { op: u8, params: Vec<u8> },
}

pub struct ParseOptions {
    extension_op: u8,
    eof_op: u8,
    commands: std::collections::HashMap<u8, usize>,
}

impl ParseOptions {
    pub fn new(extension_op: u8, eof_op: u8) -> Self {
        Self {
            extension_op,
            eof_op,
            commands: std::collections::HashMap::new(),
        }
    }

    pub fn with_command(mut self, op: u8, len: usize) -> Self {
        self.commands.insert(op, len);
        self
    }
}

pub fn parse(mut buf: &[u8], options: &ParseOptions) -> Result<Vec<Part>, std::io::Error> {
    let mut parts = vec![];
    while !buf.is_empty() {
        let op = buf.read_u8()?;

        if op == options.eof_op {
            break;
        }

        if let Some(len) = options.commands.get(&op) {
            let mut params = vec![0u8; *len];
            buf.read_exact(&mut params)?;
            parts.push(Part::Command { op, params });
        } else {
            let mut c = op as usize;
            if op == options.extension_op {
                c += buf.read_u8()? as usize;
            }
            parts.push(Part::Literal(c));
        }
    }
    Ok(parts)
}

pub fn parse_entry(
    buf: &[u8],
    i: usize,
    options: &ParseOptions,
) -> Result<Vec<Part>, std::io::Error> {
    let offset = byteorder::LittleEndian::read_u16(&buf[i * 2..(i + 1) * 2]) as usize;
    let next_offset = byteorder::LittleEndian::read_u16(&buf[(i + 1) * 2..(i + 2) * 2]) as usize;
    parse(
        if next_offset > offset && next_offset <= buf.len() {
            &buf[offset..next_offset]
        } else {
            &buf[offset..]
        },
        &options,
    )
}
