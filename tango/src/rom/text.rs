use std::io::Read;

use crate::rom;
use byteorder::{ByteOrder, ReadBytesExt};

#[derive(Clone, Debug)]
pub enum Part {
    String(String),
    Command { op: u8, params: Vec<u8> },
}

pub struct ParseOptions<'a> {
    pub charset: &'a [&'a str],
    pub extension_op: u8,
    pub eof_op: u8,
    pub newline_op: u8,
    pub commands: std::collections::HashMap<u8, usize>,
}

pub fn parse(mut buf: &[u8], options: &ParseOptions) -> Result<Vec<Part>, std::io::Error> {
    let mut parts = vec![];
    let mut out_buf = String::new();
    while !buf.is_empty() {
        let op = buf.read_u8()?;

        if op == options.eof_op {
            break;
        }

        if op == options.newline_op {
            out_buf.push('\n');
            continue;
        }

        if let Some(len) = options.commands.get(&op) {
            if !out_buf.is_empty() {
                let mut next_buf = String::new();
                std::mem::swap(&mut out_buf, &mut next_buf);
                parts.push(Part::String(next_buf));
            }

            let mut params = vec![0u8; *len];
            buf.read_exact(&mut params)?;
            parts.push(Part::Command { op, params });
        } else {
            let mut c = op as usize;
            if op == options.extension_op {
                c += buf.read_u8()? as usize;
            }
            out_buf.push_str(options.charset.get(c).unwrap_or(&"�"));
        }
    }
    if !out_buf.is_empty() {
        parts.push(Part::String(out_buf));
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
            &buf.get(offset..next_offset).ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "could not read entry",
            ))?
        } else {
            &buf.get(offset..).ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "could not read entry",
            ))?
        },
        &options,
    )
}

pub fn parse_modcard56_effect(
    parts: Vec<Part>,
    print_var_command: u8,
) -> rom::Modcard56EffectTemplate {
    parts
        .into_iter()
        .flat_map(|part| match part {
            Part::String(s) => vec![rom::Modcard56EffectTemplatePart::String(s)],
            Part::Command { op, params } if op == print_var_command => {
                vec![rom::Modcard56EffectTemplatePart::PrintVar(
                    params[2] as usize,
                )]
            }
            _ => vec![],
        })
        .collect()
}
