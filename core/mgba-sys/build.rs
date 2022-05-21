extern crate bindgen;

use std::env;
use std::io::BufRead;
use std::path::PathBuf;

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

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    let mgba_dst = cmake::Config::new("external/mgba")
        .define("LIBMGBA_ONLY", "on")
        .build();

    println!(
        "cargo:rustc-link-search=native={}/build",
        mgba_dst.display()
    );
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

    let mut flags = vec![];
    for line in std::io::BufReader::new(flags_file).lines() {
        flags = if let Some(rest) = line.unwrap().strip_prefix("C_DEFINES = ") {
            shell_words::split(rest).unwrap()
        } else {
            continue;
        }
    }

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_args(&["-Iexternal/mgba/include", "-D__STDC_NO_THREADS__=1"])
        .clang_args(&flags)
        // .parse_callbacks(Box::new(bindgen::CargoCallbacks)) // TODO: support this again
        .parse_callbacks(Box::new(ignored_macros))
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
