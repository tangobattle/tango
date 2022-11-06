extern crate embed_resource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut prost_config = prost_build::Config::new();
    prost_config.type_attribute("tango.replay.protos.replay11.Metadata", "#[derive(serde::Serialize)]");
    prost_config.type_attribute(
        "tango.replay.protos.replay11.Metadata.Side",
        "#[derive(serde::Serialize)]",
    );
    prost_config.type_attribute(
        "tango.replay.protos.replay11.Metadata.GameInfo",
        "#[derive(serde::Serialize)]",
    );
    prost_config.type_attribute(
        "tango.replay.protos.replay11.Metadata.GameInfo.Patch",
        "#[derive(serde::Serialize)]",
    );
    prost_config.compile_protos(
        &["src/replay/protos/replay11.proto", "src/replay/protos/replay10.proto"],
        &["src/"],
    )?;

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
