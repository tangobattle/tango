//! The single `Game` abstraction every game-specific crate plugs into.
//!
//! A [`Game`] is one ROM revision Tango supports, bundling *all* of its
//! per-game information in one place:
//!
//! - ROM identity (`family`/`variant`, `rom_code`/`revision`, `crc32`,
//!   `region`) — formerly the `tango-gamedb` crate.
//! - The save/ROM parsers (`parse_save_fn` / `load_rom_assets_fn`).
//! - The PvP rollback [`hooks`](tango_pvp::hooks::Hooks).
//! - The app-facing presentation bits (`match_types`, `save_templates`,
//!   `logo_image`, `background`).
//!
//! Each `tango-gamesupport-<game>` crate builds the `&'static Game`
//! registrations for its ROM revisions out of its own `dataview` and
//! `pvp_hooks` submodules. The application collects those statics into a
//! single registry slice and drives lookup through [`detect`],
//! [`find_by_family_and_variant`], and [`find_by_rom_info`]. That registry
//! is the only place that needs editing to enable a game.
//!
//! Game identity is by `&'static` pointer: each registration is a unique
//! static, so two `&'static Game` referring to the same registration hash
//! and compare equal and everything else compares distinct. (`Game` is
//! deliberately neither `Clone` nor `Copy` so a registration can't be
//! moved off its static and lose its identity.)

use std::sync::LazyLock;

#[cfg(feature = "testing")]
pub mod golden;

/// Region a ROM revision targets.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Region {
    US,
    JP,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    DataView(#[from] tango_dataview::save::Error),

    /// `parse_save` was given a save for a different region/variant than
    /// this game.
    #[error("save is not compatible with this game")]
    IncompatibleSave,
}

/// Which BNLC volume — Vol 1 (BN1-3) or Vol 2 (BN4-6). The enum also
/// carries the corresponding Steam app id. Lives here (rather than in the
/// app) so per-game [`BackgroundRef`]s can name their volume without a
/// dependency on the GUI crate.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Volume {
    Vol1,
    Vol2,
}

impl Volume {
    pub fn steam_app_id(self) -> u32 {
        match self {
            Volume::Vol1 => 1798010,
            Volume::Vol2 => 1798020,
        }
    }
}

/// Points at a background TGA inside a BNLC volume's shared `exe.dat`
/// asset archive. The full path in the zip is `exe/data/bg/<tga>`.
/// Resolved at runtime by the application; if BNLC isn't installed the
/// caller falls back to no background.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BackgroundRef {
    pub volume: Volume,
    pub tga: &'static str,
}

/// Bundled save templates for a game. Each entry is a
/// `(template_name, save)` pair; the empty-string name is the default
/// template. Lazily parsed from `include_bytes!` blobs on first access.
pub type SaveTemplates = LazyLock<Vec<(&'static str, &'static (dyn tango_dataview::save::Save + Send + Sync))>>;

/// Lazily-decoded bundled image (logo). The `include_bytes!` blob is held
/// in `.rodata`; the decode runs on first access.
pub type LazyImage = LazyLock<image::DynamicImage>;

/// Boxed save trait object the parsers hand back.
pub type BoxedSave = Box<dyn tango_dataview::save::Save + Send + Sync>;
/// Boxed ROM assets trait object the parsers hand back.
pub type BoxedAssets = Box<dyn tango_dataview::rom::Assets + Send + Sync>;

/// One ROM revision Tango supports, with all of its per-game info.
///
/// Built as a `&'static` in the owning `tango-gamesupport-<game>` crate.
/// See the module docs for the identity contract.
pub struct Game {
    /// Family + variant, e.g. `("bn6", 0)`. The family string is
    /// region-specific (`exe3` JP vs `bn3` US).
    pub family: &'static str,
    pub variant: u8,
    /// 4-byte ROM code (e.g. `b"BR5E"`) and mask-ROM revision.
    pub rom_code: &'static [u8; 4],
    pub revision: u8,
    /// CRC32 of the full clean ROM, used to validate a detected dump.
    pub crc32: u32,
    pub region: Region,

    /// Parse a cartridge SRAM dump into a save, validating that the dump
    /// matches this game (region/variant). Errors on a mismatch.
    pub parse_save_fn: fn(&[u8]) -> Result<BoxedSave, Error>,
    /// Build the ROM Assets for this game. `charset` overrides the
    /// per-game default character set; pass `None` for the default.
    pub load_rom_assets_fn: fn(rom: &[u8], wram: &[u8], charset: Option<&[&str]>) -> BoxedAssets,

