extern crate embed_resource;

/// Ship the ordinary directory packages with the app. Generated data contains
/// only paths and bytes; package validation and dependency resolution remain in
/// the same runtime used for installed packages.
fn bundle_packages() -> Result<(), Box<dyn std::error::Error>> {
    use std::fmt::Write;
    use std::path::Path;

    fn files(root: &Path, path: &Path, output: &mut String) -> Result<(), Box<dyn std::error::Error>> {
        let mut entries = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_symlink() {
                return Err(format!("bundled package contains a symlink: {}", path.display()).into());
            }
            if kind.is_dir() {
                files(root, &path, output)?;
            } else if kind.is_file() {
                let relative = path
                    .strip_prefix(root)?
                    .to_str()
                    .ok_or("non-UTF-8 package path")?
                    .replace('\\', "/");
                writeln!(
                    output,
                    "({relative:?}, include_bytes!({:?})),",
                    path.to_str().ok_or("non-UTF-8 path")?
                )?;
            }
        }
        Ok(())
    }

    let root = Path::new(&std::env::var("CARGO_MANIFEST_DIR")?)
        .join("../packages")
        .canonicalize()?;
    println!("cargo:rerun-if-changed={}", root.display());
    let mut entries = std::fs::read_dir(&root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut output = String::from("&[\n");
    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() && path.join("package.toml").is_file() {
            output.push_str("&[\n");
            files(&path, &path, &mut output)?;
            output.push_str("],\n");
        }
    }
    output.push_str("]\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR")?).join("packages.rs"), output)?;
    Ok(())
}

fn generate_rc(icon_path: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR")?;
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR")?;
    let patch = std::env::var("CARGO_PKG_VERSION_PATCH")?;
    // The icon is produced by win/build.sh at release time and isn't checked
    // in, so emit the ICON resource only when it's present; the VERSIONINFO
    // block is always embedded.
    let icon = match icon_path {
        Some(path) => format!("1 ICON \"{path}\""),
        None => String::new(),
    };
    Ok(format!(
        r#"#include "winver.h"

{icon}

VS_VERSION_INFO VERSIONINFO
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
    bundle_packages()?;
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
