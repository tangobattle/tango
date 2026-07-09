//! Thin desktop entry point — the app lives in the library crate so
//! Android's `android_main` shares it (see lib.rs). Desktop goes
//! through the crash-supervisor trampoline (lib.rs `main`); on mobile
//! this bin target is inert (the APK loads the cdylib instead).

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn main() -> anyhow::Result<()> {
    tango_ng::main()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn main() {}
