//! The single `Game` abstraction every game-specific crate plugs into.
//!
//! A [`Game`] is one ROM revision Tango supports, bundling *all* of its
//! per-game information in one place:
//!
//! - ROM identity (`family`/`variant`, `rom_code`/`revision`, `crc32`,
//!   `region`) — formerly the `tango-gamedb` crate.
//! - The save/ROM parsers (`parse_save_fn` / `load_rom_assets_fn`).
//! - The PvP engine support ([`tango_backend_mgba::GameSupport`]).
//! - The app-facing presentation bits (`match_types`, `save_templates`,
//!   `logo_image`, `background`).
//!
//! Each `tango-gamesupport-<game>` crate builds the `&'static Game`
//! registrations for its ROM revisions out of its own `dataview` and
//! `pvp` submodules. The application collects those statics into a
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

/// Region a ROM revision targets.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Region {
    US,
    JP,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The dump failed to parse as this game's save format. The concrete
    /// error type is the private gamesupport layer's; boxed here because
    /// the app only displays it.
    #[error("{0}")]
    Save(Box<dyn std::error::Error + Send + Sync>),

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

/// A parsed save, as [`Game::parse_save`] hands it out. Implemented
/// only by the private gamesupport layer — the full view surface behind
/// it is private knowledge; the app clones it, serializes it, and hands
/// it back to [`SaveEditor::load`].
pub trait SaveData: std::any::Any + Send + Sync {
    /// Serialize back to a cartridge SRAM dump.
    fn to_sram_dump(&self) -> Vec<u8>;
    /// Recompute the save's checksum. Needed after cloning a bundled
    /// template into a fresh save file — the template's checksum
    /// predates the clone.
    fn rebuild_checksum(&mut self);
    fn clone_box(&self) -> BoxedSave;
}

/// Parsed ROM assets, as [`Game::load_rom_assets`] hands them out.
/// Purely opaque: only the private gamesupport layer reads them.
pub trait AssetsData: std::any::Any + Send + Sync {}

