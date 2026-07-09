fn main() {
    // Embed images + fonts into the binary so the built app doesn't
    // depend on repo-relative asset paths at runtime (and so the same
    // setup carries to Android unchanged).
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new().embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .unwrap();
}
