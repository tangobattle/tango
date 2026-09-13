//! Resource checks shared by all Serde-decoded script output.
//! Table shapes and enum tags belong to derived Deserialize implementations.
use std::collections::BTreeSet;

use mlua::{Lua, LuaSerdeExt, Value};
use serde::de::DeserializeOwned;

use crate::{invalid, Result};

pub(crate) struct Limits {
    pub values: usize,
    pub bytes: usize,
    pub depth: usize,
    pub string: usize,
    pub root: Root,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Root {
    Map,
    Sequence,
}

pub(crate) fn from_value<T: DeserializeOwned>(lua: &Lua, value: Value, limits: Limits, arrays: bool) -> Result<T> {
    let mut budget = Budget {
        limits,
        active: BTreeSet::new(),
    };
    budget.check(&value, 0)?;
    Ok(lua.from_value_with(
        value,
        mlua::serde::DeserializeOptions::new().encode_empty_tables_as_array(arrays),
    )?)
}

struct Budget {
    limits: Limits,
    active: BTreeSet<usize>,
}
impl Budget {
    fn bytes(&mut self, len: usize) -> Result<()> {
        self.limits.bytes = self
            .limits
            .bytes
            .checked_sub(len)
            .ok_or_else(|| invalid("script output exceeds its byte limit"))?;
        Ok(())
    }
    fn check(&mut self, value: &Value, depth: usize) -> Result<()> {
        if depth > self.limits.depth {
            return Err(invalid("script output exceeds its depth limit"));
        }
        self.limits.values = self
            .limits
            .values
            .checked_sub(1)
            .ok_or_else(|| invalid("script output exceeds its value limit"))?;
        match value {
            Value::Nil | Value::Boolean(_) | Value::Integer(_) => {}
            Value::Number(n) if n.is_finite() => {}
            Value::String(s) => {
                let len = s.as_bytes().len();
                if len > self.limits.string {
                    return Err(invalid("script output string exceeds its limit"));
                }
                // Binary payloads must use buffers. mlua's Serde bridge
                // otherwise presents non-UTF-8 strings as bytes to visitors.
                s.to_str()?;
                self.bytes(len)?;
            }
            Value::Buffer(b) => self.bytes(b.len())?,
            Value::Table(table) => {
                if table.metatable().is_some() {
                    return Err(invalid("script output tables must not have metatables"));
                }
                let identity = table.to_pointer() as usize;
                if !self.active.insert(identity) {
                    return Err(invalid("script output contains a recursive table"));
                }
                let mut entries = 0;
                let mut max_index = 0;
                let mut named = false;
                for pair in table.pairs::<Value, Value>() {
                    let (key, value) = pair?;
                    match &key {
                        Value::String(_) => named = true,
                        Value::Integer(n) if *n > 0 => max_index = max_index.max(*n as u64),
                        Value::Number(n) if *n > 0.0 && n.fract() == 0.0 => max_index = max_index.max(*n as u64),
                        _ => return Err(invalid("script output keys must be strings or positive array indices")),
                    }
                    entries += 1;
                    self.check(&key, depth + 1)?;
                    self.check(&value, depth + 1)?;
                }
                // mlua's array decoding follows raw_len. Reject mixed/sparse
                // tables so fields cannot disappear silently during conversion.
                if max_index != 0 && (named || max_index != entries) {
                    return Err(invalid("script output arrays must be dense and have no named fields"));
                }
                if depth == 0
                    && ((self.limits.root == Root::Sequence && named)
                        || (self.limits.root == Root::Map && max_index != 0))
                {
                    return Err(invalid("script output has the wrong collection shape"));
                }
                self.active.remove(&identity);
            }
            _ => return Err(invalid("script output contains an unsupported value")),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_cannot_silently_discard_named_fields() {
        let lua = crate::runtime::new_lua();
        for source in [
            r#"return {"diagnostic"}"#,
            r#"return {}"#,
            r#"return {message = "diagnostic"}"#,
        ] {
            let value = lua.load(source).eval().unwrap();
            let result = from_value::<Vec<String>>(
                &lua,
                value,
                Limits {
                    values: 513,
                    bytes: 4 * 1024 * 1024,
                    depth: 1,
                    string: 16 * 1024,
                    root: Root::Sequence,
                },
                true,
            );
            assert_eq!(result.is_ok(), !source.contains("message"));
        }
    }
}
