mod gamemode;
mod memory;
mod telemetry_view;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use mlua::{Buffer, FromLua, Function, Lua, LuaSerdeExt, Table, Value, VmState};

use crate::{invalid, Action, Node, Profile, Result};

pub const MAX_BUFFER: usize = 32 * 1024 * 1024;

pub(crate) fn new_lua() -> Lua {
    crate::checker::initialize();
    Lua::new()
}

pub(crate) fn charge(remaining: &AtomicUsize, bytes: usize) -> Result<()> {
    remaining
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(bytes))
        .map(|_| ())
        .map_err(|_| invalid("script host work budget exceeded"))
}

/// An exclusive operation in a graph's VM. Only compiled prototypes survive;
/// module environments, upvalues, inputs and quotas are recreated each time.
pub(crate) struct Runtime<'a> {
    lua: Lua,
    entry: Option<Table>,
    host_count: usize,
    machine: std::sync::MutexGuard<'a, Option<Lua>>,
}

pub(crate) struct Update {
    pub bytes: Vec<u8>,
    pub state: std::collections::BTreeMap<String, String>,
    pub effects: Vec<crate::Effect>,
}

pub(crate) fn validate_export(kind: crate::ExportKind, value: &Value) -> Result<()> {
    let Value::Table(entry) = value else {
        return Err(invalid(format!("{} module must return a table", kind.as_str())));
    };
    // Recognition is owned by the individual capability, not a wrapper driver.
    entry.get::<Option<Function>>("detect_rom")?;
    let required: &[&str] = match kind {
        crate::ExportKind::Editor => {
            entry.get::<Option<Function>>("reset_view")?;
            let templates = entry.get::<Option<Function>>("templates")?;
            let create = entry.get::<Option<Function>>("create_save")?;
            if templates.is_some() != create.is_some() {
                return Err(invalid("editors must supply both templates() and create_save()"));
            }
            &["decode", "encode", "validate", "view", "update"]
        }
        crate::ExportKind::GameMode => {
            entry.get::<Option<Function>>("import_replay")?;
            entry.get::<Option<Function>>("patch_rom")?;
            entry.get::<Option<Function>>("setup")?;
            if entry.contains_key("on_hook")? {
                return Err(invalid("register callbacks with game.hook() inside setup()"));
            }
            &[]
        }
        crate::ExportKind::Telemetry => {
            entry.get::<Option<Function>>("initial_state")?;
            entry.get::<Option<Function>>("view")?;
            entry.get::<Option<Function>>("update_view")?;
            &["poll"]
        }
    };
    for name in required {
        entry
            .get::<Function>(*name)
            .map_err(|_| invalid(format!("{} must provide {name}()", kind.as_str())))?;
    }
    Ok(())
}

impl<'a> Runtime<'a> {
    pub fn new(profile: &'a Profile) -> Result<Self> {
        Self::for_export(profile, crate::ExportKind::Editor)
    }

    pub fn for_export(profile: &'a Profile, kind: crate::ExportKind) -> Result<Self> {
        Self::load(profile, profile.export_module(kind)?)
    }

    pub fn for_test(profile: &'a Profile) -> Result<Self> {
        let entry = profile
            .graph
            .entries
            .last()
            .ok_or_else(|| invalid("empty package profile"))?;
        Self::load(profile, entry)
    }

