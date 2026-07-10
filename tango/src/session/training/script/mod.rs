//! The training dummy's brain: a user-authored script.
//!
//! Scripts are engine-agnostic — the file extension picks the backend
//! ([`lua`] for `.lua` via mlua, [`rhai`] for `.rhai`) and both bind the
//! same surface:
//!
//! - `on_tick(t)` (required): called once per tick with `t.tick` (ticks
//!   since the rep started), `t.rep` (reset count) and `t.dummy_index`;
//!   returns the dummy's mgba-keys bitmask for the tick (nothing/`()`/nil
//!   counts as neutral).
//! - `on_reset(t)` (optional): the drill point was restored — rewind any
//!   script state that tracks the abandoned rep.
//! - `on_setup(t)` (optional): define the dummy's save. Runs once, before
//!   the cores boot, against an **all-zeroes** save image — nothing of
//!   the player's own save leaks in, so the same script produces the
//!   same dummy on every machine. `save_write8/16/32(offset, value)`,
//!   `save_write_hex(offset, "AABB…")` (bulk; whitespace ignored — the
//!   usual shape is one embedded known-good save plus targeted patches),
//!   `save_read8/16/32(offset)` and `save_len()` address the save's
//!   native WRAM image. Tango rebuilds the checksum afterwards, so a
//!   script + a seed is a complete, shareable, deterministic drill: no
//!   save file travels with it. Picking (or reloading) a script that
//!   defines `on_setup` mid-session relaunches the match — the loadout
//!   crosses the link at battle start and can't be swapped under a live
//!   round.
//! - `read8(addr)` / `read16(addr)` / `read32(addr)`: read the game's
//!   memory through the live core's bus, valid inside callbacks only. The
//!   core sits at its settled pre-advance boundary, so the script sees the
//!   world as of input-submission time — the same deal the human gets.
//!   There are no curated accessors and no address tables here by design:
//!   scripts carry their own game knowledge.
//! - `K`: key constants (`K.A`, `K.B`, `K.SELECT`, `K.START`, `K.RIGHT`,
//!   `K.LEFT`, `K.UP`, `K.DOWN`, `K.R`, `K.L`), combined with `|`.
//! - `rand()` / `rand_int(lo, hi)`: a tango-side PCG, reseeded from the
//!   match seed + rep on every reset — reps vary reproducibly, and the
//!   engines' own RNGs (nondeterministic across runs) are blocked.
//! - `log(msg)`: writes to the tango log (Lua's `print` aliases it).
//!
//! Persistent state: Lua scripts use globals. Rhai functions are pure (no
//! closure over the top level), so a persistent map is bound as `this` on
//! every callback — `this.foo = ...` carries across ticks.
//!
//! The smallest useful script (`mash-a.lua`):
//!
//! ```lua
//! function on_tick(t)
//!   if t.tick % 40 == 0 then
//!     return K.A
//!   end
//!   return 0
//! end
//! ```
//!
//! Callbacks run on the emulator thread inside the primary trap fire, so
//! errors never propagate: a failing script is latched dead (neutral input,
//! message surfaced on the HUD) until the next reset or reload, and a
//! runaway one is aborted by an instruction/operation budget.

mod lua;
mod rhai;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use rand::{Rng, SeedableRng};

/// Bits above the GBA's ten keys are stripped from whatever the script
/// returns rather than erroring — a joyflags value is a mask, not a code.
const KEYS_MASK: u16 = 0x03ff;

pub const KEYS: &[(&str, u32)] = &[
    ("A", mgba::input::keys::A),
    ("B", mgba::input::keys::B),
    ("SELECT", mgba::input::keys::SELECT),
    ("START", mgba::input::keys::START),
    ("RIGHT", mgba::input::keys::RIGHT),
    ("LEFT", mgba::input::keys::LEFT),
    ("UP", mgba::input::keys::UP),
    ("DOWN", mgba::input::keys::DOWN),
    ("R", mgba::input::keys::R),
    ("L", mgba::input::keys::L),
];

/// Per-tick instruction budget for the Lua backend: the hook fires every
/// `BUDGET_SLICE` VM instructions and errors after `BUDGET_SLICES` fires
/// within one callback. Rhai's native operation budget is set to the
/// product. Generous — a behavior script is a few hundred operations.
const BUDGET_SLICE: u32 = 100_000;
const BUDGET_SLICES: u32 = 20;

