//! Package-defined simulation hooks. Interpretation stays in the program;
//! emulator adapters validate and atomically apply the returned changes.
mod configuration;
pub mod identity;
pub use configuration::{Configuration, OptionValue, MAX_CONFIGURATION_BYTES};

pub use crate::telemetry::stream::{Error, Memory, Source, Value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub player: u16,
    pub seed: [u8; 16],
    pub disable_bgm: bool,
    pub options: BTreeMap<String, Value>,
}
impl Context {
    pub fn validate(&self) -> Result<(), Error> {
        if !(1..=2).contains(&self.player) || self.options.len() > 128 {
            return Err("invalid gamemode context".into());
        }
        if self.options.contains_key("disable_bgm") {
            return Err("disable_bgm belongs in the gamemode context, not package options".into());
        }
        for (name, value) in &self.options {
            if name.is_empty()
                || name.len() > 128
                || matches!(value, Value::Number(n) if !n.is_finite())
                || matches!(value, Value::String(s) if s.len() > 4096)
            {
                return Err("invalid gamemode option".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    pub name: String,
    pub address: u32,
}
fn validate_hooks(hooks: &[Hook]) -> Result<(), Error> {
    if hooks.is_empty() || hooks.len() > 256 {
        return Err("invalid gamemode hook count".into());
    }
    let mut names = BTreeSet::new();
    let mut addresses = BTreeSet::new();
    for hook in hooks {
        if hook.name.is_empty() || hook.name.len() > 128 || !names.insert(&hook.name) || !addresses.insert(hook.address)
        {
            return Err("gamemode hooks must have unique names and addresses".into());
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Write {
    Memory {
        space: String,
        address: u32,
        #[serde(with = "crate::bytes")]
        data: Vec<u8>,
    },
    Register {
        name: String,
        value: u32,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Ready,
    RoundStarted,
    RoundOutcome { winner: Option<u16> },
    MatchEnded,
    MatchAborted,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub writes: Vec<Write>,
    pub events: Vec<Event>,
}
impl Update {
    pub fn validate(&self) -> Result<(), Error> {
        if self.writes.len() > 4096 || self.events.len() > 128 {
            return Err("gamemode update count exceeded".into());
        }
        let mut bytes = 0usize;
        for write in &self.writes {
            match write {
                Write::Memory { space, address, data } => {
                    if space.is_empty()
                        || space.len() > 64
                        || u64::from(*address) + data.len() as u64 > u64::from(u32::MAX) + 1
                    {
                        return Err("invalid gamemode memory write".into());
                    }
                    bytes = bytes.saturating_add(data.len());
                }
                Write::Register { name, .. } if name.is_empty() || name.len() > 64 => {
                    return Err("invalid gamemode register".into())
                }
                _ => {}
            }
        }
        if bytes > 4 * 1024 * 1024 {
            return Err("gamemode write byte limit exceeded".into());
        }
        for event in &self.events {
            if matches!(event,Event::RoundOutcome {winner:Some(player)} if !(1..=2).contains(player)) {
                return Err("invalid gamemode outcome player".into());
            }
        }
        Ok(())
    }
}
/// Overlay byte-addressed writes in issue order. Adapters can split/normalize
/// reads before calling this to honor aliases in their physical memory map.
pub fn overlay_writes(space: &str, address: u32, output: &mut [u8], writes: &[Write]) {
    for write in writes {
        if let Write::Memory {
            space: written_space,
            address: start,
            data,
        } = write
        {
            if written_space != space {
                continue;
            }
            let from = u64::from(address).max(u64::from(*start));
            let to = (u64::from(address) + output.len() as u64).min(u64::from(*start) + data.len() as u64);
            if from < to {
                output[(from - u64::from(address)) as usize..(to - u64::from(address)) as usize]
                    .copy_from_slice(&data[(from - u64::from(*start)) as usize..(to - u64::from(*start)) as usize]);
            }
        }
    }
}
/// Reads observe the core at hook entry. No write occurs until the whole update
/// succeeds. Validation must check every write without touching emulator state.
pub trait Reader: Memory {
    fn read_register(&mut self, name: &str) -> Result<u32, Error>;

    /// A script's direct reads must observe its preceding writes, including any
    /// backend-specific memory aliases. Writes here have not yet been validated.
    fn read_pending(&mut self, space: &str, address: u32, output: &mut [u8], writes: &[Write]) -> Result<(), Error> {
        self.read(space, address, output)?;
        overlay_writes(space, address, output, writes);
        Ok(())
    }
}
pub trait Machine: Reader {
    fn validate_update(&self, update: &Update) -> Result<(), Error>;
    /// Only called with an update that passed validate_update.
    fn apply_writes(&mut self, writes: &[Write]);
}
pub trait Program: Send + std::any::Any {
    fn source(&self) -> &Source;
    fn hooks(&self) -> &[Hook];
    fn on_hook(&self, name: &str, machine: &mut dyn Reader) -> Result<Update, Error>;
}

/// A host-verified, immutable seat configuration. Every open recreates callbacks
/// against the same effective ROM and options; no catalog lookup occurs here.
pub trait Factory: Send + Sync {
    fn configuration(&self) -> Result<Configuration, Error>;
    fn rom(&self) -> &[u8];
    fn open(&self, player: u16, seed: [u8; 16]) -> Result<Box<dyn Program>, Error>;
}

/// The adapter owns the machine; the program only sees its read interface.
/// Scripts stage direct writes; the adapter commits them after both validations.
pub struct Controller {
    program: Box<dyn Program>,
}

pub struct Snapshot {
    source: Source,
    program_type: std::any::TypeId,
}

impl Controller {
    pub fn new(program: Box<dyn Program>) -> Result<Self, Error> {
        validate_hooks(program.hooks())?;
        Ok(Self { program })
    }

    pub fn hooks(&self) -> &[Hook] {
        self.program.hooks()
    }

    pub fn on_hook(&self, name: &str, machine: &mut dyn Machine) -> Result<Vec<Event>, Error> {
        if !self.hooks().iter().any(|hook| hook.name == name) {
            return Err("unknown gamemode hook".into());
        }
        let update = self.program.on_hook(name, machine)?;
        update.validate()?;
        machine.validate_update(&update)?;
        machine.apply_writes(&update.writes);
        Ok(update.events)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            source: self.program.source().clone(),
            program_type: self.program.as_ref().type_id(),
        }
    }

    pub fn validate_snapshot(&self, snapshot: &Snapshot) -> Result<(), Error> {
        if self.program.source() != &snapshot.source || self.program.as_ref().type_id() != snapshot.program_type {
            return Err("capture belongs to a different gamemode module or context".into());
        }
        Ok(())
    }

    pub fn restore(&mut self, snapshot: &Snapshot) -> Result<(), Error> {
        self.validate_snapshot(snapshot)
    }
}
