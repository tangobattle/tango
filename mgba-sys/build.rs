extern crate bindgen;

use std::env;
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
            "IPPORT_RESERVED".into(),
        ]
        .into_iter()
        .collect(),
    );

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .blacklist_type("LPMONITORINFOEXA?W?")
        .blacklist_type("LPTOP_LEVEL_EXCEPTION_FILTER")
        .blacklist_type("MONITORINFOEXA?W?")
        .blacklist_type("PEXCEPTION_FILTER")
        .blacklist_type("PEXCEPTION_ROUTINE")
        .blacklist_type("PSLIST_HEADER")
        .blacklist_type("PTOP_LEVEL_EXCEPTION_FILTER")
        .blacklist_type("PVECTORED_EXCEPTION_HANDLER")
        .blacklist_type("_?L?P?CONTEXT")
        .blacklist_type("_?L?P?EXCEPTION_POINTERS")
        .blacklist_type("_?P?DISPATCHER_CONTEXT")
        .blacklist_type("_?P?EXCEPTION_REGISTRATION_RECORD")
        .blacklist_type("_?P?IMAGE_TLS_DIRECTORY.*")
        .blacklist_type("_?P?NT_TIB")
        .blacklist_type("tagMONITORINFOEXA")
        .blacklist_type("tagMONITORINFOEXW")
        .blacklist_function("AddVectoredContinueHandler")
        .blacklist_function("AddVectoredExceptionHandler")
        .blacklist_function("CopyContext")
        .blacklist_function("GetThreadContext")
        .blacklist_function("GetXStateFeaturesMask")
        .blacklist_function("InitializeContext")
        .blacklist_function("InitializeContext2")
        .blacklist_function("InitializeSListHead")
        .blacklist_function("InterlockedFlushSList")
        .blacklist_function("InterlockedPopEntrySList")
        .blacklist_function("InterlockedPushEntrySList")
        .blacklist_function("InterlockedPushListSListEx")
        .blacklist_function("LocateXStateFeature")
        .blacklist_function("QueryDepthSList")
        .blacklist_function("RaiseFailFastException")
        .blacklist_function("RtlCaptureContext")
        .blacklist_function("RtlCaptureContext2")
        .blacklist_function("RtlFirstEntrySList")
        .blacklist_function("RtlInitializeSListHead")
        .blacklist_function("RtlInterlockedFlushSList")
        .blacklist_function("RtlInterlockedPopEntrySList")
        .blacklist_function("RtlInterlockedPushEntrySList")
        .blacklist_function("RtlInterlockedPushListSListEx")
        .blacklist_function("RtlQueryDepthSList")
        .blacklist_function("RtlRestoreContext")
        .blacklist_function("RtlUnwindEx")
        .blacklist_function("RtlVirtualUnwind")
        .blacklist_function("SetThreadContext")
        .blacklist_function("SetUnhandledExceptionFilter")
        .blacklist_function("SetXStateFeaturesMask")
        .blacklist_function("UnhandledExceptionFilter")
        .blacklist_function("__C_specific_handler")
        .clang_args(&["-Iexternal/mgba/include", "-D__STDC_NO_THREADS__=1"])
        // .parse_callbacks(Box::new(bindgen::CargoCallbacks)) // TODO: support this again
        .parse_callbacks(Box::new(ignored_macros))
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
