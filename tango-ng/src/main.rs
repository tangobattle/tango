//! Thin desktop entry point — the app lives in the library crate so a
//! future `android_main` can share it (see lib.rs `run`). Desktop goes
//! through the crash-supervisor trampoline (lib.rs `main`).

fn main() -> anyhow::Result<()> {
    tango_ng::main()
}
