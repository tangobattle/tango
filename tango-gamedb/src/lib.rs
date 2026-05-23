use std::any::Any;

mod games;
pub use games::*;

#[derive(Clone, Copy, PartialEq)]
pub enum Region {
    US,
    JP,
}

/// One ROM revision known to Trill. Each variant is a zero-sized type
/// (see [`games`]) with a `pub static` instance — callers refer to games
/// by the `&AREJ_00` style. Compare via the `&dyn Game` PartialEq impl,
/// which uses TypeId, so each per-game type is its own identity.
pub trait Game: Any + Send + Sync {
    fn family_and_variant(&self) -> (&'static str, u8);
    fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8);
    fn crc32(&self) -> u32;
    fn region(&self) -> Region;

    /// Parse a cartridge SRAM dump into a [`Save`](tango_dataview::save::Save),
    /// validating that the dump matches this game (region/variant). Errors
    /// when the dump is for a different game.
    fn parse_save(&self, sram: &[u8]) -> Result<Box<dyn tango_dataview::save::Save + Send + Sync>, Error>;

    /// Build the rom Assets for this game. `charset` overrides the
    /// per-game default character set (passed as `Option<&[&str]>`); pass
    /// `None` to use the per-game default. The application wraps the
    /// returned Assets with patch-overrides (chip names, navicust parts,
    /// etc.) outside this crate.
    fn load_rom_assets(
        &self,
        rom: &[u8],
        wram: &[u8],
        charset: Option<&[&str]>,
    ) -> Box<dyn tango_dataview::rom::Assets + Send + Sync>;
}

// `&'static dyn Game` identity is by TypeId — each per-game unit struct in
// `games.rs` is its own type, so two references to different game statics
// hash and compare unequal even though both are zero-sized.
impl PartialEq for &'static (dyn Game + Send + Sync) {
    fn eq(&self, other: &Self) -> bool {
        Any::type_id(*self) == Any::type_id(*other)
    }
}
impl Eq for &'static (dyn Game + Send + Sync) {}
impl std::hash::Hash for &'static (dyn Game + Send + Sync) {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Any::type_id(*self).hash(state)
    }
}
impl std::fmt::Debug for &'static (dyn Game + Send + Sync) {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("family_and_variant", &self.family_and_variant())
            .finish()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    DataView(#[from] tango_dataview::save::Error),

    /// `parse_save` was given a save for a different region/variant
    /// than this gamedb entry.
    #[error("save is not compatible with this game")]
    IncompatibleSave,
}

pub static GAMES: &[&'static (dyn Game + Send + Sync)] = &[
    &AREJ_00,
    &AREE_00,
    &AE2J_00_AC,
    &AE2E_00,
    &A6BJ_01,
    &A3XJ_01,
    &A6BE_00,
    &A3XE_00,
    &B4WJ_01,
    &B4BJ_01,
    &B4WE_00,
    &B4BE_00,
    &BRBJ_00,
    &BRKJ_00,
    &BRBE_00,
    &BRKE_00,
    &BR5J_00,
    &BR6J_00,
    &BR5E_00,
    &BR6E_00,
    &BR4J_00,
];

pub fn find_by_family_and_variant(family: &str, variant: u8) -> Option<&'static (dyn Game + Send + Sync)> {
    GAMES
        .iter()
        .copied()
        .find(|g| g.family_and_variant() == (family, variant))
}

pub fn find_by_rom_info(code: &[u8; 4], revision: u8) -> Option<&'static (dyn Game + Send + Sync)> {
    GAMES
        .iter()
        .copied()
        .find(|g| g.rom_code_and_revision() == (code, revision))
}

pub fn detect(rom: &[u8]) -> Option<&'static (dyn Game + Send + Sync)> {
    let code: &[u8; 4] = rom.get(0xac..0xac + 4)?.try_into().ok()?;
    let revision = *rom.get(0xbc)?;
    let entry = GAMES
        .iter()
        .copied()
        .find(|g| g.rom_code_and_revision() == (code, revision))?;
    let crc32 = crc32fast::hash(rom);
    if crc32 != entry.crc32() {
        return None;
    }
    Some(entry)
}
