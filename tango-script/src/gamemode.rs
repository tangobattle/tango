//! A selected gamemode's callbacks, rebuilt from immutable setup inputs.
mod prepared;
mod replay;
pub use prepared::PreparedGameMode;
pub use replay::{LegacyReplay, ReplayImport};

use std::collections::BTreeMap;
use std::sync::Arc;

use sha3::{Digest, Sha3_256};
use tango_match::gamemode::{Context, Hook, Program, Reader, Source, Update, Value};

// Includes host semantics beyond the Luau/compiler version already hashed in
// Profile. Bump when the same valid callback can produce a different result.
const SCRIPT_REVISION: u32 = 3;

fn hash_options(hash: &mut Sha3_256, options: &BTreeMap<String, Value>) {
    fn string(hash: &mut Sha3_256, value: &str) {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    }
    for (key, value) in options {
        string(hash, key);
        match value {
            Value::Boolean(value) => hash.update([0, u8::from(*value)]),
            Value::Number(value) => {
                hash.update([1]);
                hash.update(value.to_le_bytes());
            }
            Value::String(value) => {
                hash.update([2]);
                string(hash, value);
            }
        }
    }
}

#[derive(Clone)]
pub struct GameModeProgram {
    profile: Arc<crate::Profile>,
    context: Arc<Context>,
    source: Arc<Source>,
    hooks: Arc<[Hook]>,
}

impl GameModeProgram {
    /// Patch-only exports return None. Setup runs after the host has bound the
    /// effective ROM and other immutable inputs to this profile.
    pub fn new(profile: crate::Profile, context: Context) -> crate::Result<Option<Self>> {
        context.validate().map_err(|e| crate::invalid(e.to_string()))?;
        let profile = profile.for_gamemode()?;
        let hooks =
            crate::runtime::Runtime::for_export(&profile, crate::ExportKind::GameMode)?.setup_gamemode(&context)?;
        let Some(hooks) = hooks else { return Ok(None) };
        // A checkpoint from another seed/player/options must not restore this
        // program, even when its package graph and ROM are identical.
        let mut hash = Sha3_256::new();
        hash.update(b"tango-gamemode-context-v2\0");
        hash.update(SCRIPT_REVISION.to_le_bytes());
        hash.update(profile.digest());
        hash.update(context.player.to_le_bytes());
        hash.update(context.seed);
        hash.update([u8::from(context.disable_bgm)]);
        hash_options(&mut hash, &context.options);
        let source = Arc::new(Source {
            digest: hash.finalize().into(),
            name: profile.display_name().to_owned(),
        });
        Ok(Some(Self {
            profile: Arc::new(profile),
            context: Arc::new(context),
            source,
            hooks: hooks.into(),
        }))
    }
}

impl Program for GameModeProgram {
    fn source(&self) -> &Source {
        &self.source
    }
    fn hooks(&self) -> &[Hook] {
        &self.hooks
    }
    fn on_hook(&self, name: &str, reader: &mut dyn Reader) -> Result<Update, tango_match::gamemode::Error> {
        if !self.hooks.iter().any(|hook| hook.name == name) {
            return Err("unknown gamemode hook".into());
        }
        crate::runtime::Runtime::for_export(&self.profile, crate::ExportKind::GameMode)?
            .gamemode_hook(name, &self.hooks, &self.context, reader)
            .map_err(Into::into)
    }
}
