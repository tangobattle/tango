//! Thin desktop entry point — the app lives in the library crate so a
//! future `android_main` can share it (see lib.rs `run`).

fn main() -> anyhow::Result<()> {
    tango_ng::run()
}
