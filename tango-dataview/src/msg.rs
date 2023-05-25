use itertools::Itertools;

#[derive(Debug, PartialEq)]
pub enum Chunk<Command> {
    Text(String),
    Command(Command),
}

enum Rule<Command> {
    PushText(String),
    ReadCommand(fn(&[u8]) -> Option<(Command, &[u8])>),
    Skip(usize),
    Error,
    Stop,
}

pub trait CommandBody<Command>
where
    Self: bytemuck::AnyBitPattern,
{
    fn into_wrapped(self) -> Command;
}

pub struct ParserBuilder<Command> {
    rules: patricia_tree::PatriciaMap<Rule<Command>>,
    fallthrough_rule: Rule<Command>,
}

pub enum FallthroughBehavior {
    Skip,
    Error,
    Text(String),
}

impl<Command> ParserBuilder<Command> {
    pub fn with_fallthrough_rule(mut self, behavior: FallthroughBehavior) -> Self {
        self.fallthrough_rule = match behavior {
            FallthroughBehavior::Skip => Rule::Skip(0),
            FallthroughBehavior::Error => Rule::Error,
            FallthroughBehavior::Text(s) => Rule::PushText(s),
        };
        self
    }

    fn add_rule(mut self, pat: &[u8], rule: Rule<Command>) -> Self {
        self.rules.insert(Box::from(pat), rule);
        self
    }

    pub fn add_stop_rule(self, pat: &[u8]) -> Self {
        self.add_rule(pat, Rule::Stop)
    }

    pub fn add_skip_rule(self, pat: &[u8], n: usize) -> Self {
        self.add_rule(pat, Rule::Skip(n))
    }

    pub fn add_command_rule<T>(self, pat: &[u8]) -> Self
    where
        T: CommandBody<Command>,
    {
        self.add_rule(
            pat,
            Rule::ReadCommand(|buf| {
                let len = std::mem::size_of::<T>();
                if buf.len() < len {
                    return None;
                }
                let (params, rest) = buf.split_at(len);
                let body = bytemuck::pod_read_unaligned::<T>(params);
                Some((body.into_wrapped(), rest))
            }),
        )
    }

    pub fn add_text_rule(self, pat: &[u8], s: &str) -> Self {
        self.add_rule(pat, Rule::PushText(s.to_string()))
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

    pub fn build(self) -> Parser<Command> {
        Parser {
            rules: self.rules,
            fallthrough_rule: self.fallthrough_rule,
        }
    }
}

pub struct Parser<Command> {
    rules: patricia_tree::PatriciaMap<Rule<Command>>,
    fallthrough_rule: Rule<Command>,
}

fn coalesce<Command>(chunks: Vec<Chunk<Command>>) -> Vec<Chunk<Command>> {
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

impl<Command> Parser<Command> {
    pub fn builder() -> ParserBuilder<Command> {
        ParserBuilder {
            rules: patricia_tree::PatriciaMap::new(),
            fallthrough_rule: Rule::Skip(0),
        }
    }

    pub fn parse(&self, mut buf: &[u8]) -> Result<Vec<Chunk<Command>>, std::io::Error> {
        let mut chunks = vec![];

        while !buf.is_empty() {
            let (prefix, rule) = self
                .rules
                .get_longest_common_prefix(buf)
                .unwrap_or_else(|| (&buf[..1], &self.fallthrough_rule));

            buf = &buf[prefix.len()..];
            chunks.push(match rule {
                Rule::PushText(t) => Chunk::Text(t.clone()),
                Rule::ReadCommand(read) => {
                    let (wrapped, rest) = if let Some((wrapped, rest)) = read(buf) {
                        (wrapped, rest)
                    } else {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            format!("not enough bytes for command, {} remaining", buf.len()),
                        ));
                    };
                    buf = rest;
                    Chunk::Command(wrapped)
                }
                Rule::Stop => {
                    break;
                }
                Rule::Skip(n) => {
                    buf = &buf[*n..];
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
