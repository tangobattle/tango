use std::io::Result;

fn main() -> Result<()> {
    prost_build::compile_protos(
        &["src/protos/signaling.proto", "src/protos/iceconfig.proto"],
        &["src/"],
    )?;
    Ok(())
}