    /// PvP / replay rollback hooks for this ROM.
    pub hooks: &'static (dyn tango_pvp::hooks::Hooks + Send + Sync),

    /// Length-per-mode list. Entry `i` is how many subtypes mode `i` has —
    /// e.g. BN6 is `[1, 1]`. Drives the match-type pick_list in the lobby.
    pub match_types: &'static [usize],
    /// Bundled save templates, lazily parsed on first access.
    pub save_templates: &'static SaveTemplates,
    /// Logo for the game, decoded on first access.
    pub logo_image: &'static LazyImage,
    /// Pointer to the BNLC-hosted background TGA.
    pub background: BackgroundRef,
}

impl Game {
    pub fn family_and_variant(&self) -> (&'static str, u8) {
        (self.family, self.variant)
    }

    pub fn rom_code_and_revision(&self) -> (&'static [u8; 4], u8) {
        (self.rom_code, self.revision)
    }

    pub fn crc32(&self) -> u32 {
        self.crc32
    }

    pub fn region(&self) -> Region {
        self.region
    }

    pub fn parse_save(&self, sram: &[u8]) -> Result<BoxedSave, Error> {
        (self.parse_save_fn)(sram)
    }

    pub fn load_rom_assets(&self, rom: &[u8], wram: &[u8], charset: Option<&[&str]>) -> BoxedAssets {
        (self.load_rom_assets_fn)(rom, wram, charset)
    }
}

// Identity by static address: each registration is a unique `&'static`,
// so the same registration hashes/compares equal and distinct ones don't.
impl PartialEq for Game {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for Game {}
impl std::hash::Hash for Game {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self as *const Game).hash(state);
    }
}
impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("family_and_variant", &self.family_and_variant())
            .finish()
    }
}

/// A reference to a registered game. Cheap to copy and used as a map key.
pub type GameRef = &'static Game;

/// A game family — a region/title grouping (e.g. `"bn6"` / `"exe6"`) that
/// owns its variant [`Game`]s and its localized strings. Each
/// `tango-gamesupport-<game>` crate exports its families as `FAMILIES`;
/// the app aggregates those into the single registry and the game-name
/// localizer, so games and their translations stay together and are
/// enabled by one feature.
pub struct Family {
    /// Family id, e.g. `"bn6"` / `"exe6"`. Equal to the `family` field of
    /// every game in [`games`](Self::games).
    pub id: &'static str,
    /// The variants in this family (its `Game` registrations).
    pub games: &'static [GameRef],
    /// Per-locale Fluent fragments for this family, one `(lang, source)`
    /// entry per locale. Keys are *bare* (`name`, `short`,
    /// `variant-<n>`, `variant-<n>-short`, `match-type-<m>-<s>`,
    /// `save-<template>`) — the family supplies the namespace, so there's
    /// no error-prone `game-<family>` prefix to keep in sync.
    pub translations: &'static [(&'static str, &'static str)],
}

/// Flatten a family slice into the game registry it represents.
pub fn games_of(families: &[&'static Family]) -> Vec<GameRef> {
    families.iter().flat_map(|f| f.games.iter().copied()).collect()
}

pub fn find_by_family_and_variant(games: &[GameRef], family: &str, variant: u8) -> Option<GameRef> {
    games.iter().copied().find(|g| g.family_and_variant() == (family, variant))
}

pub fn find_by_rom_info(games: &[GameRef], code: &[u8; 4], revision: u8) -> Option<GameRef> {
    games.iter().copied().find(|g| g.rom_code_and_revision() == (code, revision))
}

/// Identify a clean ROM dump: match the `code`/`revision` header bytes,
/// then confirm the CRC32. Returns `None` if unrecognized or corrupted.
pub fn detect(games: &[GameRef], rom: &[u8]) -> Option<GameRef> {
    let code: &[u8; 4] = rom.get(0xac..0xac + 4)?.try_into().ok()?;
    let revision = *rom.get(0xbc)?;
    let entry = find_by_rom_info(games, code, revision)?;
    if crc32fast::hash(rom) != entry.crc32() {
        return None;
    }
    Some(entry)
}
