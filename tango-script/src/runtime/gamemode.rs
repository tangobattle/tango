//! Direct script operations are staged until the callback and adapter validate.
//! There is no script state to rewind: every invocation recreates setup closures
//! from the frozen ROM, context and package graph.
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use mlua::{Function, LuaSerdeExt, Value};
use tango_match::gamemode::{Context, Event, Hook, Reader, Update, Write};

use super::Runtime;
use crate::{invalid, Result};

fn unsigned(value: Value) -> mlua::Result<u32> {
    let number = match value {
        Value::Integer(n) => n as f64,
        Value::Number(n) => n,
        _ => return Err(mlua::Error::runtime("expected an unsigned integer")),
    };
    if !number.is_finite() || number < 0.0 || number > u32::MAX as f64 || number.fract() != 0.0 {
        return Err(mlua::Error::runtime("expected an unsigned 32-bit integer"));
    }
    Ok(number as u32)
}

fn name(value: Value) -> mlua::Result<String> {
    let Value::String(value) = value else {
        return Err(mlua::Error::runtime("expected a name"));
    };
    let value = value.to_str()?;
    if value.is_empty() || value.len() > 64 {
        return Err(mlua::Error::runtime("invalid memory space or register name"));
    }
    Ok(value.to_owned())
}

fn in_callback(active: &Cell<bool>) -> mlua::Result<()> {
    if !active.get() {
        return Err(mlua::Error::runtime(
            "game operations are only available inside hook callbacks",
        ));
    }
    Ok(())
}

