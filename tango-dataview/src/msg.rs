use itertools::Itertools;

#[derive(Debug, PartialEq)]
pub enum Chunk<Command> {
    Text(String),
    Command(Command),
}

enum Rule<Command> {
    PushText(String),
    ReadCommand(fn(&mut dyn std::io::Read) -> Result<Command, std::io::Error>),
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

fn read_wrapped_command<Command, Body>(r: &mut dyn std::io::Read) -> Result<Command, std::io::Error>
where
    Body: CommandBody<Command>,
{
    let mut buf = vec![0; std::mem::size_of::<Body>()];
    r.read_exact(&mut buf)?;
    Ok(bytemuck::pod_read_unaligned::<Body>(&buf).into_wrapped())
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

    pub fn add_command_rule<Body>(self, pat: &[u8]) -> Self
    where
        Body: CommandBody<Command>,
    {
        self.add_rule(pat, Rule::ReadCommand(|buf| read_wrapped_command::<Command, Body>(buf)))
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
                            Chunk::Command(_) => unreachable!(),
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
                Rule::ReadCommand(read) => Chunk::Command(read(&mut buf)?),
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
    let num_entries = (bytemuck::pod_read_unaligned::<u16>(&buf[..std::mem::size_of::<u16>()]) / 2) as usize;

    let offset =
        bytemuck::pod_read_unaligned::<u16>(&buf[i * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()])
            as usize;

    let next_offset = if i < num_entries - 1 {
        bytemuck::pod_read_unaligned::<u16>(&buf[(i + 1) * std::mem::size_of::<u16>()..][..std::mem::size_of::<u16>()])
            as usize
    } else {
        // While msgs have an entry offset table in the header, for msgs that are not LZ77 compressed and just in raw memory we don't know how long the last entry is.
        // As such, we have to assume it's the full length of the buffer. Sometimes, this is the entire remainder of the ROM!
        // For non-malformed msgs, we should encounter a stop rule way before then, though.
        buf.len()
    };

    buf.get(offset..next_offset)
}