    fn load(profile: &'a Profile, entry: &str) -> Result<Self> {
        let mut machine = profile
            .graph
            .machine
            .lock()
            .map_err(|_| invalid("package VM lock poisoned"))?;
        if machine.is_none() {
            *machine = Some(sandboxed_lua()?);
        }
        let lua = machine.as_ref().unwrap().clone();
        // Construct the lease before any fallible operation so errors clean up
        // their environments and closures just like successful calls.
        let mut runtime = Self {
            lua: lua.clone(),
            entry: None,
            host_count: profile.packages.len(),
            machine,
        };
        let remaining = AtomicUsize::new(4_000_000);
        lua.set_interrupt(move |_| {
            if remaining
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                .is_err()
            {
                return Err(mlua::Error::RuntimeError("script execution budget exceeded".into()));
            }
            Ok(VmState::Continue)
        });
        let host_budget = Arc::new(AtomicUsize::new(128 * 1024 * 1024));
        lua.set_named_registry_value(crate::modules::CACHE, lua.create_table()?)?;
        crate::i18n::initialize(&lua)?;
        for (index, package) in profile.packages.iter().enumerate() {
            let host = lua.create_table()?;
            crate::encoding::install(&lua, &host, host_budget.clone())?;
            let locale = profile.locale.to_string();
            host.set("locale", lua.create_function(move |_, ()| Ok(locale.clone()))?)?;
            let direction = match profile.locale.character_direction() {
                unic_langid::CharacterDirection::LTR => "ltr",
                unic_langid::CharacterDirection::RTL => "rtl",
                unic_langid::CharacterDirection::TTB => "ttb",
            };
            host.set("text_direction", lua.create_function(move |_, ()| Ok(direction))?)?;
            let budget = host_budget.clone();
            host.set(
                "lowercase",
                lua.create_function(move |_, value: Value| {
                    let Value::String(value) = value else {
                        return Err(mlua::Error::runtime("lowercase expects a string"));
                    };
                    if value.as_bytes().len() > 16 * 1024 {
                        return Err(mlua::Error::runtime("text exceeds 16 KiB"));
                    }
                    charge(&budget, value.as_bytes().len() * 3 + 64)
                        .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    let output = value.to_str()?.to_lowercase();
                    if output.len() > 16 * 1024 {
                        return Err(mlua::Error::runtime("lowercase text exceeds 16 KiB"));
                    }
                    Ok(output)
                })?,
            )?;
            let inputs = profile.inputs.clone();
            host.set(
                "input_size",
                lua.create_function(move |_, name: String| Ok(inputs.get(&name).map(<[u8]>::len)))?,
            )?;
            let inputs = profile.inputs.clone();
            let budget = host_budget.clone();
            host.set(
                "read_input",
                lua.create_function(move |lua, (name, offset, length): (String, f64, f64)| {
                    let bytes = inputs
                        .get(&name)
                        .ok_or_else(|| mlua::Error::runtime(format!("input not supplied: {name}")))?;
                    // Check numbers before conversion: fractional, negative and
                    // non-finite Luau numbers must never truncate or wrap.
                    if !offset.is_finite()
                        || offset < 0.0
                        || offset.fract() != 0.0
                        || offset > bytes.len() as f64
                        || !length.is_finite()
                        || length < 0.0
                        || length.fract() != 0.0
                        || length > MAX_BUFFER as f64
                    {
                        return Err(mlua::Error::runtime("invalid input range"));
                    }
                    let offset = offset as usize;
                    let length = length as usize;
                    let slice = bytes
                        .get(offset..offset + length)
                        .ok_or_else(|| mlua::Error::runtime("input range out of bounds"))?;
                    charge(&budget, length).map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    lua.create_buffer(slice)
                })?,
            )?;
            let files = package.files.clone();
            let budget = host_budget.clone();
            host.set(
                "read_file",
                lua.create_function(move |lua, path: String| {
                    let path = crate::package::file_path(&path).map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    let asset = files
                        .get(&path)
                        .ok_or_else(|| mlua::Error::RuntimeError(format!("package file not found: {path}")))?;
                    if asset.len() > MAX_BUFFER {
                        return Err(mlua::Error::RuntimeError("package file exceeds buffer limit".into()));
                    }
                    charge(&budget, asset.len()).map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    lua.create_buffer(asset)
                })?,
            )?;
            let budget = host_budget.clone();
            host.set(
                "crc32",
                lua.create_function(move |_, bytes: Buffer| {
                    if bytes.len() > MAX_BUFFER {
                        return Err(mlua::Error::runtime("CRC32 buffer exceeds 32 MiB"));
                    }
                    charge(&budget, bytes.len()).map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    use std::io::Read;
                    let mut source = bytes.cursor();
                    let mut hash = crc32fast::Hasher::new();
                    let mut chunk = [0; 16 * 1024];
                    loop {
                        let n = source.read(&mut chunk).map_err(mlua::Error::external)?;
                        if n == 0 {
                            break;
                        }
                        hash.update(&chunk[..n]);
                    }
                    Ok(hash.finalize())
                })?,
            )?;
            let budget = host_budget.clone();
            host.set(
                "unlz77",
                lua.create_function(move |lua, (source, offset): (Buffer, Option<f64>)| {
                    use std::io::Seek;
                    if source.len() > MAX_BUFFER {
                        return Err(mlua::Error::runtime("LZ77 buffer exceeds 32 MiB"));
                    }
                    let offset = offset.unwrap_or(0.0);
                    if !offset.is_finite() || offset < 0.0 || offset.fract() != 0.0 || offset > source.len() as f64 {
                        return Err(mlua::Error::runtime("invalid LZ77 offset"));
                    }
                    let mut source = source.cursor();
                    source
                        .seek(std::io::SeekFrom::Start(offset as u64))
                        .map_err(mlua::Error::external)?;
                    let output = crate::lz77::decode(&mut source, |n| charge(&budget, n))
                        .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    output.map(|bytes| lua.create_buffer(bytes)).transpose()
                })?,
            )?;
            let budget = host_budget.clone();
            host.set(
                "bps",
                lua.create_function(move |lua, (source, patch): (Buffer, Buffer)| {
                    if source.len() > MAX_BUFFER || patch.len() > MAX_BUFFER {
                        return Err(mlua::Error::RuntimeError("BPS buffer exceeds 32 MiB".into()));
                    }
                    let output = crate::bps::apply(&source.to_vec(), &patch.to_vec(), |n| charge(&budget, n))
                        .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
                    lua.create_buffer(output)
                })?,
            )?;
            host.set_readonly(true);
            lua.set_named_registry_value(&crate::modules::host_key(index), host)?;
        }
        let loader =
            crate::modules::Loader::new(profile.graph.clone(), crate::i18n::Context::new(profile, host_budget));
        runtime.entry = Some(Table::from_lua(loader.load(&lua, entry)?, &lua)?);
        Ok(runtime)
    }

