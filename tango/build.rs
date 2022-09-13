extern crate embed_resource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    prost_build::compile_protos(&["src/replay/protos/replay11.proto"], &["src/"])?;

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();

    if target_os == "windows" {
        embed_resource::compile("resource.rc");
    }

    Ok(())
}
