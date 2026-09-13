//! Compatibility with replay headers written before package configurations.
use std::collections::BTreeMap;
use tango_match::gamemode::{identity::Runtime, Context, Value};

#[derive(serde::Serialize)]
pub struct LegacyReplay<'a> {
    pub game: &'a tango_replay::metadata::GameInfo,
    pub match_type: u32,
    pub match_subtype: u32,
    pub engine: &'a Runtime,
}

/// An explicit import supplies invocation options, never a claimed identity.
/// The host prepares and pins the actual installed code and transformed ROM.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayImport {
    #[serde(default)]
    pub disable_bgm: bool,
    #[serde(default)]
    pub options: BTreeMap<String, Value>,
}

impl ReplayImport {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        Context {
            player: 1,
            seed: [0; 16],
            disable_bgm: self.disable_bgm,
            options: self.options.clone(),
        }
        .validate()
        .map_err(|error| crate::invalid(error.to_string()))
    }
}

impl crate::Profile {
    pub fn import_replay(&self, legacy: &LegacyReplay<'_>, rom: &[u8]) -> crate::Result<Option<ReplayImport>> {
        if rom.len() > crate::MAX_BUFFER {
            return Err(crate::invalid("ROM exceeds 32 MiB"));
        }
        let profile = self
            .clone()
            .for_gamemode()?
            .with_locale("en-US")?
            .with_inputs(crate::Inputs::new([("rom".into(), rom.to_vec())].into())?);
        let runtime = crate::runtime::Runtime::for_export(&profile, crate::ExportKind::GameMode)?;
        runtime.import_replay(legacy, rom)
    }
}
