//! The save-editor embedding API — the one thing this crate knows about
//! the save UI: it can be loaded, held, rendered and updated. Every
//! actual shape (the view state's internals, the message vocabulary,
//! the loaded bundle) is private gamesupport knowledge; here they are
//! opaque marker traits, so the app embeds a save editor while staying
//! completely game-agnostic, and this crate stays oblivious to how any
//! of it works.
//!
//! The private UI layer implements [`SaveEditor`] once (a generic shell
//! over its per-game interface), so `Game::save_editor` is a real,
//! renderable trait object — no downcasting anywhere in the dispatch.

use unic_langid::LanguageIdentifier;

/// A patch applied on top of a ROM, as the save UI needs to know it:
/// its identity plus the exact ROM's object from `[rom_overrides]`
/// (charset, display-name, and chip-legality overrides).
#[derive(Clone)]
pub struct AppliedPatch {
    pub name: String,
    pub version: semver::Version,
    pub rom_overrides: tango_patch::Overrides,
}

/// One chip as [`LoadedSave::chips`] carries it: display name and
/// pre-baked icon, each `None` when the game has none for that id.
#[derive(Default, Clone)]
pub struct ChipDisplay {
    pub name: Option<String>,
    pub icon: Option<iced::widget::image::Handle>,
}

/// One chip implicated in a PvP build violation. Names are resolved while the
/// save's patched ROM assets are available; `id` remains as a stable fallback
/// for games or patches that do not publish one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildChip {
    pub id: usize,
    pub code: String,
    pub name: Option<String>,
}

/// One enabled BN5/BN6 Patch Card implicated in a PvP build violation. The
/// display name is resolved while patched ROM assets are available; `id` is a
/// stable fallback and `mb` preserves the exact cost shown by the save editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPatchCard {
    pub id: usize,
    pub name: Option<String>,
    pub mb: u8,
}

/// The legality rule violated by one chip slot. This enum is intentionally
/// chip-specific: NaviCust and other subject types get their own kind enums
/// rather than being attached to [`BuildViolation::Chip`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildViolationKind {
    ChipIllegalForGame,
    TooManyCopiesOfChip { used: usize, limit: usize },
    TooManyNaviChips { used: usize, limit: usize },
    TooManyMegaChips { used: usize, limit: usize },
    TooManyGigaChips { used: usize, limit: usize },
    TooManyDarkChips { used: usize, limit: usize },
    RegularChipExceedsMemory { used: u32, limit: u32 },
    TagChipsExceedMemory { used: u32, limit: u32 },
}

/// The legality rule violated by an enabled BN5/BN6 Patch Card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchCardViolationKind {
    TotalMbExceeded { used: u32, limit: u32 },
}

/// One raw legality problem in a PvP build. Subject-specific violations are
/// returned at their source granularity; presentation layers may group them.
/// Top-level rules remain separate variants. Future non-chip subjects add a
/// new variant with their own typed subject and kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildViolation {
    FolderNotFull {
        used: usize,
        required: usize,
    },
    Chip {
        slot: usize,
        chip: BuildChip,
        kind: BuildViolationKind,
    },
    PatchCard {
        slot: usize,
        patch_card: BuildPatchCard,
        kind: PatchCardViolationKind,
    },
}

/// Structured, still-unlocalized input for a view-time violation formatter.
/// The PvP view may group raw slot violations before submitting them here.
pub enum BuildViolationFormat<'a> {
    FolderNotFull {
        used: usize,
        required: usize,
    },
    Chips {
        chips: &'a [&'a BuildChip],
        kind: BuildViolationKind,
    },
    PatchCards {
        patch_cards: &'a [&'a BuildPatchCard],
        kind: PatchCardViolationKind,
    },
}

/// A formatter supplied by the opaque save-editor implementation. Keeping a
/// function pointer lets a blinded opponent's loaded save be discarded while
/// the session view still formats its structured violations in the current UI
/// language on every frame.
pub type BuildViolationFormatter = for<'a> fn(&LanguageIdentifier, BuildViolationFormat<'a>) -> String;

/// Legality of the exact save committed to a PvP match. There is deliberately
/// no "unknown" state: failure to parse or load a committed save is a session
/// construction error, while every successfully loaded save is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildValidity {
    Valid,
    Invalid(Vec<BuildViolation>),
}

impl BuildValidity {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    pub fn violations(&self) -> &[BuildViolation] {
        match self {
            Self::Valid => &[],
            Self::Invalid(violations) => violations,
        }
    }
}

/// A message minted inside the save-editor view. Implemented (and
/// consumed) only by the private UI layer; the app routes
/// `Arc<dyn SaveEditorMessage>` through its message enums without looking
/// inside (the `Arc` keeps it cheaply `Clone`, as iced messages must
/// be).
pub trait SaveEditorMessage: std::any::Any + std::fmt::Debug + Send + Sync {}

/// Opaque per-save view state (active tab, edit session, scroll and
/// animation bookkeeping), held as [`LoadedSave::state`]. Minted by
/// [`SaveEditor::load`] and dropped with the save it belongs to, so a
/// save switch or a closed view takes its view state with it and a
/// rebuilt save can never inherit a stale one — except for where the
/// reader was looking, which [`SaveEditor::carry_view_position`] hands
/// across a rebuild of the *same* save on purpose.
pub trait SaveEditorState: std::any::Any + Send + Sync {}

