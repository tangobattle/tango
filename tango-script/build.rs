use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=vendor/luau");
    println!("cargo:rerun-if-changed=src/checker/bridge.cpp");
    let root = Path::new("vendor/luau");
    let mut build = cc::Build::new();
    build.cpp(true).std("c++17").warnings(false);
    build.define("LUA_API", "extern \"C\"");
    build.define("LUACODE_API", "extern \"C\"");
    // Analysis executes user-defined type functions in mlua's Luau VM.
    // This setting changes the pseudo-indices in lua.h, so it must match
    // mlua-sys's VM build even though Analysis does not compile the VM itself.
    build.define("LUAI_MAXCSTACK", "1000000");
    for name in ["Analysis", "Common", "Ast", "Config", "Compiler", "VM", "Bytecode"] {
        build.include(root.join(name).join("include"));
    }
    let mut sources: Vec<_> = std::fs::read_dir(root.join("Analysis/src"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "cpp"))
        .collect();
    sources.sort();
    let mut flags = std::collections::BTreeSet::new();
    for source in &sources {
        let text = std::fs::read_to_string(source).unwrap();
        for tail in text.split("LUAU_FASTFLAGVARIABLE(").skip(1) {
            let name = tail.split(')').next().unwrap().trim();
            if name.starts_with("Luau") && name.chars().all(|c| c.is_ascii_alphanumeric()) {
                flags.insert(name.to_owned());
            }
        }
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(
        out.join("analysis_flags.h"),
        flags.iter().map(|name| format!("\"{name}\",\n")).collect::<String>(),
    )
    .unwrap();
    build.include(out);
    build
        .files(sources)
        .file("src/checker/bridge.cpp")
        .compile("tango_luau_analysis");
}
