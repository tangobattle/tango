extern crate winres;

use std::env;

fn main() {
    prost_build::compile_protos(&["src/protos/ipc.proto"], &["src/"]).unwrap();

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    if target_os == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("tango.ico")
            .set_ar_path("x86_64-w64-mingw32-ar")
            .set_windres_path("x86_64-w64-mingw32-windres")
            .compile()
            .unwrap();
    }
    println!("cargo:rustc-link-search=external/sdl2");
}
