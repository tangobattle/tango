use std::io::Read;

use crate::rom;
use byteorder::{ByteOrder, ReadBytesExt};
use itertools::Itertools;

#[derive(Debug, PartialEq)]
pub enum Chunk {
    Text(String),
    Command { op: Vec<u8>, params: Vec<u8> },
}

enum Rule {
    Text(String),
    Command(usize),
}

pub struct ParserBuilder {
    rules: patricia_tree::PatriciaMap<Rule>,
    eof: &'static [u8],
    ignore_unknown: bool,
}

impl ParserBuilder {
    pub fn add_command_rule(mut self, pat: &[u8], len: usize) -> Self {
        self.rules.insert(Box::from(pat), Rule::Command(len));
        self
    }

    pub fn add_text_rule(mut self, pat: &[u8], s: &str) -> Self {
        self.rules.insert(Box::from(pat), Rule::Text(s.to_string()));
        self
    }

    pub fn add_charset_rules(self, charset: &[String], extension_op_base: u8) -> Self {
        let mut this = self;
        for (i, c) in charset.iter().enumerate() {
            if i < extension_op_base as usize {
                this = this.add_text_rule(&[i as u8][..], c);
            } else {
                let offset = i - extension_op_base as usize;
                this = this.add_text_rule(
                    &[extension_op_base + (offset / 0x100) as u8, (offset % 0x100) as u8][..],
                    c,
                );
            }
        }
        this
    }

    pub fn build(self) -> Parser {
        Parser {
            rules: self.rules,
            eof: self.eof,
            ignore_unknown: self.ignore_unknown,
        }
    }
}

pub struct Parser {
    rules: patricia_tree::PatriciaMap<Rule>,
    eof: &'static [u8],
    ignore_unknown: bool,
}

impl Parser {
    pub fn builder(ignore_unknown: bool, eof: &'static [u8]) -> ParserBuilder {
        ParserBuilder {
            rules: patricia_tree::PatriciaMap::new(),
            eof,
            ignore_unknown,
        }
    }

    pub fn parse(&self, mut buf: &[u8]) -> Result<Vec<Chunk>, std::io::Error> {
        let mut chunks = vec![];

        while !buf.is_empty() {
            if buf.starts_with(self.eof) {
                break;
            }

            let (prefix, rule) = if let Some(rule) = self.rules.get_longest_common_prefix(buf) {
                rule
            } else {
                let stray_byte = buf.read_u8().unwrap();
                if self.ignore_unknown {
                    continue;
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("unknown byte: {:02x}", stray_byte),
                    ));
                }
            };

            buf = &buf[prefix.len()..];
            chunks.push(match rule {
                Rule::Text(t) => Chunk::Text(t.clone()),
                Rule::Command(len) => {
                    let mut params = vec![0; *len];
                    buf.read(&mut params)?;
                    Chunk::Command {
                        op: prefix.to_vec(),
                        params,
                    }
                }
            });
        }

        // Coalesce text chunks together.
        Ok(chunks
            .into_iter()
            .group_by(|chunk| matches!(chunk, Chunk::Text(_)))
            .into_iter()
            .flat_map(|(is_text, g)| {
                if !is_text {
                    g.into_iter().collect::<Vec<_>>()
                } else {
                    vec![Chunk::Text(
                        g.into_iter()
                            .map(|chunk| match chunk {
                                Chunk::Text(t) => t,
                                Chunk::Command { .. } => unreachable!(),
                            })
                            .collect::<String>(),
                    )]
                }
            })
            .collect::<Vec<_>>())
    }
}

#[derive(Clone, Debug)]
pub enum Part {
    String(String),
    Command { op: u8, params: Vec<u8> },
}

pub struct ParseOptions {
    pub charset: Vec<String>,
    pub extension_ops: Vec<u8>,
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
            if options.extension_ops.contains(&op) {
                c += buf.read_u8()? as usize;
            }
            out_buf.push_str(&options.charset.get(c).cloned().unwrap_or_else(|| "�".to_string()));
        }
    }
    if !out_buf.is_empty() {
        parts.push(Part::String(out_buf));
    }
    Ok(parts)
}

pub fn get_entry(buf: &[u8], i: usize) -> Option<&[u8]> {
    let offset = byteorder::LittleEndian::read_u16(&buf[i * 2..(i + 1) * 2]) as usize;
    let next_offset = byteorder::LittleEndian::read_u16(&buf[(i + 1) * 2..(i + 2) * 2]) as usize;

    if next_offset > offset && next_offset <= buf.len() {
        buf.get(offset..next_offset)
    } else {
        buf.get(offset..)
    }
}

pub fn parse_entry(buf: &[u8], i: usize, options: &ParseOptions) -> Result<Vec<Part>, std::io::Error> {
    parse(
        get_entry(buf, i).ok_or(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "could not read entry",
        ))?,
        &options,
    )
}

pub fn parse_patch_card56_effect(parts: Vec<Part>, print_var_command: u8) -> rom::PatchCard56EffectTemplate {
    parts
        .into_iter()
        .flat_map(|part| match part {
            Part::String(s) => vec![rom::PatchCard56EffectTemplatePart::String(s)],
            Part::Command { op, params } if op == print_var_command => {
                vec![rom::PatchCard56EffectTemplatePart::PrintVar(params[2] as usize)]
            }
            _ => vec![],
        })
        .collect()
}
