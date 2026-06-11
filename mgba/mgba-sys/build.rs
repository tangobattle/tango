extern crate bindgen;

use std::env;
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// Pull the `-D…` flags cmake fed to the mgba C compile out of whatever
/// per-generator layout cmake produced this time:
///
/// * Makefile-style generators (NMake / Unix Makefiles / MinGW Makefiles)
///   write `build/CMakeFiles/mgba.dir/flags.make` with a literal
///   `C_DEFINES = -Dfoo -Dbar` line.
/// * Visual Studio generators write `build/mgba.vcxproj` (XML) with one
///   `<PreprocessorDefinitions>FOO;BAR;%(PreprocessorDefinitions)</…>`
///   element per build config. We grab the first non-empty one — the
///   defines don't differ meaningfully across Debug/Release for the
///   bindgen-visible header set.
fn extract_c_defines(build_dir: &Path) -> Option<Vec<String>> {
    let flags_make = build_dir.join("CMakeFiles").join("mgba.dir").join("flags.make");
    if flags_make.exists() {
        return extract_from_flags_make(&flags_make);
    }
    let vcxproj = build_dir.join("mgba.vcxproj");
    if vcxproj.exists() {
        return extract_from_vcxproj(&vcxproj);
    }
    None
}

fn extract_from_flags_make(path: &Path) -> Option<Vec<String>> {
    let file = std::fs::File::open(path).ok()?;
    let mut flags = None;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.ok()?;
        if let Some(rest) = line.strip_prefix("C_DEFINES = ") {
            flags = Some(shell_words::split(rest).ok()?);
        }
    }
    flags
}

fn extract_from_vcxproj(path: &Path) -> Option<Vec<String>> {
    // The vcxproj is one big XML blob; rather than dragging in a full
    // parser, slice between the first non-empty <PreprocessorDefinitions>
    // open/close tag pair. Configs share the same defines for mgba so
    // first-match is fine.
    let content = std::fs::read_to_string(path).ok()?;
    const OPEN: &str = "<PreprocessorDefinitions>";
    const CLOSE: &str = "</PreprocessorDefinitions>";
    let mut cursor = 0;
    while let Some(start) = content[cursor..].find(OPEN) {
        let abs_start = cursor + start + OPEN.len();
        let end = content[abs_start..].find(CLOSE)?;
        let raw = &content[abs_start..abs_start + end];
        let flags: Vec<String> = raw
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with("%("))
            .map(|s| format!("-D{s}"))
            .collect();
        if !flags.is_empty() {
            return Some(flags);
        }
        cursor = abs_start + end + CLOSE.len();
    }
    None
}

/// Extra preprocessor defines forced onto the mgba build.
///
/// `COLOR_16_BIT` switches mgba's `mColor` from 32-bit XBGR8 to the GBA-native
/// 15-bit BGR555 (no `COLOR_5_6_5`, so it stays BGR5, not RGB565), letting
/// tango do its own color conversion off the raw framebuffer.
///
/// These must reach BOTH the C compile (via cmake CFLAGS) and the bindgen pass
/// (via clang args) — if only one side sees them, `mColor`'s width disagrees
/// across the FFI boundary and the video buffer is silently misinterpreted.
const FORCED_DEFINES: &[&str] = &["COLOR_16_BIT"];

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    let mut cfg = cmake::Config::new("mgba");
    cfg.define("LIBMGBA_ONLY", "on");
    for def in FORCED_DEFINES {
        cfg.cflag(format!("-D{def}"));
    }

    let mgba_dst = cfg.build();

    // Makefile generators (NMake / Unix / MinGW) output directly under
    // `build/`; the Visual Studio generator buries artifacts in a
    // per-config subdir (`build/Release/` for cargo release builds).
    // Emit both so cargo's link-search picks up whichever actually
    // contains `mgba.lib` / `libmgba.a`.
    let build_dir = mgba_dst.join("build");
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    for config in ["Release", "Debug", "MinSizeRel", "RelWithDebInfo"] {
        println!("cargo:rustc-link-search=native={}/{}", build_dir.display(), config);
    }
    println!("cargo:rustc-link-lib=static=mgba");
    match target_os.as_str() {
        "macos" => {
            println!("cargo:rustc-link-lib=framework=Cocoa");
        }
        "windows" => {
            println!("cargo:rustc-link-lib=shlwapi");
            println!("cargo:rustc-link-lib=ole32");
            println!("cargo:rustc-link-lib=uuid");
        }
        "linux" => {}
        tos => panic!("unknown target os {:?}!", tos),
    }
    println!("cargo:rerun-if-changed=wrapper.h");
    // We emit explicit rerun-if-changed directives, which override cargo's
    // default of re-running on any package change — so track build.rs itself,
    // or edits to FORCED_DEFINES (e.g. toggling COLOR_16_BIT) won't take effect.
    println!("cargo:rerun-if-changed=build.rs");

    let build_dir = mgba_dst.join("build");
    let flags = extract_c_defines(&build_dir).expect("could not extract C_DEFINES from cmake build");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .blocklist_item("FP_INFINITE")
        .blocklist_item("FP_NAN")
        .blocklist_item("FP_NORMAL")
        .blocklist_item("FP_SUBNORMAL")
        .blocklist_item("FP_ZERO")
        .blocklist_item("FP_INT_UPWARD")
        .blocklist_item("FP_INT_DOWNWARD")
        .blocklist_item("FP_INT_TOWARDZERO")
        .blocklist_item("FP_INT_TONEARESTFROMZERO")
        .blocklist_item("FP_INT_TONEAREST")
        .blocklist_item("IPPORT_RESERVED")
        .clang_args(&["-Imgba/include", "-D__STDC_NO_THREADS__=1"])
        .clang_args(FORCED_DEFINES.iter().map(|def| format!("-D{def}")))
        .clang_args(flags)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
