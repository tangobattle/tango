//! Content identities shared by hosts, lobby codecs and replay containers.
//! These are claims until a host resolves and compares its own installed data.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub name: String,
    /// Bump when the same inputs can produce a different simulation.
    pub revision: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    /// Canonical package version; the package resolver validates its syntax.
    pub version: String,
    pub digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub engine: Runtime,
    pub script: Runtime,
    pub package: Package,
    pub export: String,
    /// Complete transitive dependencies, sorted by (name, version).
    pub dependencies: Vec<Package>,
    /// SHA3-256 of the effective ROM, after package transformations.
    pub rom: [u8; 32],
    /// Domain-separated digest of the executable environment and options.
    /// The negotiated seed and player position belong to the later invocation.
    pub environment: [u8; 32],
}

fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

impl Runtime {
    pub fn validate(&self) -> Result<(), super::Error> {
        if !name(&self.name) {
            return Err("invalid gamemode runtime name".into());
        }
        Ok(())
    }
}

impl Identity {
    /// Structural limits only. A claim never authorizes downloading or running
    /// code; compare it with a locally prepared identity before starting.
    pub fn validate(&self) -> Result<(), super::Error> {
        self.engine.validate()?;
        self.script.validate()?;
        if !name(&self.export) || self.dependencies.len() > 127 {
            return Err("invalid gamemode export or dependency count".into());
        }
        for package in std::iter::once(&self.package).chain(&self.dependencies) {
            if !name(&package.name) || package.version.is_empty() || package.version.len() > 128 {
                return Err("invalid gamemode package reference".into());
            }
        }
        fn key(package: &Package) -> (&str, &str) {
            (&package.name, &package.version)
        }
        if self.dependencies.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1]))
            || self
                .dependencies
                .iter()
                .any(|package| key(package) == key(&self.package))
        {
            return Err("gamemode dependencies must be unique and sorted".into());
        }
        Ok(())
    }
}
