//! Headless save preparation and validation contracts.

/// A patch applied on top of a ROM, as the save UI needs to know it:
/// its identity plus the exact ROM's object from `[rom_overrides]`
/// (charset, display-name, and chip-legality overrides).
#[derive(Clone)]
pub struct AppliedPatch {
    pub name: String,
    pub version: semver::Version,
    pub rom_overrides: tango_patch::Overrides,
}

/// A parsed save plus the effective assets derived from its ROM and patch,
/// prepared before either validation or presentation is involved.
pub struct PreparedSave {
    pub game: crate::GameRef,
    pub save_path: std::path::PathBuf,
    pub patch: Option<AppliedPatch>,
    pub save: crate::BoxedSave,
    pub assets: crate::BoxedAssets,
}

/// Structured game-owned findings. Presentation adapters may inspect their own type.
pub trait Validation: std::any::Any + Send + Sync {
    fn is_empty(&self) -> bool;
}
impl PreparedSave {
    pub fn validate(&self) -> Box<dyn Validation> {
        self.save.validate(self.assets.as_ref())
    }
    pub fn snapshot_sram(&self) -> Vec<u8> {
        self.save.snapshot_sram()
    }
}
