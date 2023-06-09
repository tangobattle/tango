extern crate embed_resource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();

    if target_os == "windows" {
        match std::fs::metadata("resource.rc") {
            Ok(_) => {
                embed_resource::compile("resource.rc");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(Box::new(e));
            }
        }
    }

    Ok(())
}
