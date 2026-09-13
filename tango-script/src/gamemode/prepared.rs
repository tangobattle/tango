use std::collections::BTreeMap;

use sha3::{Digest, Sha3_256};
use tango_match::gamemode::{
    identity::{Identity, Package, Runtime},
    Context, Value,
};

use crate::{invalid, ExportKind, GameModeProgram, Inputs, Profile, Result};

/// One seat's immutable simulation inputs. Preparing applies package patches
/// once, then freezes the resulting ROM and options for every live/replay boot.
/// No filesystem or catalog lookup occurs when a program is opened.
#[derive(Clone)]
pub struct PreparedGameMode {
    profile: Profile,
    options: BTreeMap<String, Value>,
    disable_bgm: bool,
    identity: Identity,
}

impl PreparedGameMode {
    /// `engine` is the host backend's simulation ABI, never a package claim.
    /// Only the ROM is exposed as a host input. Inputs from editor sessions are
    /// discarded, and all simulation callbacks use the fixed en-US locale.
    pub fn prepare(
        profile: Profile,
        engine: Runtime,
        rom: Vec<u8>,
        disable_bgm: bool,
        options: BTreeMap<String, Value>,
    ) -> Result<Self> {
        engine.validate().map_err(|e| invalid(e.to_string()))?;
        Context {
            player: 1,
            seed: [0; 16],
            disable_bgm,
            options: options.clone(),
        }
        .validate()
        .map_err(|e| invalid(e.to_string()))?;
        if rom.len() > crate::MAX_BUFFER {
            return Err(invalid("ROM exceeds 32 MiB"));
        }
        let profile = profile
            .for_gamemode()?
            .with_inputs(Inputs::new(BTreeMap::from([("rom".into(), rom)]))?);
        let rom = profile.patch_rom(profile.inputs.get("rom").expect("bound ROM"))?;
        let rom_digest = Sha3_256::digest(&rom).into();
        let profile = profile.with_inputs(Inputs::new(BTreeMap::from([("rom".into(), rom)]))?);
        let mut packages: Vec<_> = profile
            .packages
            .iter()
            .map(|package| Package {
                name: package.manifest().name.clone(),
                version: package.manifest().version.to_string(),
                digest: package.digest(),
            })
            .collect();
        let package = packages.pop().expect("resolved profile");
        packages.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        let mut hash = Sha3_256::new();
        hash.update(b"tango-gamemode-environment-v1\0");
        hash.update(super::SCRIPT_REVISION.to_le_bytes());
        hash.update(profile.digest());
        hash.update([u8::from(disable_bgm)]);
        super::hash_options(&mut hash, &options);
        let identity = Identity {
            engine,
            script: Runtime {
                name: "luau".into(),
                revision: super::SCRIPT_REVISION,
            },
            package,
            export: profile
                .selected_export(ExportKind::GameMode)
                .expect("selected export")
                .into(),
            dependencies: packages,
            rom: rom_digest,
            environment: hash.finalize().into(),
        };
        identity.validate().map_err(|e| invalid(e.to_string()))?;
        Ok(Self {
            profile,
            options,
            disable_bgm,
            identity,
        })
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// The exact descriptor to exchange or record, including option values.
    pub fn configuration(&self) -> Result<tango_match::gamemode::Configuration> {
        tango_match::gamemode::Configuration::new(self.identity.clone(), self.disable_bgm, self.options.clone())
            .map_err(|error| invalid(error.to_string()))
    }

    pub fn rom(&self) -> &[u8] {
        self.profile.inputs.get("rom").expect("bound ROM")
    }

    pub fn options(&self) -> &BTreeMap<String, Value> {
        &self.options
    }

    /// Verify this seat against the same seat in a lobby offer or replay.
    /// Cross-variant players can have different ROMs: comparing player one's
    /// identity to player two's is not a compatibility check.
    pub fn verify(&self, expected: &Identity) -> Result<()> {
        expected.validate().map_err(|e| invalid(e.to_string()))?;
        if &self.identity != expected {
            return Err(invalid(
                "gamemode contents, runtime, ROM or options do not match the pinned identity",
            ));
        }
        Ok(())
    }

    /// The only values supplied after preparation are the negotiated seed and
    /// 1-based player position. Every opening recreates callbacks from setup.
    pub fn program(&self, player: u16, seed: [u8; 16]) -> Result<GameModeProgram> {
        let mut program = GameModeProgram::new(
            self.profile.clone(),
            Context {
                player,
                seed,
                disable_bgm: self.disable_bgm,
                options: self.options.clone(),
            },
        )?
        .ok_or_else(|| invalid("selected gamemode only patches ROMs; it does not provide simulation hooks"))?;
        let mut hash = Sha3_256::new();
        hash.update(b"tango-gamemode-engine-context-v1\0");
        hash.update((self.identity.engine.name.len() as u64).to_le_bytes());
        hash.update(self.identity.engine.name.as_bytes());
        hash.update(self.identity.engine.revision.to_le_bytes());
        hash.update(program.source.digest);
        program.source = std::sync::Arc::new(tango_match::gamemode::Source {
            digest: hash.finalize().into(),
            name: program.source.name.clone(),
        });
        Ok(program)
    }
}

impl tango_match::gamemode::Factory for PreparedGameMode {
    fn configuration(&self) -> std::result::Result<tango_match::gamemode::Configuration, tango_match::gamemode::Error> {
        Ok(self.configuration()?)
    }

    fn rom(&self) -> &[u8] {
        self.rom()
    }

    fn open(
        &self,
        player: u16,
        seed: [u8; 16],
    ) -> std::result::Result<Box<dyn tango_match::gamemode::Program>, tango_match::gamemode::Error> {
        Ok(Box::new(self.program(player, seed)?))
    }
}