impl Runtime<'_> {
    pub fn import_replay(&self, legacy: &crate::LegacyReplay<'_>, rom: &[u8]) -> Result<Option<crate::ReplayImport>> {
        let Some(import) = self.entry().get::<Option<Function>>("import_replay")? else {
            return Ok(None);
        };
        let context = self.lua.to_value_with(
            legacy,
            mlua::serde::SerializeOptions::new().serialize_none_to_null(false),
        )?;
        let output: Value = import.call((context, self.buffer(rom)?))?;
        if output.is_nil() {
            return Ok(None);
        }
        let imported: crate::ReplayImport = crate::decode::from_value(
            &self.lua,
            output,
            crate::decode::Limits {
                values: 512,
                bytes: 16 * 1024,
                depth: 2,
                string: 4096,
                root: crate::decode::Root::Map,
            },
            false,
        )?;
        imported.validate()?;
        Ok(Some(imported))
    }

    pub fn gamemode_platform(&self) -> Result<Option<String>> {
        let value = self.entry().get::<Option<mlua::LuaString>>("platform")?;
        value
            .map(|value| {
                let platform = value.to_str()?;
                if !crate::package::valid_name(&platform) {
                    return Err(invalid("invalid gamemode platform"));
                }
                Ok(platform.to_owned())
            })
            .transpose()
    }

    pub fn setup_gamemode(&self, context: &Context) -> Result<Option<Vec<Hook>>> {
        Ok(self.run_gamemode(context, None, None)?.map(|(hooks, _)| hooks))
    }

    pub fn gamemode_hook(
        &self,
        name: &str,
        hooks: &[Hook],
        context: &Context,
        reader: &mut dyn Reader,
    ) -> Result<Update> {
        self.run_gamemode(context, Some((name, hooks)), Some(reader))?
            .map(|(_, update)| update)
            .ok_or_else(|| invalid("gamemode has no setup()"))
    }

    fn run_gamemode(
        &self,
        context: &Context,
        target: Option<(&str, &[Hook])>,
        reader: Option<&mut dyn Reader>,
    ) -> Result<Option<(Vec<Hook>, Update)>> {
        let Some(setup) = self.entry().get::<Option<Function>>("setup")? else {
            return Ok(None);
        };
        let callbacks = RefCell::new(BTreeMap::<u32, Function>::new());
        let active = Cell::new(false);
        let reader = RefCell::new(reader);
        let update = RefCell::new(Update::default());
        let write_bytes = Cell::new(0usize);
        let reads = Cell::new(0usize);
        let hooks = self.lua.scope(|scope| {
            let game = self.lua.create_table()?;
            game.set(
                "hook",
                scope.create_function(|_, (address, callback): (Value, Function)| {
                    if active.get() {
                        return Err(mlua::Error::runtime("register hooks during setup()"));
                    }
                    let address = unsigned(address)?;
                    let mut callbacks = callbacks.borrow_mut();
                    if callbacks.len() >= 256 || callbacks.contains_key(&address) {
                        return Err(mlua::Error::runtime(
                            "hooks require unique addresses; at most 256 are allowed",
                        ));
                    }
                    callbacks.insert(address, callback);
                    Ok(())
                })?,
            )?;
            game.set(
                "read",
                super::memory::read(scope, |space, address, output| {
                    in_callback(&active)?;
                    reader
                        .borrow_mut()
                        .as_deref_mut()
                        .unwrap()
                        .read_pending(space, address, output, &update.borrow().writes)
                        .map_err(|e| invalid(e.to_string()))?;
                    Ok(())
                })?,
            )?;
            game.set(
                "write",
                scope.create_function(|_, (space, address, data): (Value, Value, Value)| {
                    in_callback(&active)?;
                    let space = name(space)?;
                    let address = unsigned(address)?;
                    let Value::Buffer(data) = data else {
                        return Err(mlua::Error::runtime("game.write expects a buffer"));
                    };
                    if u64::from(address) + data.len() as u64 > u64::from(u32::MAX) + 1 {
                        return Err(mlua::Error::runtime("invalid memory write range"));
                    }
                    let bytes = write_bytes.get().saturating_add(data.len());
                    let mut update = update.borrow_mut();
                    if update.writes.len() >= 4096 || bytes > 4 * 1024 * 1024 {
                        return Err(mlua::Error::runtime("gamemode write limit exceeded"));
                    }
                    write_bytes.set(bytes);
                    update.writes.push(Write::Memory {
                        space,
                        address,
                        data: data.to_vec(),
                    });
                    Ok(())
                })?,
            )?;
            game.set(
                "read_register",
                scope.create_function(|_, register: Value| {
                    in_callback(&active)?;
                    let register = name(register)?;
                    reads.set(reads.get() + 1);
                    if reads.get() > 4096 {
                        return Err(mlua::Error::runtime("register read call limit exceeded"));
                    }
                    let original = reader
                        .borrow_mut()
                        .as_deref_mut()
                        .unwrap()
                        .read_register(&register)
                        .map_err(mlua::Error::external)?;
                    Ok(update
                        .borrow()
                        .writes
                        .iter()
                        .rev()
                        .find_map(|write| match write {
                            Write::Register { name, value } if name == &register => Some(*value),
                            _ => None,
                        })
                        .unwrap_or(original))
                })?,
            )?;
            game.set(
                "write_register",
                scope.create_function(|_, (register, value): (Value, Value)| {
                    in_callback(&active)?;
                    let name = name(register)?;
                    let value = unsigned(value)?;
                    let mut update = update.borrow_mut();
                    if update.writes.len() >= 4096 {
                        return Err(mlua::Error::runtime("gamemode write limit exceeded"));
                    }
                    update.writes.push(Write::Register { name, value });
                    Ok(())
                })?,
            )?;
            for (method, event) in [
                ("ready", Event::Ready),
                ("round_started", Event::RoundStarted),
                ("match_ended", Event::MatchEnded),
                ("match_aborted", Event::MatchAborted),
            ] {
                let active = &active;
                let update = &update;
                game.set(
                    method,
                    scope.create_function(move |_, ()| {
                        in_callback(active)?;
                        let mut update = update.borrow_mut();
                        if update.events.len() >= 128 {
                            return Err(mlua::Error::runtime("gamemode event limit exceeded"));
                        }
                        update.events.push(event.clone());
                        Ok(())
                    })?,
                )?;
            }
            game.set(
                "round_outcome",
                scope.create_function(|_, value: Value| {
                    in_callback(&active)?;
                    let winner = match value {
                        Value::Nil => None,
                        value => {
                            let winner = unsigned(value)?;
                            if !(1..=2).contains(&winner) {
                                return Err(mlua::Error::runtime("winner must be 1, 2 or nil"));
                            }
                            Some(winner as u16)
                        }
                    };
                    let mut update = update.borrow_mut();
                    if update.events.len() >= 128 {
                        return Err(mlua::Error::runtime("gamemode event limit exceeded"));
                    }
                    update.events.push(Event::RoundOutcome { winner });
                    Ok(())
                })?,
            )?;
            game.set_readonly(true);
            let result: Value = setup.call((self.lua.to_value(context)?, game))?;
            if !result.is_nil() {
                return Err(mlua::Error::runtime(
                    "setup registers hooks and must not return a value",
                ));
            }
            let hooks: Vec<_> = callbacks
                .borrow()
                .keys()
                .map(|address| Hook {
                    name: format!("{address:08x}"),
                    address: *address,
                })
                .collect();
            if hooks.is_empty() {
                return Err(mlua::Error::runtime("setup must register at least one hook"));
            }
            if let Some((name, expected)) = target {
                if hooks != expected {
                    return Err(mlua::Error::runtime("gamemode hook addresses changed since setup"));
                }
                let hook = hooks
                    .iter()
                    .find(|hook| hook.name == name)
                    .ok_or_else(|| mlua::Error::runtime("unknown gamemode hook"))?;
                let callback = callbacks.borrow()[&hook.address].clone();
                active.set(true);
                let result: Value = callback.call(())?;
                if !result.is_nil() {
                    return Err(mlua::Error::runtime("hook callbacks must not return a value"));
                }
            }
            Ok(hooks)
        })?;
        let update = update.into_inner();
        update.validate().map_err(|e| invalid(e.to_string()))?;
        Ok(Some((hooks, update)))
    }
}
