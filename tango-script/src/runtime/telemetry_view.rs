use super::Runtime;
use crate::{decode, invalid, Action, Node, Result, TelemetryViewContext};
use mlua::{Function, LuaSerdeExt, Table, Value};
use std::collections::BTreeMap;
use tango_match::telemetry::stream::{Range, Timeline, ValueAt};

fn input<T: serde::de::DeserializeOwned>(lua: &mlua::Lua, value: Value) -> Result<T> {
    decode::from_value(
        lua,
        value,
        decode::Limits {
            values: 20,
            bytes: 8192,
            depth: 2,
            string: 4096,
            root: decode::Root::Map,
        },
        false,
    )
}

impl Runtime<'_> {
    pub fn telemetry_view(
        &self,
        history: &Timeline,
        context: TelemetryViewContext,
        state: &BTreeMap<String, String>,
    ) -> Result<Option<Node>> {
        let Some(view) = self.entry().get::<Option<Function>>("view")? else {
            return Ok(None);
        };
        let table: Table = self.lua.scope(|scope| {
            let data = self.lua.create_table()?;
            let mut values = 64usize;
            data.set(
                "value",
                scope.create_function_mut(move |lua, query: Value| {
                    let query: ValueAt = input(lua, query).map_err(mlua::Error::external)?;
                    values = values
                        .checked_sub(1)
                        .ok_or_else(|| mlua::Error::runtime("telemetry query limit exceeded"))?;
                    let result = history.value_at(&query).map_err(mlua::Error::external)?;
                    lua.to_value_with(
                        &result,
                        mlua::serde::SerializeOptions::new().serialize_none_to_null(false),
                    )
                })?,
            )?;
            let mut calls = 64usize;
            let mut points = 8192usize;
            data.set(
                "series",
                scope.create_function_mut(move |lua, (range, resolution): (Value, Value)| {
                    let range: Range = input(lua, range).map_err(mlua::Error::external)?;
                    // Let Serde validate the numeric type without Lua's coercing FromLua conversions.
                    let resolution: usize = lua.from_value(resolution)?;
                    calls = calls
                        .checked_sub(1)
                        .ok_or_else(|| mlua::Error::runtime("telemetry query limit exceeded"))?;
                    points = points
                        .checked_sub(resolution)
                        .ok_or_else(|| mlua::Error::runtime("telemetry resolution budget exceeded"))?;
                    let result = history
                        .numeric_series(&range, resolution)
                        .map_err(mlua::Error::external)?;
                    lua.to_value_with(
                        &result,
                        mlua::serde::SerializeOptions::new().serialize_none_to_null(false),
                    )
                })?,
            )?;
            let mut calls = 64usize;
            data.set(
                "count_events",
                scope.create_function_mut(move |lua, range: Value| {
                    let range: Range = input(lua, range).map_err(mlua::Error::external)?;
                    calls = calls
                        .checked_sub(1)
                        .ok_or_else(|| mlua::Error::runtime("telemetry query limit exceeded"))?;
                    history.count_events(&range).map_err(mlua::Error::external)
                })?,
            )?;
            data.set_readonly(true);
            view.call((data, self.lua.to_value(&context)?, self.lua.to_value(state)?))
        })?;
        Ok(Some(Node::read(&self.lua, table)?))
    }

    pub fn update_telemetry_view(
        &self,
        state: &BTreeMap<String, String>,
        action: &Action,
    ) -> Result<BTreeMap<String, String>> {
        let Some(update) = self.entry().get::<Option<Function>>("update_view")? else {
            return Ok(state.clone());
        };
        let value = update.call((self.lua.to_value(state)?, self.lua.to_value(action)?))?;
        let state: BTreeMap<String, String> = decode::from_value(
            &self.lua,
            value,
            decode::Limits {
                values: 2049,
                bytes: 64 * 1024,
                depth: 2,
                string: 4096,
                root: decode::Root::Map,
            },
            false,
        )?;
        if state.len() > 1024 {
            return Err(invalid("telemetry view state exceeds 1024 entries"));
        }
        Ok(state)
    }
}
