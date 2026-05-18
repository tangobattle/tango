extern crate bindgen;

use std::env;
use std::io::BufRead;
use std::path::PathBuf;
use std::process::Command;

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

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    apply_patches();

    let mut cfg = cmake::Config::new("mgba");
    cfg.define("LIBMGBA_ONLY", "on");

    let mgba_dst = cfg.build();

    println!("cargo:rustc-link-search=native={}/build", mgba_dst.display());
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

    let flags_file = std::fs::File::open(
        mgba_dst
            .join("build")
            .join("CMakeFiles")
            .join("mgba.dir")
            .join("flags.make"),
    )
    .unwrap();

    let mut flags = None;
    for line in std::io::BufReader::new(flags_file).lines() {
        flags = Some(if let Some(rest) = line.unwrap().strip_prefix("C_DEFINES = ") {
            shell_words::split(rest).unwrap()
        } else {
            continue;
        })
    }

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_args(&["-Imgba/include", "-D__STDC_NO_THREADS__=1"])
        .clang_args(flags.unwrap())
        .blocklist_item("__mingw_ldbl_type_t")
        // .parse_callbacks(Box::new(bindgen::CargoCallbacks)) // TODO: support this again
        .parse_callbacks(Box::new(ignored_macros))
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
