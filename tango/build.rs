extern crate embed_resource;

fn generate_rc(icon_path: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR")?;
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR")?;
    let patch = std::env::var("CARGO_PKG_VERSION_PATCH")?;
    // The icon is produced by win/build.sh at release time and isn't checked
    // in, so emit the ICON resource only when it's present; the VERSIONINFO
    // block is always embedded.
    let icon = match icon_path {
        Some(path) => format!("1 ICON \"{path}\"\n\n"),
        None => String::new(),
    };
    Ok(format!(
        r#"#include "winver.h"

{icon}VS_VERSION_INFO VERSIONINFO
FILEVERSION    {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
BEGIN
BLOCK "StringFileInfo"
BEGIN
    BLOCK "040904b0"
    BEGIN
        VALUE "FileDescription", "Tango\0"
        VALUE "ProductVersion", "{major}.{minor}.{patch}.0\0"
        VALUE "FileVersion", "{major}.{minor}.{patch}.0\0"
        VALUE "OriginalFilename", "tango.exe\0"
        VALUE "Info", "https://tango.n1gp.net\0"
    END
END
BLOCK "VarFileInfo"
BEGIN
    VALUE "Translation", 0x0, 1200
END
END
"#
    ))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS")?;

    if target_os == "windows" {
        // Always embed a VERSIONINFO resource. `icon.ico` is produced by
        // `win/build.sh` at release time and isn't checked in, so the
        // taskbar icon is only included when present — local source builds
        // get the version info but no icon.
        let icon_file = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR")?).join("icon.ico");
        // Render `resource.rc` into OUT_DIR to keep the source tree clean.
        // Since the .rc no longer sits next to the icon, it references it by
        // absolute path with forward slashes (RC string literals treat `\`
        // as an escape; both rc.exe and windres accept `/`).
        let icon_path = icon_file
            .exists()
            .then(|| icon_file.to_string_lossy().replace('\\', "/"));
        let rc_path = std::path::Path::new(&std::env::var("OUT_DIR")?).join("resource.rc");
        std::fs::write(&rc_path, generate_rc(icon_path.as_deref())?)?;
        embed_resource::compile(&rc_path);
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
