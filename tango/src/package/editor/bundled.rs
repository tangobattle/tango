use std::sync::OnceLock;

use tango_script::Package;

type Files = &'static [(&'static str, &'static [u8])];
const BUNDLED: &[Files] = include!(concat!(env!("OUT_DIR"), "/packages.rs"));

/// Decode bundled files once; the library combines them with installed versions
/// and resolves package graphs on its background scan.
pub(crate) fn packages() -> Result<&'static [Package], String> {
    static PACKAGES: OnceLock<Result<Vec<Package>, String>> = OnceLock::new();
    PACKAGES
        .get_or_init(|| {
            BUNDLED
                .iter()
                .map(|files| {
                    Package::load(
                        files
                            .iter()
                            .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
                            .collect(),
                    )
                    .map_err(|error| error.to_string())
                })
                .collect()
        })
        .as_deref()
        .map_err(Clone::clone)
}