    fn entry(&self) -> &Table {
        self.entry.as_ref().expect("loaded module")
    }

    fn buffer(&self, bytes: &[u8]) -> Result<Buffer> {
        if bytes.len() > MAX_BUFFER {
            return Err(invalid("buffer exceeds 32 MiB"));
        }
        Ok(self.lua.create_buffer(bytes)?)
    }

    pub fn test(&self) -> Result<()> {
        let test: Function = self.entry().get("test")?;
        Ok(test.call::<()>(())?)
    }

    pub fn detect_rom(&self, rom: &[u8]) -> Result<bool> {
        let Some(detect) = self.entry().get::<Option<Function>>("detect_rom")? else {
            return Ok(false);
        };
        match detect.call::<Value>(self.buffer(rom)?)? {
            Value::Boolean(matches) => Ok(matches),
            _ => Err(invalid("detect_rom must return a boolean")),
        }
    }

    pub fn transform(&self, callback: &str, bytes: &[u8]) -> Result<Vec<u8>> {
        let input = self.buffer(bytes)?;
        let function = if callback == "patch_rom" {
            let Some(function) = self.entry().get::<Option<Function>>(callback)? else {
                return Ok(bytes.to_vec());
            };
            function
        } else {
            self.entry().get::<Function>(callback)?
        };
        let output: Buffer = function.call(input)?;
        if output.len() > MAX_BUFFER {
            return Err(invalid("script output exceeds 32 MiB"));
        }
        Ok(output.to_vec())
    }

    pub fn poll_telemetry<F>(
        &self,
        state: &[u8],
        context: crate::TelemetryContext,
        read: F,
    ) -> Result<crate::TelemetrySample>
    where
        F: FnMut(&str, u32, &mut [u8]) -> Result<()>,
    {
        if state.len() > 64 * 1024 {
            return Err(invalid("telemetry state exceeds 64 KiB"));
        }
        if context.player == 0 || context.player > 256 {
            return Err(invalid("telemetry player must be in 1..=256"));
        }
        let state = self.buffer(state)?;
        let value = self.lua.scope(|scope| {
            let memory = self.lua.create_table()?;
            memory.set("read", memory::read(scope, read)?)?;
            memory.set_readonly(true);
            self.entry()
                .get::<Function>("poll")?
                .call::<Value>((state.clone(), self.lua.to_value(&context)?, memory))
        })?;
        let frame = crate::decode::from_value(
            &self.lua,
            value,
            crate::decode::Limits {
                values: 4096,
                bytes: 64 * 1024,
                depth: 5,
                string: 4096,
                root: crate::decode::Root::Map,
            },
            true,
        )?;
        Ok(crate::TelemetrySample {
            state: state.to_vec(),
            frame,
        })
    }

