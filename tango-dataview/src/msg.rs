use std::io::Read;

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
    Eof,
}

pub struct ParserBuilder {
    rules: patricia_tree::PatriciaMap<Rule>,
    ignore_unknown: bool,
}

impl ParserBuilder {
    pub fn with_ignore_unknown(mut self, ignore_unknown: bool) -> Self {
        self.ignore_unknown = ignore_unknown;
        self
    }

    pub fn add_eof_rule(mut self, pat: &[u8]) -> Self {
        self.rules.insert(Box::from(pat), Rule::Eof);
        self
    }

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
            ignore_unknown: self.ignore_unknown,
        }
    }
}

pub struct Parser {
    rules: patricia_tree::PatriciaMap<Rule>,
    ignore_unknown: bool,
}

impl Parser {
    pub fn builder() -> ParserBuilder {
        ParserBuilder {
            rules: patricia_tree::PatriciaMap::new(),
            ignore_unknown: false,
        }
    }

    pub fn parse(&self, mut buf: &[u8]) -> Result<Vec<Chunk>, std::io::Error> {
        let mut chunks = vec![];

        while !buf.is_empty() {
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
                Rule::Eof => {
                    break;
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

pub fn get_mpak_entry(buf: &[u8], i: usize) -> Option<&[u8]> {
    let offset = byteorder::LittleEndian::read_u16(&buf[i * 2..(i + 1) * 2]) as usize;
    let next_offset = byteorder::LittleEndian::read_u16(&buf[(i + 1) * 2..(i + 2) * 2]) as usize;
    buf.get(offset..std::cmp::min(next_offset, buf.len()))
}
