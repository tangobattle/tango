extern crate embed_resource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();

    if target_os == "windows" {
        // Embed the Windows resource (icon + VERSIONINFO) into
        // the exe. `resource.rc` + `icon.ico` are produced by
        // the mako generator at release time and aren't
        // checked in, so missing-file = "skip silently" for
        // local source builds.
        match std::fs::metadata("resource.rc") {
            Ok(_) => {
                embed_resource::compile("resource.rc");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Box::new(e)),
        }
    } else if target_os == "macos" {
        // SDL3 (>= 3.4) uses `@available(macOS 26.0, *)` runtime checks in
        // SDL_cocoawindow.m, which clang lowers to calls to the compiler-rt
        // builtin `__isPlatformVersionAtLeast`. That symbol lives only in
        // libclang_rt.osx.a (not libSystem); when rustc drives the final
        // link it doesn't pull in clang's runtime, so the symbol is left
        // undefined. Link it explicitly. `clang -print-runtime-dir` resolves
        // the version-specific path, and libclang_rt.osx.a is a fat archive
        // so the same flag works for both the arm64 and x86_64 builds.
        let out = std::process::Command::new("clang").arg("-print-runtime-dir").output()?;
        if out.status.success() {
            let dir = String::from_utf8(out.stdout)?.trim().to_string();
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        }
    }

    Ok(())
}