/// The private UI layer's loaded bundle (model + baked art), as held
/// behind [`LoadedSave::payload`]. Opaque by design; only that layer
/// implements and reads it.
pub trait LoadedSavePayload: std::any::Any + Send + Sync {}

/// A loaded save, ready to render: the game-agnostic facts the app
/// needs for launching and committing (game, path, patch), the UI to
/// drive it with, the view state that UI is currently in, and the
/// private layer's loaded bundle behind an opaque payload.
pub struct LoadedSave {
    /// The save editor driving this data — render with
    /// [`SaveEditor::view`], mutate with [`SaveEditor::update`].
    pub editor: &'static dyn SaveEditor,
    pub game: crate::GameRef,
    /// The ROM's chip table as anything outside the save view draws it
    /// (the match-analysis chart's chip lanes), indexed by chip id:
    /// name and pre-baked icon, both `None` where the game has neither.
    /// Baked with the rest of the art at load, since only the private
    /// layer can read the assets behind [`payload`](Self::payload). The
    /// icon handles are Arc-backed — cloning one per use is a refcount
    /// bump.
    pub chips: Vec<ChipDisplay>,
    /// Where a commit writes back to. Empty for saves without a backing
    /// file (a replay's embedded SRAM).
    pub save_path: std::path::PathBuf,
    pub patch: Option<AppliedPatch>,
    /// Where the view for this save currently is (open tab, edit
    /// session, scroll). Lives here so it is born and dropped with the
    /// save it describes — there is nothing for the embedder to
    /// reset or tear down.
    pub state: Box<dyn SaveEditorState>,
    /// The private UI layer's loaded bundle (model + baked art).
    pub payload: Box<dyn LoadedSavePayload>,
}

/// What the app must act on after an [`SaveEditor::update`] — deliberately
/// app-semantic only (clipboard, launches, disk writes); staged edits
/// are applied to the data internally and never surface.
pub enum SaveEditorEvent {
    /// Copy plain text to the clipboard.
    CopyText(String),
    /// Copy a raster image to the clipboard.
    CopyImage(image::RgbaImage),
    /// The embedder-defined Play button was pressed.
    Play,
    /// The embedder-defined Training button was pressed.
    Training,
    /// The edit session committed: write `sram` to the data's
    /// [`save_path`](LoadedSave::save_path) (the in-memory save is
    /// already the committed state).
    Commit { sram: Vec<u8> },
    /// The edit session was discarded — reload the on-disk original.
    Cancel,
}

/// A game's save editor, as [`crate::Game::save_editor`] carries it. The
/// contract is exactly "load, render, update": everything else about
/// the editor is the private layer's business.
pub trait SaveEditor: Send + Sync {
    /// Bundle a parsed save (+ its already-patched ROM) into renderable
    /// data.
    fn load(
        &'static self,
        game: crate::GameRef,
        patched_rom: Vec<u8>,
        save_path: std::path::PathBuf,
        save: crate::BoxedSave,
        patch: Option<AppliedPatch>,
    ) -> LoadedSave;

    /// Render the save view. `play_button`: `None` hides the Play
    /// button, `Some(enabled)` renders it. `editable` gates the whole
    /// edit affordance (only the play tab passes true).
    fn view<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        data: &'a LoadedSave,
        streamer_mode: bool,
        play_button: Option<bool>,
        inline_actions: bool,
        editable: bool,
    ) -> iced::Element<'a, std::sync::Arc<dyn SaveEditorMessage>>;

    /// Fold a message into the data: its view state always, and the
    /// save itself when the message is a staged edit (applied in place,
    /// including derived art — an `editable: false` embed can't mint
    /// one). Returns a follow-up task plus whatever the app must act
    /// on. `lang` feeds the arms that render text (clipboard copies
    /// localize their section headers the same way the view does).
    fn update(
        &self,
        lang: &LanguageIdentifier,
        data: &mut LoadedSave,
        msg: &dyn SaveEditorMessage,
    ) -> (
        iced::Task<std::sync::Arc<dyn SaveEditorMessage>>,
        Option<SaveEditorEvent>,
    );

    /// Serialize the current in-memory save (staged edits included) —
    /// what a netplay commitment or session launch runs on.
    fn sram(&self, data: &LoadedSave) -> Vec<u8>;

    /// Validate the exact in-memory build represented by `data`, returning
    /// structured violations suitable for an in-match advisory warning.
    fn build_validity(&self, data: &LoadedSave) -> BuildValidity;

    /// The save UI's canonical, localized presentation for build violations.
    /// Callers pass the current language at render time; formatted strings must
    /// never be cached in [`BuildValidity`].
    fn build_violation_formatter(&self) -> BuildViolationFormatter;

    /// Carry where the view was looking — the open tab, the sort
    /// preferences — from a state built for this same save onto a
    /// freshly built one.
    ///
    /// Cancelling an edit session reverts by rebuilding the whole
    /// loaded save from disk, which mints a new state with it; without
    /// this, cancelling would also throw the reader back to the first
    /// tab, which committing does not. Editors with no view position to
    /// speak of need not implement it.
    fn carry_view_position(&self, _from: &dyn SaveEditorState, _into: &mut dyn SaveEditorState) {}
}