/// Bundled save templates for a game. Each entry is a
/// `(template_name, save)` pair; the empty-string name is the default
/// template. Lazily parsed from `include_bytes!` blobs on first access.
pub type SaveTemplates = LazyLock<Vec<(&'static str, BoxedSave)>>;

/// Lazily-decoded bundled image (logo). The `include_bytes!` blob is held
/// in `.rodata`; the decode runs on first access.
pub type LazyImage = LazyLock<image::DynamicImage>;

/// Boxed opaque save the parsers hand back.
pub type BoxedSave = Box<dyn SaveData>;
/// Boxed opaque ROM assets the parsers hand back.
pub type BoxedAssets = Box<dyn AssetsData>;

// The save-editor embedding API, feature-gated so the base crate stays
// a pure detection/registry surface. Deliberately shape-oblivious: the
// trait speaks opaque envelopes; the private gamesupport UI layer
// implements it.
#[cfg(feature = "ui")]
pub mod save_editor;
#[cfg(feature = "ui")]
pub use save_editor::{
    AppliedPatch, ChipDisplay, LoadedSave, LoadedSavePayload, SaveEditor, SaveEditorEvent, SaveEditorMessage,
    SaveEditorState,
};

/// One ROM revision Tango supports, with all of its per-game info.
///
/// Built as a `&'static` in the owning `tango-gamesupport-<game>` crate.
/// See the module docs for the identity contract.
pub struct Game {
    /// The [`Family`] this game is a variant of — the back half of the
    /// families-own-their-games link, so anything holding a [`GameRef`]
    /// can reach its siblings (the other color version) without a
    /// registry to search. Its id is region-specific (`exe3` JP vs
    /// `bn3` US).
    pub family: &'static Family,
    /// Which variant of [`family`](Self::family) this is, e.g. 0 for
    /// Gregar and 1 for Falzar.
    pub variant: u8,
    /// 4-byte ROM code (e.g. `b"BR5E"`) and mask-ROM revision.
    pub rom_code: &'static [u8; 4],
    pub revision: u8,
    /// CRC32 of the full clean ROM, used to validate a detected dump.
    pub crc32: u32,
    pub region: Region,

    /// Parse a cartridge SRAM dump into a save, validating that the dump
    /// matches this game (region/variant). Errors on a mismatch.
    ///
    /// Required even for a netplay-only game: this is how a dump is
    /// recognized as belonging to this game, which is what makes it
    /// selectable in the save picker before a match. A game with no
    /// save *model* still validates the dump and hands back an opaque
    /// save that only round-trips its bytes.
    pub parse_save_fn: fn(&[u8]) -> Result<BoxedSave, Error>,
    /// Build the ROM Assets for this game. `charset` overrides the
    /// per-game default character set; pass `None` for the default.
    /// `None` when this game has no save/ROM model.
    pub load_rom_assets_fn: Option<fn(rom: &[u8], wram: &[u8], charset: Option<&[&str]>) -> BoxedAssets>,

    /// How this ROM plays netplay, and on which engine.
    /// How this game starts a match, plays on its own, and replays a
    /// recording — all on whatever emulator it runs, which nothing
    /// outside the game's own crate ever learns.
    pub pvp: &'static (dyn tango_match::Backend + Send + Sync),

    /// Length-per-mode list. Entry `i` is how many subtypes mode `i` has —
    /// e.g. BN6 is `[1, 1]`. Drives the match-type pick_list in the lobby.
    pub match_types: &'static [usize],
    /// Whether this game colors its players by *seat* rather than by
    /// field half. The BN games put your own navi on the red half
    /// whichever seat you take, so their panels lead with your side;
    /// Battle Chip Challenge instead paints P1 red and P2 blue for both
    /// players, and its panels follow that fixed order.
    pub players_colored_by_seat: bool,
    /// Bundled save templates, lazily parsed on first access. `None`
    /// when this game has no save model to template.
    pub save_templates: Option<&'static SaveTemplates>,
    /// Logo for the game, decoded on first access.
    pub logo_image: Option<&'static LazyImage>,
    /// Pointer to the BNLC-hosted background TGA — `None` for a game
    /// with no BNLC release to borrow art from.
    pub background: Option<BackgroundRef>,

    /// The game's save editor — a real, renderable [`save_editor::SaveEditor`]
    /// (load / render / update; every shape behind it is opaque). Exists
    /// only when this crate's `ui` feature is on — headless builds (the
    /// pvp probes, engine hosts) have no field. A game crate built
    /// alongside a `ui` consumer must have its own `ui` feature on to
    /// initialize it (tango's `gamesupport-*` features pair the two);
    /// mixing them is a missing-field error here, on purpose.
    /// Every game has one — a netplay-only game points at the shared
    /// empty editor, which renders the shell with no section tabs — so
    /// embedders never need an editor-less path.
    #[cfg(feature = "ui")]
    pub save_editor: &'static dyn save_editor::SaveEditor,
}

impl Game {
    pub fn family_and_variant(&self) -> (&'static str, u8) {
        (self.family.id, self.variant)
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

    pub fn load_rom_assets(&self, rom: &[u8], wram: &[u8], charset: Option<&[&str]>) -> Option<BoxedAssets> {
        Some((self.load_rom_assets_fn?)(rom, wram, charset))
    }

    /// Whether this game models its save beyond identifying it — false
    /// for netplay-only games, which have no templates or ROM assets
    /// behind the dump they accept (their save editor is the shared
    /// empty one).
    pub fn has_save_model(&self) -> bool {
        self.load_rom_assets_fn.is_some()
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
    games
        .iter()
        .copied()
        .find(|g| g.family_and_variant() == (family, variant))
}

pub fn find_by_rom_info(games: &[GameRef], code: &[u8; 4], revision: u8) -> Option<GameRef> {
    games
        .iter()
        .copied()
        .find(|g| g.rom_code_and_revision() == (code, revision))
}

/// Where each console keeps the game code and mask-ROM revision in its
/// cartridge header. A dump is identified by trying every layout: a
/// wrong guess reads bytes that match no registration, and the CRC32
/// below is the real gate either way.
const HEADER_LAYOUTS: &[(usize, usize)] = &[
    // Game Boy Advance.
    (0xac, 0xbc),
    // Nintendo DS.
    (0x0c, 0x1e),
];

/// Identify a clean ROM dump: match the `code`/`revision` header bytes,
/// then confirm the CRC32. Returns `None` if unrecognized or corrupted.
pub fn detect(games: &[GameRef], rom: &[u8]) -> Option<GameRef> {
    HEADER_LAYOUTS
        .iter()
        .filter_map(|&(code_at, revision_at)| {
            let code: &[u8; 4] = rom.get(code_at..code_at + 4)?.try_into().ok()?;
            find_by_rom_info(games, code, *rom.get(revision_at)?)
        })
        .find(|entry| crc32fast::hash(rom) == entry.crc32())
}
