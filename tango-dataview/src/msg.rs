use itertools::Itertools;

#[derive(Debug, PartialEq)]
pub enum Chunk<'a> {
    Text(String),
    Command { op: &'a [u8], params: &'a [u8] },
}

enum Rule {
    PushText(String),
    PushCommand(usize),
    Skip,
    Error,
    Stop,
}

pub struct ParserBuilder {
    rules: patricia_tree::PatriciaMap<Rule>,
    fallthrough_rule: Rule,
}

impl ParserBuilder {
    pub fn with_error_on_fallthrough(mut self, error_on_fallthrough: bool) -> Self {
        self.fallthrough_rule = if error_on_fallthrough { Rule::Error } else { Rule::Skip };
        self
    }

    pub fn add_stop_rule(mut self, pat: &[u8]) -> Self {
        self.rules.insert(Box::from(pat), Rule::Stop);
        self
    }

    pub fn add_command_rule(mut self, pat: &[u8], len: usize) -> Self {
        self.rules.insert(Box::from(pat), Rule::PushCommand(len));
        self
    }

    pub fn add_text_rule(mut self, pat: &[u8], s: &str) -> Self {
        self.rules.insert(Box::from(pat), Rule::PushText(s.to_string()));
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
            fallthrough_rule: self.fallthrough_rule,
        }
    }
}

pub struct Parser {
    rules: patricia_tree::PatriciaMap<Rule>,
    fallthrough_rule: Rule,
}

fn coalesce(chunks: Vec<Chunk>) -> Vec<Chunk> {
    chunks
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
        .collect::<Vec<_>>()
}

impl Parser {
    pub fn builder() -> ParserBuilder {
        ParserBuilder {
            rules: patricia_tree::PatriciaMap::new(),
            fallthrough_rule: Rule::Skip,
        }
    }

    pub fn parse<'a>(&'a self, mut buf: &'a [u8]) -> Result<Vec<Chunk<'a>>, std::io::Error> {
        let mut chunks = vec![];

        while !buf.is_empty() {
            let (prefix, rule) = self
                .rules
                .get_longest_common_prefix(buf)
                .unwrap_or_else(|| (&buf[..1], &self.fallthrough_rule));

            buf = &buf[prefix.len()..];
            chunks.push(match rule {
                Rule::PushText(t) => Chunk::Text(t.clone()),
                Rule::PushCommand(len) => {
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
                Rule::Stop => {
                    break;
                }
                Rule::Skip => {
                    continue;
                }
                Rule::Error => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("could not parse: {:02x?}", prefix),
                    ));
                }
            });
        }

        Ok(coalesce(chunks))
    }
}

pub fn get_entry(buf: &[u8], i: usize) -> Option<&[u8]> {
    let [offset, next_offset] = bytemuck::pod_read_unaligned::<[u16; 2]>(
        &buf[i * std::mem::size_of::<u16>()..][..std::mem::size_of::<[u16; 2]>()],
    );

    let offset = offset as usize;
    let mut next_offset = next_offset as usize;
    if next_offset < offset || next_offset > buf.len() {
        next_offset = buf.len();
    }

    buf.get(offset..next_offset)
}
