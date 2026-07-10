//! The Lua backend (mlua, vendored Lua 5.4). See the parent module docs
//! for the API surface it binds.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use super::{HostState, ScriptBackend, BUDGET_SLICE, BUDGET_SLICES, KEYS, KEYS_MASK};

pub(super) struct LuaBackend {
    _lua: mlua::Lua,
    on_tick: mlua::Function,
    on_reset: Option<mlua::Function>,
    ctx: mlua::Table,
    budget: Arc<AtomicU32>,
    dummy_index: u8,
}

impl LuaBackend {
    pub(super) fn load(name: &str, source: &str, dummy_index: u8, host: Arc<HostState>) -> anyhow::Result<Self> {
        // Base only plus pure stdlibs: no io/os/package, so the sandbox
        // can't touch the machine; load/dofile/loadfile are nil'd below
        // (code is data here), and math.random with them (determinism —
        // rand()/rand_int() are the seeded replacements).
        let lua = mlua::Lua::new_with(
            mlua::StdLib::TABLE | mlua::StdLib::STRING | mlua::StdLib::MATH,
            mlua::LuaOptions::default(),
        )?;
        {
            let globals = lua.globals();
            for name in ["load", "dofile", "loadfile", "collectgarbage"] {
                globals.set(name, mlua::Nil)?;
            }
            let math: mlua::Table = globals.get("math")?;
            math.set("random", mlua::Nil)?;
            math.set("randomseed", mlua::Nil)?;

            let k = lua.create_table()?;
            for (name, bit) in KEYS {
                k.set(*name, *bit)?;
            }
            globals.set("K", k)?;

            let h = host.clone();
            globals.set(
                "read8",
                lua.create_function(move |_, addr: u32| h.read8(addr).map_err(mlua::Error::external))?,
            )?;
            let h = host.clone();
            globals.set(
                "read16",
                lua.create_function(move |_, addr: u32| h.read16(addr).map_err(mlua::Error::external))?,
            )?;
            let h = host.clone();
            globals.set(
                "read32",
                lua.create_function(move |_, addr: u32| h.read32(addr).map_err(mlua::Error::external))?,
            )?;
            let h = host.clone();
            globals.set("rand", lua.create_function(move |_, ()| Ok(h.rand()))?)?;
            let h = host.clone();
            globals.set(
                "rand_int",
                lua.create_function(move |_, (lo, hi): (i64, i64)| h.rand_int(lo, hi).map_err(mlua::Error::external))?,
            )?;
            let log_fn = lua.create_function(|_, msg: String| {
                log::info!("training script: {msg}");
                Ok(())
            })?;
            globals.set("log", log_fn.clone())?;
            globals.set("print", log_fn)?;
        }

        // The budget hook survives for the backend's whole life; the
        // per-callback counter reset is what scopes it to one call.
        let budget = Arc::new(AtomicU32::new(0));
        {
            let budget = budget.clone();
            lua.set_hook(
                mlua::HookTriggers {
                    every_nth_instruction: Some(BUDGET_SLICE),
                    ..Default::default()
                },
                move |_, _| {
                    if budget.fetch_add(1, Ordering::Relaxed) >= BUDGET_SLICES {
                        Err(mlua::Error::RuntimeError(
                            "instruction budget exceeded (infinite loop?)".to_string(),
                        ))
                    } else {
                        Ok(mlua::VmState::Continue)
                    }
                },
            )?;
        }

        lua.load(source).set_name(name).exec()?;

        let on_tick: mlua::Function = lua
            .globals()
            .get::<Option<mlua::Function>>("on_tick")?
            .ok_or_else(|| anyhow::anyhow!("script must define an on_tick function"))?;
        let on_reset: Option<mlua::Function> = lua.globals().get("on_reset")?;
        // One ctx table, reused every tick — scripts see fresh values, we
        // skip a per-tick allocation.
        let ctx = lua.create_table()?;
        Ok(Self {
            _lua: lua,
            on_tick,
            on_reset,
            ctx,
            budget,
            dummy_index,
        })
    }

    fn ctx(&self, tick: u32, rep: u32) -> anyhow::Result<mlua::Table> {
        self.ctx.set("tick", tick)?;
        self.ctx.set("rep", rep)?;
        self.ctx.set("dummy_index", self.dummy_index)?;
        Ok(self.ctx.clone())
    }
}

impl ScriptBackend for LuaBackend {
    fn on_tick(&mut self, tick: u32, rep: u32) -> anyhow::Result<u16> {
        self.budget.store(0, Ordering::Relaxed);
        let joyflags = self.on_tick.call::<Option<i64>>(self.ctx(tick, rep)?)?;
        Ok(joyflags.unwrap_or(0) as u16 & KEYS_MASK)
    }

    fn on_reset(&mut self, tick: u32, rep: u32) -> anyhow::Result<()> {
        let Some(on_reset) = &self.on_reset else {
            return Ok(());
        };
        self.budget.store(0, Ordering::Relaxed);
        on_reset.call::<()>(self.ctx(tick, rep)?)?;
        Ok(())
    }
}