/// The live core, published for the duration of one script callback.
///
/// The memory-read functions are registered on the script engines once, at
/// load, so they are `'static` closures — but the core they read arrives
/// borrowed into [`ScriptDummy::next_joyflags`] for a single trap fire.
/// This slot launders that lifetime: `publish` stores the (Copy, pointer-
/// sized) core ref and returns a guard that clears the slot on drop — error
/// and unwind paths included — so the pointer can never be observed outside
/// the callback it was published for. Reads outside a callback error.
#[derive(Clone, Default)]
struct CoreSlot(Arc<Mutex<Option<mgba::core::CoreMutRef<'static>>>>);

impl CoreSlot {
    fn publish(&self, core: mgba::core::CoreMutRef<'_>) -> CoreSlotGuard {
        *self.0.lock().unwrap() = Some(unsafe {
            std::mem::transmute::<mgba::core::CoreMutRef<'_>, mgba::core::CoreMutRef<'static>>(core)
        });
        CoreSlotGuard(self.clone())
    }

    fn with<R>(&self, f: impl FnOnce(&mut mgba::core::CoreMutRef<'static>) -> R) -> anyhow::Result<R> {
        let mut slot = self.0.lock().unwrap();
        let core = slot
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("memory reads are only valid inside a script callback"))?;
        Ok(f(core))
    }
}

struct CoreSlotGuard(CoreSlot);

impl Drop for CoreSlotGuard {
    fn drop(&mut self) {
        *self.0 .0.lock().unwrap() = None;
    }
}

/// State shared between a backend's registered functions and the
/// [`ScriptDummy`] driving it: the published core, the script-visible
/// RNG, and — during `on_setup` only — the dummy's save being defined.
/// One per loaded script.
struct HostState {
    core: CoreSlot,
    rng: Mutex<rand_pcg::Mcg128Xsl64>,
    /// The dummy's save, present only while `on_setup` runs (see
    /// [`LoadedScript::run_setup`]); the `save_*` functions error at any
    /// other time.
    save: Mutex<Option<Box<dyn tango_dataview::save::Save + Send + Sync>>>,
}

impl HostState {
    fn new(seed: [u8; 16]) -> Arc<Self> {
        Arc::new(Self {
            core: CoreSlot::default(),
            rng: Mutex::new(rand_pcg::Mcg128Xsl64::from_seed(seed)),
            save: Mutex::new(None),
        })
    }

    /// Run `f` over the setup save's native WRAM image. Errors outside
    /// `on_setup` and on saves that don't support raw editing.
    fn with_save_wram<R>(&self, f: impl FnOnce(&mut [u8]) -> anyhow::Result<R>) -> anyhow::Result<R> {
        let mut slot = self.save.lock().unwrap();
        let save = slot
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("save access is only valid inside on_setup"))?;
        let wram = save
            .as_raw_wram_mut()
            .ok_or_else(|| anyhow::anyhow!("this game's save doesn't support raw editing"))?;
        f(wram)
    }

    fn save_len(&self) -> anyhow::Result<i64> {
        self.with_save_wram(|wram| Ok(wram.len() as i64))
    }

    fn save_read(&self, offset: i64, size: usize) -> anyhow::Result<i64> {
        self.with_save_wram(|wram| {
            let Some(idx) = usize::try_from(offset).ok().filter(|o| o + size <= wram.len()) else {
                anyhow::bail!("save read out of range: {offset:#x}+{size}");
            };
            let mut v: i64 = 0;
            for (i, b) in wram[idx..idx + size].iter().enumerate() {
                v |= (*b as i64) << (i * 8);
            }
            Ok(v)
        })
    }

    fn save_write_hex(&self, offset: i64, hex: &str) -> anyhow::Result<()> {
        let digits: Vec<u8> = hex
            .bytes()
            .filter(|b| !b.is_ascii_whitespace())
            .map(|b| {
                (b as char)
                    .to_digit(16)
                    .map(|d| d as u8)
                    .ok_or_else(|| anyhow::anyhow!("save_write_hex: not a hex digit: {:?}", b as char))
            })
            .collect::<anyhow::Result<_>>()?;
        if digits.len() % 2 != 0 {
            anyhow::bail!("save_write_hex: odd number of hex digits");
        }
        let bytes: Vec<u8> = digits.chunks(2).map(|d| (d[0] << 4) | d[1]).collect();
        self.with_save_wram(|wram| {
            let Some(idx) = usize::try_from(offset).ok().filter(|o| o + bytes.len() <= wram.len()) else {
                anyhow::bail!("save write out of range: {offset:#x}+{}", bytes.len());
            };
            wram[idx..idx + bytes.len()].copy_from_slice(&bytes);
            Ok(())
        })
    }

    fn save_write(&self, offset: i64, size: usize, value: i64) -> anyhow::Result<()> {
        self.with_save_wram(|wram| {
            let Some(idx) = usize::try_from(offset).ok().filter(|o| o + size <= wram.len()) else {
                anyhow::bail!("save write out of range: {offset:#x}+{size}");
            };
            for (i, b) in wram[idx..idx + size].iter_mut().enumerate() {
                *b = (value >> (i * 8)) as u8;
            }
            Ok(())
        })
    }

    fn read8(&self, addr: u32) -> anyhow::Result<u8> {
        self.core.with(|core| core.raw_read_8(addr, -1))
    }

    fn read16(&self, addr: u32) -> anyhow::Result<u16> {
        self.core.with(|core| core.raw_read_16(addr, -1))
    }

    fn read32(&self, addr: u32) -> anyhow::Result<u32> {
        self.core.with(|core| core.raw_read_32(addr, -1))
    }

    fn rand(&self) -> f64 {
        self.rng.lock().unwrap().gen_range(0.0..1.0)
    }

    fn rand_int(&self, lo: i64, hi: i64) -> anyhow::Result<i64> {
        if lo > hi {
            anyhow::bail!("rand_int: empty range {lo}..={hi}");
        }
        Ok(self.rng.lock().unwrap().gen_range(lo..=hi))
    }

    fn reseed(&self, seed: [u8; 16]) {
        *self.rng.lock().unwrap() = rand_pcg::Mcg128Xsl64::from_seed(seed);
    }
}

