use typescript_type_def::TypeDef;

fn main() -> anyhow::Result<()> {
    typescript_type_def::write_definition_file_from_type_infos(
        std::io::stdout(),
        typescript_type_def::DefinitionFileOptions {
            header: None,
            root_namespace: None,
        },
        &[
            &tango_core::ipc::Args::INFO,
            &tango_core::ipc::Notification::INFO,
        ],
    )?;
    Ok(())
}