    pub fn initial_telemetry_state(&self) -> Result<Vec<u8>> {
        let Some(initial) = self.entry().get::<Option<Function>>("initial_state")? else {
            return Ok(Vec::new());
        };
        let state: Buffer = initial.call(())?;
        if state.len() > 64 * 1024 {
            return Err(invalid("telemetry state exceeds 64 KiB"));
        }
        Ok(state.to_vec())
    }

    pub fn save_templates(&self) -> Result<Vec<crate::SaveTemplate>> {
        let Some(templates) = self.entry().get::<Option<Function>>("templates")? else {
            return Ok(Vec::new());
        };
        let templates: Vec<crate::SaveTemplate> = crate::decode::from_value(
            &self.lua,
            templates.call::<Value>(())?,
            crate::decode::Limits {
                values: 512,
                root: crate::decode::Root::Sequence,
                bytes: 64 * 1024,
                depth: 2,
                string: 1024,
            },
            true,
        )?;
        let mut names = std::collections::BTreeSet::new();
        if templates.len() > 64
            || templates
                .iter()
                .any(|template| template.name.is_empty() || template.name.len() > 128 || !names.insert(&template.name))
        {
            return Err(invalid("save templates must have unique names and at most 64 entries"));
        }
        Ok(templates)
    }

    pub fn create_save(&self, name: &str) -> Result<Vec<u8>> {
        let output: Buffer = self.entry().get::<Function>("create_save")?.call(name)?;
        if output.len() > crate::editor::MAX_DOCUMENT {
            return Err(invalid("save template exceeds 8 MiB"));
        }
        Ok(output.to_vec())
    }

    pub fn validate(&self, bytes: &[u8]) -> Result<Vec<String>> {
        let table: Table = self.entry().get::<Function>("validate")?.call(self.buffer(bytes)?)?;
        crate::decode::from_value(
            &self.lua,
            Value::Table(table),
            crate::decode::Limits {
                values: 513,
                root: crate::decode::Root::Sequence,
                bytes: 256 * 16 * 1024,
                depth: 1,
                string: 16 * 1024,
            },
            true,
        )
    }

    pub fn view(
        &self,
        bytes: &[u8],
        state: &std::collections::BTreeMap<String, String>,
        context: crate::EditorContext,
    ) -> Result<Node> {
        let table: Table = self.entry().get::<Function>("view")?.call((
            self.buffer(bytes)?,
            self.lua.to_value(state)?,
            self.lua.to_value(&context)?,
        ))?;
        Node::read(&self.lua, table)
    }

