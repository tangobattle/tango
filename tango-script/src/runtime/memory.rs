//! The same bounded, scoped memory reader serves telemetry and match hooks.
use mlua::{Function, Scope, Value};

pub(super) fn read<'scope, 'env: 'scope, F>(scope: &'scope Scope<'scope, 'env>, mut read: F) -> mlua::Result<Function>
where
    F: FnMut(&str, u32, &mut [u8]) -> crate::Result<()> + 'scope,
{
    let mut remaining: usize = 4 * 1024 * 1024;
    let mut calls = 4096usize;
    scope.create_function_mut(move |lua, (space, address, length): (Value, Value, Value)| {
        let number = |value: Value| match value {
            Value::Integer(value) => Some(value as f64),
            Value::Number(value) => Some(value),
            _ => None,
        };
        let (Value::String(space), Some(address), Some(length)) = (space, number(address), number(length)) else {
            return Err(mlua::Error::runtime("memory.read expects a space, address and length"));
        };
        let space = space.to_str()?;
        if space.is_empty() || space.len() > 64 {
            return Err(mlua::Error::runtime("invalid memory address space"));
        }
        if !address.is_finite()
            || address < 0.0
            || address.fract() != 0.0
            || address > u32::MAX as f64
            || !length.is_finite()
            || length < 0.0
            || length.fract() != 0.0
            || length > 1024.0 * 1024.0
            || address + length > u32::MAX as f64 + 1.0
        {
            return Err(mlua::Error::runtime("invalid memory range"));
        }
        let length = length as usize;
        calls = calls
            .checked_sub(1)
            .ok_or_else(|| mlua::Error::runtime("memory read call limit exceeded"))?;
        remaining = remaining
            .checked_sub(length)
            .ok_or_else(|| mlua::Error::runtime("memory read byte limit exceeded"))?;
        let mut bytes = vec![0; length];
        read(&space, address as u32, &mut bytes).map_err(mlua::Error::external)?;
        lua.create_buffer(&bytes)
    })
}
