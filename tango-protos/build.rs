use std::io::Result;

fn main() -> Result<()> {
    prost_build::compile_protos(&["src/protos/matchmaking.proto"], &["src/"])?;
    Ok(())
}
