//! The Rhai backend. See the parent module docs for the API surface it
//! binds. (Inside this module, `rhai::` paths resolve to the extern crate
//! — the module itself is only nameable from the parent.)

use std::sync::Arc;

use super::{HostState, ScriptBackend, BUDGET_SLICE, BUDGET_SLICES, KEYS, KEYS_MASK};

pub(super) struct RhaiBackend {
    engine: rhai::Engine,
    ast: rhai::AST,
    scope: rhai::Scope<'static>,
    /// Rhai functions are pure — they can't see top-level `let`s — so
    /// persistent script state lives in this map, bound as `this` on every
    /// callback (`this.foo = ...`). Lua scripts just use globals.
    state: rhai::Dynamic,
    has_on_reset: bool,
    has_on_setup: bool,
    dummy_index: u8,
}

/// Rhai's error type is unwieldy in anyhow chains — flatten to a message.
fn rhai_err(e: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

impl RhaiBackend {
    pub(super) fn load(name: &str, source: &str, dummy_index: u8, host: Arc<HostState>) -> anyhow::Result<Self> {
        let mut engine = rhai::Engine::new();
        engine.set_max_operations(BUDGET_SLICE as u64 * BUDGET_SLICES as u64);

        let h = host.clone();
        engine.register_fn("read8", move |addr: i64| -> Result<i64, Box<rhai::EvalAltResult>> {
            h.read8(addr as u32).map(|v| v as i64).map_err(|e| e.to_string().into())
        });
        let h = host.clone();
        engine.register_fn("read16", move |addr: i64| -> Result<i64, Box<rhai::EvalAltResult>> {
            h.read16(addr as u32).map(|v| v as i64).map_err(|e| e.to_string().into())
        });
        let h = host.clone();
        engine.register_fn("read32", move |addr: i64| -> Result<i64, Box<rhai::EvalAltResult>> {
            h.read32(addr as u32).map(|v| v as i64).map_err(|e| e.to_string().into())
        });
        let h = host.clone();
        engine.register_fn("rand", move || h.rand());
        let h = host.clone();
        engine.register_fn("rand_int", move |lo: i64, hi: i64| -> Result<i64, Box<rhai::EvalAltResult>> {
            h.rand_int(lo, hi).map_err(|e| e.to_string().into())
        });
        engine.register_fn("log", |msg: &str| log::info!("training script: {msg}"));
        for (name, size) in [("save_read8", 1usize), ("save_read16", 2), ("save_read32", 4)] {
            let h = host.clone();
            engine.register_fn(name, move |offset: i64| -> Result<i64, Box<rhai::EvalAltResult>> {
                h.save_read(offset, size).map_err(|e| e.to_string().into())
            });
        }
        for (name, size) in [("save_write8", 1usize), ("save_write16", 2), ("save_write32", 4)] {
            let h = host.clone();
            engine.register_fn(name, move |offset: i64, value: i64| -> Result<(), Box<rhai::EvalAltResult>> {
                h.save_write(offset, size, value).map_err(|e| e.to_string().into())
            });
        }
        let h = host.clone();
        engine.register_fn("save_len", move || -> Result<i64, Box<rhai::EvalAltResult>> {
            h.save_len().map_err(|e| e.to_string().into())
        });

        // Scope constants aren't visible inside rhai functions; a global
        // module is what makes `K` reachable from `on_tick`.
        let mut module = rhai::Module::new();
        let mut k = rhai::Map::new();
        for (name, bit) in KEYS {
            k.insert((*name).into(), rhai::Dynamic::from_int(*bit as i64));
        }
        module.set_var("K", k);
        engine.register_global_module(module.into());

        let mut scope = rhai::Scope::new();
        let ast = engine.compile(source).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        // Run the top level once (load-time validation/logging); the
        // callbacks below skip it via `eval_ast(false)`.
        engine
            .run_ast_with_scope(&mut scope, &ast)
            .map_err(|e| anyhow::anyhow!("{name}: {e}"))?;

        if !ast.iter_functions().any(|f| f.name == "on_tick") {
            anyhow::bail!("script must define an on_tick function");
        }
        let has_on_reset = ast.iter_functions().any(|f| f.name == "on_reset");
        let has_on_setup = ast.iter_functions().any(|f| f.name == "on_setup");
        Ok(Self {
            engine,
            ast,
            scope,
            state: rhai::Dynamic::from_map(rhai::Map::new()),
            has_on_reset,
            has_on_setup,
            dummy_index,
        })
    }

    fn ctx(&self, tick: u32, rep: u32) -> rhai::Map {
        let mut ctx = rhai::Map::new();
        ctx.insert("tick".into(), rhai::Dynamic::from_int(tick as i64));
        ctx.insert("rep".into(), rhai::Dynamic::from_int(rep as i64));
        ctx.insert("dummy_index".into(), rhai::Dynamic::from_int(self.dummy_index as i64));
        ctx
    }

    fn call(&mut self, name: &str, tick: u32, rep: u32) -> anyhow::Result<rhai::Dynamic> {
        let ctx = self.ctx(tick, rep);
        let options = rhai::CallFnOptions::new()
            .eval_ast(false)
            .bind_this_ptr(&mut self.state);
        self.engine
            .call_fn_with_options::<rhai::Dynamic>(options, &mut self.scope, &self.ast, name, (ctx,))
            .map_err(rhai_err)
    }
}

impl ScriptBackend for RhaiBackend {
    fn on_tick(&mut self, tick: u32, rep: u32) -> anyhow::Result<u16> {
        let joyflags = self.call("on_tick", tick, rep)?;
        if joyflags.is_unit() {
            return Ok(0);
        }
        let joyflags = joyflags
            .as_int()
            .map_err(|t| anyhow::anyhow!("on_tick returned {t}, expected an integer keys mask"))?;
        Ok(joyflags as u16 & KEYS_MASK)
    }

    fn on_reset(&mut self, tick: u32, rep: u32) -> anyhow::Result<()> {
        if !self.has_on_reset {
            return Ok(());
        }
        let _ = self.call("on_reset", tick, rep)?;
        Ok(())
    }

    fn has_setup(&self) -> bool {
        self.has_on_setup
    }

    fn on_setup(&mut self) -> anyhow::Result<()> {
        if !self.has_on_setup {
            return Ok(());
        }
        let _ = self.call("on_setup", 0, 0)?;
        Ok(())
    }
}
