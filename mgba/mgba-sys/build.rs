extern crate bindgen;

use std::env;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::Command;

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

#[derive(Debug)]
struct IgnoreMacros(std::collections::HashSet<String>);

impl bindgen::callbacks::ParseCallbacks for IgnoreMacros {
    fn will_parse_macro(&self, name: &str) -> bindgen::callbacks::MacroParsingBehavior {
        if self.0.contains(name) {
            bindgen::callbacks::MacroParsingBehavior::Ignore
        } else {
            bindgen::callbacks::MacroParsingBehavior::Default
        }
    }
}

/// Apply every `.patch` under `mgba-sys/patches/` to the vendored mgba
/// submodule tree, skipping any that's already applied. Uses `git apply`
/// (assumed available — the submodule itself needs git to fetch). The
/// patches sit on top of the submodule's pinned commit; on a submodule
/// bump they'll either still apply or need regenerating against the new
/// upstream.
fn apply_patches() {
    // Absolute paths without Path::canonicalize, which on Windows returns a
    // `\\?\` UNC-prefixed form that `git.exe` can't parse.
    let crate_dir = env::current_dir().expect("current_dir");
    let mgba_dir = crate_dir.join("mgba");
    let patches_dir = crate_dir.join("patches");
    if !patches_dir.exists() {
        return;
    }

    let mut entries: Vec<_> = std::fs::read_dir(&patches_dir)
        .expect("read patches/")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("patch"))
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let patch_path = entry.path();
        println!("cargo:rerun-if-changed={}", patch_path.display());

        // `git apply --reverse --check` succeeds iff the forward patch is
        // currently applied (we could undo it). If it succeeds the patch is
        // already in place — nothing to do.
        let already_applied = Command::new("git")
            .args(["apply", "--reverse", "--check"])
            .arg(&patch_path)
            .current_dir(&mgba_dir)
            .status()
            .expect("git apply --reverse --check")
            .success();
        if already_applied {
            continue;
        }

        // Apply forward. Fails loudly so a botched patch (e.g., after a
        // submodule bump) doesn't silently produce a build with the wrong
        // mgba behavior.
        let status = Command::new("git")
            .args(["apply"])
            .arg(&patch_path)
            .current_dir(&mgba_dir)
            .status()
            .expect("git apply");
        assert!(status.success(), "failed to apply patch {}", patch_path.display());
    }
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

    apply_patches();

    let mut cfg = cmake::Config::new("mgba");
    cfg.define("LIBMGBA_ONLY", "on");
    // The CPU dynarec (mgba's ENABLE_DYNAREC) defaults on for x86_64/arm64
    // hosts and silently compiles out elsewhere; pin it on explicitly so a
    // change of upstream defaults can't flip it without us noticing. Cores
    // opt in at runtime via Core::set_dynarec_enabled.
    cfg.define("ENABLE_DYNAREC", "on");
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
    let ignored_macros = IgnoreMacros(
        vec![
            "FP_INFINITE".into(),
            "FP_NAN".into(),
            "FP_NORMAL".into(),
            "FP_SUBNORMAL".into(),
            "FP_ZERO".into(),
            "FP_INT_UPWARD".into(),
            "FP_INT_DOWNWARD".into(),
            "FP_INT_TOWARDZERO".into(),
            "FP_INT_TONEARESTFROMZERO".into(),
            "FP_INT_TONEAREST".into(),
            "IPPORT_RESERVED".into(),
        ]
        .into_iter()
        .collect(),
    );

    let build_dir = mgba_dst.join("build");
    let flags = extract_c_defines(&build_dir).expect("could not extract C_DEFINES from cmake build");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_args(&["-Imgba/include", "-D__STDC_NO_THREADS__=1"])
        .clang_args(FORCED_DEFINES.iter().map(|def| format!("-D{def}")))
        .clang_args(flags)
        // .parse_callbacks(Box::new(bindgen::CargoCallbacks)) // TODO: support this again
        .parse_callbacks(Box::new(ignored_macros))
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