/// One scripting backend, behind which language the script happens to be
/// written in. Implementations bind the shared API surface (module docs) at
/// load and hold their engine + compiled script.
trait ScriptBackend: Send {
    fn on_tick(&mut self, tick: u32, rep: u32) -> anyhow::Result<u16>;
    fn on_reset(&mut self, tick: u32, rep: u32) -> anyhow::Result<()>;
    /// Whether the script defines `on_setup` — decides between a live
    /// hot-swap and a session relaunch when it's picked.
    fn has_setup(&self) -> bool;
    /// Run `on_setup`. The save is reachable through the host's slot;
    /// [`LoadedScript::run_setup`] is the owner of that dance.
    fn on_setup(&mut self) -> anyhow::Result<()>;
}

/// A loaded, ready-to-run script: the backend plus the host state its
/// registered functions share. Feed it to [`ScriptDummy::new`].
pub struct LoadedScript {
    backend: Box<dyn ScriptBackend>,
    host: Arc<HostState>,
}

impl LoadedScript {
    /// Whether the script defines `on_setup` (a save-defining drill).
    pub fn has_setup(&self) -> bool {
        self.backend.has_setup()
    }

    /// Run `on_setup` over `save`, rebuild its checksum, and hand it
    /// back. The image is **zeroed first**: the script is the sole source
    /// of the dummy's identity, so the same script yields the same dummy
    /// on every machine — a save that merely mirrored the player's own
    /// would make shared drills nondeterministic. The save rides the host
    /// slot for the duration so the registered `save_*` functions can
    /// reach it; it comes back out on every path, error included.
    pub fn run_setup(
        &mut self,
        mut save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    ) -> (Box<dyn tango_dataview::save::Save + Send + Sync>, anyhow::Result<()>) {
        if let Some(wram) = save.as_raw_wram_mut() {
            wram.fill(0);
        } else {
            return (save, Err(anyhow::anyhow!("this game's save doesn't support raw editing")));
        }
        *self.host.save.lock().unwrap() = Some(save);
        let result = self.backend.on_setup();
        let mut save = self.host.save.lock().unwrap().take().expect("setup save");
        if result.is_ok() {
            save.rebuild_checksum();
        }
        (save, result)
    }
}

/// Compile `source` under the backend `name`'s extension picks (`.lua` /
/// `.rhai`), run its top level, and require an `on_tick` function. `seed`
/// is the match seed; the script RNG re-derives from it per rep.
pub fn load_script(name: &str, source: &str, dummy_index: u8, seed: [u8; 16]) -> anyhow::Result<LoadedScript> {
    let host = HostState::new(rep_seed(seed, 0));
    let backend: Box<dyn ScriptBackend> = match name.rsplit('.').next() {
        Some("lua") => Box::new(lua::LuaBackend::load(name, source, dummy_index, host.clone())?),
        Some("rhai") => Box::new(rhai::RhaiBackend::load(name, source, dummy_index, host.clone())?),
        _ => anyhow::bail!("unsupported script extension: {name} (expected .lua or .rhai)"),
    };
    Ok(LoadedScript { backend, host })
}

