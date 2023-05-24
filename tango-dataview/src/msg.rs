use std::io::Read;

use itertools::Itertools;

#[derive(Debug, PartialEq)]
pub enum Chunk<'a> {
    Text(String),
    Command { op: &'a [u8], params: &'a [u8] },
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

    pub fn parse<'a>(&'a self, mut buf: &'a [u8]) -> Result<Vec<Chunk<'a>>, std::io::Error> {
        let mut chunks = vec![];

        while !buf.is_empty() {
            let (prefix, rule) = if let Some(rule) = self.rules.get_longest_common_prefix(buf) {
                rule
            } else {
                let mut stray_byte = [0u8; 1];
                buf.read(&mut stray_byte).unwrap();
                if self.ignore_unknown {
                    continue;
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("unknown byte: {:02x}", stray_byte[0]),
                    ));
                }
            };

            buf = &buf[prefix.len()..];
            chunks.push(match rule {
                Rule::Text(t) => Chunk::Text(t.clone()),
                Rule::Command(len) => {
                    if buf.len() < *len {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            format!("not enough bytes for command: {} < {}", buf.len(), *len),
                        ));
                    }
                    let (params, rest) = buf.split_at(*len);
                    buf = rest;
                    Chunk::Command { op: prefix, params }
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

pub fn get_entry(buf: &[u8], i: usize) -> Option<&[u8]> {
    let (offset, next_offset) = match bytemuck::cast_slice::<_, u16>(&buf[i * 2..][..4]) {
        &[offset, next_offset] => (offset, next_offset),
        _ => unreachable!(),
    };

    let offset = offset as usize;
    let mut next_offset = next_offset as usize;
    if next_offset < offset || next_offset > buf.len() {
        next_offset = buf.len();
    }

    buf.get(offset..next_offset)
}
