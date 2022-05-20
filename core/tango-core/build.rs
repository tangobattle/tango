extern crate winres;

use std::env;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    if target_os == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("tango.ico")
            .set_ar_path("x86_64-w64-mingw32-ar")
            .set_windres_path("x86_64-w64-mingw32-windres")
            .compile()
            .unwrap();
    }
}