/// Fold the rep counter into the match seed so each rep draws a fresh but
/// reproducible stream: same seed text + same rep = same draws, every time.
/// Spread across the high half — the MCG fixes the seed's low bits (its
/// state must be odd), so a small XOR down there would be discarded.
fn rep_seed(mut seed: [u8; 16], rep: u32) -> [u8; 16] {
    let fold = (rep as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for (b, r) in seed[8..].iter_mut().zip(fold.to_le_bytes()) {
        *b ^= r;
    }
    seed
}

/// Status the session/HUD reads out of a running dummy: the latched script
/// error (if any) and the joyflags last returned, for the input display.
#[derive(Default)]
pub struct ScriptStatus {
    pub error: Mutex<Option<String>>,
    pub last_joyflags: AtomicU32,
}

/// The [`TrainingRemoteSource`](tango_pvp::battle::TrainingRemoteSource)
/// installed on the match: owns the loaded script, tracks the rep/tick
/// counters, publishes the core around callbacks, and latches errors dead
/// (neutral input) until the next reset gives the script another chance.
///
/// `reset_epoch` mirrors the session's rep counter (applied resets + real
/// round ends) — the session's frame callback stores it, and the first
/// `next_joyflags` after a bump notices the change, reseeds the RNG, and
/// fires `on_reset`.
pub struct ScriptDummy {
    backend: Box<dyn ScriptBackend>,
    host: Arc<HostState>,
    seed: [u8; 16],
    tick: u32,
    rep: u32,
    dead: bool,
    reset_epoch: Arc<AtomicU32>,
    status: Arc<ScriptStatus>,
}

impl ScriptDummy {
    pub fn new(
        script: LoadedScript,
        seed: [u8; 16],
        reset_epoch: Arc<AtomicU32>,
        status: Arc<ScriptStatus>,
    ) -> Self {
        Self {
            backend: script.backend,
            host: script.host,
            seed,
            tick: 0,
            rep: reset_epoch.load(Ordering::Acquire),
            dead: false,
            reset_epoch,
            status,
        }
    }

    fn fail(&mut self, e: anyhow::Error) {
        log::warn!("training script error: {e:#}");
        self.dead = true;
        *self.status.error.lock().unwrap() = Some(format!("{e:#}"));
        self.status.last_joyflags.store(0, Ordering::Relaxed);
    }
}

impl tango_pvp::battle::TrainingRemoteSource for ScriptDummy {
    fn next_joyflags(&mut self, core: mgba::core::CoreMutRef<'_>) -> u16 {
        let _guard = self.host.core.publish(core);
        let epoch = self.reset_epoch.load(Ordering::Acquire);
        if epoch != self.rep {
            self.rep = epoch;
            self.tick = 0;
            self.host.reseed(rep_seed(self.seed, epoch));
            // A reset is a fresh rep: un-latch a dead script so an edit-free
            // retry (or a transient bad state read) gets another chance.
            self.dead = false;
            *self.status.error.lock().unwrap() = None;
            if let Err(e) = self.backend.on_reset(0, epoch) {
                self.fail(e);
            }
        }
        if self.dead {
            return 0;
        }
        let tick = self.tick;
        self.tick += 1;
        match self.backend.on_tick(tick, self.rep) {
            Ok(joyflags) => {
                self.status.last_joyflags.store(joyflags as u32, Ordering::Relaxed);
                joyflags
            }
            Err(e) => {
                self.fail(e);
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 16] = *b"0123456789abcdef";

    fn load(name: &str, source: &str) -> LoadedScript {
        load_script(name, source, 1, SEED).unwrap()
    }

    /// Raw-editable stand-in for a game save. `rebuild_checksum` stamps a
    /// marker byte so the tests can see it ran through the `dyn Save`.
    #[derive(Clone)]
    struct FakeSave {
        wram: Vec<u8>,
    }

    impl tango_dataview::save::Save for FakeSave {
        fn to_sram_dump(&self) -> Vec<u8> {
            self.wram.clone()
        }

        fn as_raw_wram(&self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Borrowed(&self.wram)
        }

        fn as_raw_wram_mut(&mut self) -> Option<&mut [u8]> {
            Some(&mut self.wram)
        }

        fn rebuild_checksum(&mut self) {
            self.wram[0] = 0xCC;
        }
    }

    fn fake_save() -> Box<dyn tango_dataview::save::Save + Send + Sync> {
        Box::new(FakeSave { wram: vec![7; 0x100] })
    }

    #[test]
    fn setup_edits_save_and_rebuilds_checksum() {
        for (name, src) in [
            (
                "t.lua",
                "function on_setup(t)\n  save_write8(0x10, save_read8(0x10) + 1)\n  save_write16(0x20, 0xBEEF)\n  save_write_hex(0x30, \"de AD\\nbe ef\")\nend\nfunction on_tick(t)\n  return 0\nend",
            ),
            (
                "t.rhai",
                "fn on_setup(t) {\n  save_write8(0x10, save_read8(0x10) + 1);\n  save_write16(0x20, 0xBEEF);\n  save_write_hex(0x30, \"de AD\\nbe ef\");\n}\nfn on_tick(t) {\n  0\n}",
            ),
        ] {
            let mut s = load(name, src);
            assert!(s.has_setup(), "{name}");
            let (save, result) = s.run_setup(fake_save());
            result.unwrap();
            let dump = save.to_sram_dump();
            // The base is zeroed — the fake's 7-fill must not survive
            // into the script's view (shared drills are deterministic).
            assert_eq!(dump[0x10], 1, "{name}");
            assert_eq!(dump[0x11], 0, "{name}");
            assert_eq!(&dump[0x20..0x22], &[0xEF, 0xBE], "{name}");
            // Bulk hex, case- and whitespace-tolerant.
            assert_eq!(&dump[0x30..0x34], &[0xDE, 0xAD, 0xBE, 0xEF], "{name}");
            // rebuild_checksum ran after the edits.
            assert_eq!(dump[0], 0xCC, "{name}");
        }
    }

    #[test]
    fn setup_absent_and_failing() {
        // No on_setup: has_setup is false and run_setup is a no-op pass.
        let mut s = load("t.lua", "function on_tick(t) return 0 end");
        assert!(!s.has_setup());
        // An out-of-range write errors and the save still comes back,
        // un-checksummed (the drill failed; don't bless the result).
        let mut s = load(
            "t.lua",
            "function on_setup(t)\n  save_write8(save_len(), 1)\nend\nfunction on_tick(t)\n  return 0\nend",
        );
        let (save, result) = s.run_setup(fake_save());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("out of range"), "{err}");
        // Zeroed base, un-checksummed (the drill failed; don't bless it).
        assert_eq!(save.to_sram_dump()[0], 0);
    }

    #[test]
    fn save_access_outside_setup_errors() {
        let mut s = load(
            "t.lua",
            "function on_tick(t)\n  return save_read8(0)\nend",
        );
        let err = s.backend.on_tick(0, 0).unwrap_err().to_string();
        assert!(err.contains("on_setup"), "{err}");
    }

    #[test]
    fn lua_tick_mask_and_ctx() {
        let mut s = load(
            "t.lua",
            "function on_tick(t)\n  if t.tick % 2 == 0 then return K.A | K.RIGHT end\n  return 0x10000 | K.B\nend",
        );
        assert_eq!(s.backend.on_tick(0, 0).unwrap(), 0x011);
        // Out-of-range bits are masked off, not an error.
        assert_eq!(s.backend.on_tick(1, 0).unwrap(), 0x002);
    }

    #[test]
    fn rhai_tick_mask_and_ctx() {
        let mut s = load(
            "t.rhai",
            "fn on_tick(t) {\n  if t.tick % 2 == 0 { return K.A | K.RIGHT; }\n  0x10000 | K.B\n}",
        );
        assert_eq!(s.backend.on_tick(0, 0).unwrap(), 0x011);
        assert_eq!(s.backend.on_tick(1, 0).unwrap(), 0x002);
    }

    #[test]
    fn lua_nil_return_is_neutral() {
        let mut s = load("t.lua", "function on_tick(t)\nend");
        assert_eq!(s.backend.on_tick(0, 0).unwrap(), 0);
    }

    #[test]
    fn rhai_unit_return_is_neutral() {
        let mut s = load("t.rhai", "fn on_tick(t) {\n}");
        assert_eq!(s.backend.on_tick(0, 0).unwrap(), 0);
    }

    #[test]
    fn missing_on_tick_rejected() {
        assert!(load_script("t.lua", "x = 1", 1, SEED).is_err());
        assert!(load_script("t.rhai", "let x = 1;", 1, SEED).is_err());
        assert!(load_script("t.txt", "hello", 1, SEED).is_err());
    }

    #[test]
    fn lua_budget_aborts_infinite_loop() {
        let mut s = load("t.lua", "function on_tick(t)\n  while true do end\nend");
        let err = s.backend.on_tick(0, 0).unwrap_err().to_string();
        assert!(err.contains("instruction budget"), "{err}");
        // The budget is per callback, so a recovered script keeps running —
        // the counter must reset.
        let mut s = load("t.lua", "function on_tick(t) return K.A end");
        for tick in 0..100 {
            assert_eq!(s.backend.on_tick(tick, 0).unwrap(), 0x001);
        }
    }

    #[test]
    fn rhai_budget_aborts_infinite_loop() {
        let mut s = load("t.rhai", "fn on_tick(t) {\n  loop { }\n}");
        assert!(s.backend.on_tick(0, 0).is_err());
    }

    #[test]
    fn memory_reads_error_outside_callback_core() {
        // No core is ever published in unit tests, so the reads must
        // surface the slot error instead of dereferencing anything.
        let mut s = load("t.lua", "function on_tick(t)\n  return read8(0x02000000)\nend");
        let err = s.backend.on_tick(0, 0).unwrap_err().to_string();
        assert!(err.contains("script callback"), "{err}");
        let mut s = load("t.rhai", "fn on_tick(t) {\n  read8(0x02000000)\n}");
        assert!(s.backend.on_tick(0, 0).is_err());
    }

    #[test]
    fn rng_is_deterministic_and_rep_seeded() {
        let script = "function on_tick(t)\n  return rand_int(0, 1023)\nend";
        let mut a = load("t.lua", script);
        let mut b = load("t.lua", script);
        let draws_a: Vec<u16> = (0..32).map(|t| a.backend.on_tick(t, 0).unwrap()).collect();
        let draws_b: Vec<u16> = (0..32).map(|t| b.backend.on_tick(t, 0).unwrap()).collect();
        assert_eq!(draws_a, draws_b);

        // Reseeding for a different rep changes the stream; reseeding for
        // the same rep replays it. (Same via ScriptDummy in live use.)
        a.host.reseed(rep_seed(SEED, 1));
        let rep1: Vec<u16> = (0..32).map(|t| a.backend.on_tick(t, 1).unwrap()).collect();
        assert_ne!(rep1, draws_a);
        a.host.reseed(rep_seed(SEED, 0));
        let rep0: Vec<u16> = (0..32).map(|t| a.backend.on_tick(t, 0).unwrap()).collect();
        assert_eq!(rep0, draws_a);
    }

    #[test]
    fn lua_sandbox_is_sealed() {
        for expr in ["io", "os", "package", "require", "load", "dofile", "math.random"] {
            let mut s = load(
                "t.lua",
                &format!("function on_tick(t)\n  if {expr} == nil then return K.A end\n  return 0\nend"),
            );
            assert_eq!(s.backend.on_tick(0, 0).unwrap(), 0x001, "{expr} is reachable");
        }
    }

    #[test]
    fn rhai_this_state_persists_across_ticks() {
        let mut s = load(
            "t.rhai",
            "fn on_tick(t) {\n  this.presses = if \"presses\" in this { this.presses + 1 } else { 1 };\n  if this.presses > 2 { K.A } else { 0 }\n}",
        );
        assert_eq!(s.backend.on_tick(0, 0).unwrap(), 0);
        assert_eq!(s.backend.on_tick(1, 0).unwrap(), 0);
        assert_eq!(s.backend.on_tick(2, 0).unwrap(), 0x001);
    }

    #[test]
    fn lua_on_reset_optional_and_called() {
        let mut s = load(
            "t.lua",
            "resets = 0\nfunction on_tick(t)\n  return resets\nend\nfunction on_reset(t)\n  resets = resets + 1\nend",
        );
        assert_eq!(s.backend.on_tick(0, 0).unwrap(), 0);
        s.backend.on_reset(0, 1).unwrap();
        assert_eq!(s.backend.on_tick(0, 1).unwrap(), 1);
        // No on_reset defined is fine.
        let mut s = load("t.lua", "function on_tick(t) return 0 end");
        s.backend.on_reset(0, 1).unwrap();
    }
}
