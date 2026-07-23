//! Shared test fixtures. Each test binary uses a different subset, so
//! anything unused in one of them isn't dead.
#![allow(dead_code)]

use tango_patch::{bundle::Builder, Manifest, RomTarget};

pub fn target(s: &str) -> RomTarget {
    s.parse().unwrap()
}

pub fn manifest(name: &str, version: &str, netplay: &str) -> Manifest {
    Manifest::parse(&format!(
        r#"
format = 1
name = "{name}"
version = "{version}"
title = "Test {name}"
authors = ["Someone <someone@example.com>"]
license = "MIT"
netplay = "{netplay}"
"#
    ))
    .unwrap()
}

/// A package with a README and one BPS, which is the minimum that passes
/// validation.
pub fn package(name: &str, version: &str, netplay: &str) -> Builder {
    let mut builder = Builder::new(manifest(name, version, netplay));
    builder.set_readme("# readme");
    builder.add_rom(target("BR6E_00"), b"bps".to_vec());
    builder
}

/// A scratch directory that cleans up on drop. The crate has no
/// dev-dependency on `tempfile`, and these tests only need "a fresh empty
/// directory that goes away afterwards".
pub struct TempDir(std::path::PathBuf);

impl TempDir {
    pub fn new() -> Self {
        // Distinct per call without pulling in a clock or rng: pid plus a
        // process-wide counter.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("tango-patch-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