    pub fn update(
        &self,
        bytes: &[u8],
        state: &std::collections::BTreeMap<String, String>,
        action: &Action,
        context: crate::EditorContext,
    ) -> Result<Update> {
        let buffer = self.buffer(bytes)?;
        let state: Table = self
            .lua
            .create_table_from(state.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;
        let effects = self.entry().get::<Function>("update")?.call::<Value>((
            buffer.clone(),
            state.clone(),
            self.lua.to_value(action)?,
            self.lua.to_value(&context)?,
        ))?;
        let state = crate::decode::from_value(
            &self.lua,
            Value::Table(state),
            crate::decode::Limits {
                values: 2049,
                root: crate::decode::Root::Map,
                bytes: 64 * 1024,
                depth: 1,
                string: 64 * 1024,
            },
            false,
        )?;
        Ok(Update {
            bytes: buffer.to_vec(),
            state,
            effects: crate::effects::read(&self.lua, effects)?,
        })
    }

    pub fn reset_view(
        &self,
        state: &std::collections::BTreeMap<String, String>,
    ) -> Result<std::collections::BTreeMap<String, String>> {
        let Some(reset) = self.entry().get::<Option<Function>>("reset_view")? else {
            return Ok(Default::default());
        };
        let state = self
            .lua
            .create_table_from(state.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;
        reset.call::<()>(state.clone())?;
        crate::decode::from_value(
            &self.lua,
            Value::Table(state),
            crate::decode::Limits {
                values: 2049,
                root: crate::decode::Root::Map,
                bytes: 64 * 1024,
                depth: 1,
                string: 64 * 1024,
            },
            false,
        )
    }
}

fn sandboxed_lua() -> Result<Lua> {
    let lua = new_lua();
    lua.set_compiler(mlua::chunk::Compiler::new().set_optimization_level(2));
    #[cfg(not(target_arch = "wasm32"))]
    lua.enable_jit(true);
    lua.set_memory_limit(128 * 1024 * 1024)?;
    // Packages have no filesystem, network, clock, dynamic loader, or
    // catchable quota errors. Coroutines cannot escape the operation.
    for name in [
        "os",
        "io",
        "debug",
        "require",
        "loadstring",
        "load",
        "dofile",
        "loadfile",
        "getfenv",
        "setfenv",
        "collectgarbage",
        "gcinfo",
        "newproxy",
        "coroutine",
        "pcall",
        "xpcall",
        "print",
        "_G",
    ] {
        lua.globals().set(name, Value::Nil)?;
    }
    let math: Table = lua.globals().get("math")?;
    math.set("random", Value::Nil)?;
    math.set("randomseed", Value::Nil)?;
    lua.sandbox(true)?;
    lua.globals().set_readonly(true);
    lua.set_named_registry_value(crate::modules::COMPILED, lua.create_table()?)?;
    Ok(lua)
}

impl Drop for Runtime<'_> {
    fn drop(&mut self) {
        self.lua.remove_interrupt();
        self.entry.take();
        let cleanup = || -> mlua::Result<()> {
            self.lua.unset_named_registry_value(crate::modules::CACHE)?;
            self.lua.unset_named_registry_value(crate::i18n::TRANSLATORS)?;
            for index in 0..self.host_count {
                self.lua.unset_named_registry_value(&crate::modules::host_key(index))?;
            }
            // Package callbacks can form Lua/Rust reference cycles via require.
            // Collect them before releasing this operation's exclusive lease.
            self.lua.gc_collect()
        };
        if cleanup().is_err() {
            // Never reuse a VM whose operation roots could not be released.
            self.machine.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Inputs, Package, PackageRef, Profile};

    #[test]
    fn reused_native_code_isolates_modules_and_recovers_from_failed_operations() {
        let package = Package::load(
            [
                (
                    "package.toml".into(),
                    b"api = 1\nname = 'runtime-test'\nversion = '1.0.0'\ndefault_gamemode='main'\n[[gamemode]]\nname = 'main'\npath = './init'\n"
                        .to_vec(),
                ),
                (
                    "init.luau".into(),
                    br#"--!strict
local shared = require('./shared')
local mode = buffer.readu8(tango.read_input('mode', 0, 1), 0)
local locale = tango.locale()
local counter = 0
return {patch_rom = function(bytes: buffer): buffer
    counter += 1
    assert(counter == 1)
    assert(mode == buffer.readu8(bytes, 0))
    assert(shared == require('./shared'))
    assert(shared.increment() == 1)
    assert(shared.mode == mode)
    assert(shared.locale == locale)
    assert(locale == (mode == 0 and 'en-US' or 'ja-JP'))
    if mode == 1 then error('intentional failure') end
    if mode == 2 then while true do end end
    if mode == 3 then return buffer.create(256 * 1024 * 1024) end
    return bytes
end}
"#
                    .to_vec(),
                ),
                (
                    "shared.luau".into(),
                    br#"--!strict
local state = {counter = 0}
return {
    mode = buffer.readu8(tango.read_input('mode', 0, 1), 0),
    locale = tango.locale(),
    increment = function(): number state.counter += 1; return state.counter end,
}
"#
                    .to_vec(),
                ),
            ]
            .into(),
        )
        .unwrap();
        let profile = Profile::resolve(
            &[package],
            &PackageRef {
                name: "runtime-test".into(),
                version: "1.0.0".parse().unwrap(),
            },
        )
        .unwrap();
        let graph = std::sync::Arc::downgrade(&profile.graph);
        for mode in [0, 1, 0, 2, 0, 3, 0] {
            let operation = profile
                .clone()
                .with_inputs(Inputs::new([("mode".into(), vec![mode])].into()).unwrap())
                .with_locale(if mode == 0 { "en-US" } else { "ja-JP" })
                .unwrap();
            let result = operation.patch_rom(&[mode]);
            match mode {
                0 => assert_eq!(result.unwrap(), [0]),
                1 => assert!(result.unwrap_err().to_string().contains("intentional failure")),
                2 => assert!(result.unwrap_err().to_string().contains("budget exceeded")),
                3 => assert!(result.is_err()),
                _ => unreachable!(),
            }
        }
        // Registry cleanup and collection must break the require closure's
        // Lua -> Rust -> graph -> Lua cycle, even after a quota failure.
        drop(profile);
        assert!(graph.upgrade().is_none());
    }
}
