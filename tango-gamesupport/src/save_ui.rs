//! The save-editor embedding API — the one thing this crate knows about
//! the save UI: it can be loaded, held, rendered and updated. Every
//! actual shape (the view state's internals, the message vocabulary,
//! the loaded bundle) is private gamesupport knowledge; here they are
//! opaque marker traits, so the app embeds a save editor while staying
//! completely game-agnostic, and this crate stays oblivious to how any
//! of it works.
//!
//! The private UI layer implements [`SaveUi`] once (a generic shell
//! over its per-game interface), so `Game::save_ui` is a real,
//! renderable trait object — no downcasting anywhere in the dispatch.

use unic_langid::LanguageIdentifier;

/// A patch applied on top of a ROM, as the save UI needs to know it:
/// its identity plus the `[rom_overrides]` scanned from its package
/// (charset + display-name overrides).
#[derive(Clone)]
pub struct AppliedPatch {
    pub name: String,
    pub version: semver::Version,
    pub rom_overrides: tango_patch::Overrides,
}

/// A message minted inside the save-editor view. Implemented (and
/// consumed) only by the private UI layer; the app routes
/// `Arc<dyn SaveUiMessage>` through its message enums without looking
/// inside (the `Arc` keeps it cheaply `Clone`, as iced messages must
/// be).
pub trait SaveUiMessage: std::any::Any + std::fmt::Debug + Send + Sync {}

/// Opaque per-embed view state (active tab, edit session, scroll and
/// animation bookkeeping). One `Box<dyn SaveViewState>` per on-screen
/// save view; outlives game and save switches. Created by
/// [`SaveUi::new_state`] — or, before any game is loaded, by the
/// private layer's game-independent constructor.
pub trait SaveViewState: std::any::Any + Send + Sync {}

/// The private UI layer's loaded bundle (model + baked art), as held
/// behind [`SaveViewData::payload`]. Opaque by design; only that layer
/// implements and reads it.
pub trait SaveViewPayload: std::any::Any + Send + Sync {}

/// A loaded save, ready to render: the game-agnostic facts the app
/// needs for launching and committing (game, path, patch), the UI to
/// drive it with, and the private layer's loaded bundle behind an
/// opaque payload.
pub struct SaveViewData {
    /// The save editor driving this data — render with
    /// [`SaveUi::view`], mutate with [`SaveUi::update`].
    pub ui: &'static dyn SaveUi,
    pub game: crate::GameRef,
    /// Where a commit writes back to. Empty for saves without a backing
    /// file (a replay's embedded SRAM).
    pub save_path: std::path::PathBuf,
    pub patch: Option<AppliedPatch>,
    /// The private UI layer's loaded bundle (model + baked art).
    pub payload: Box<dyn SaveViewPayload>,
}

/// What the app must act on after an [`SaveUi::update`] — deliberately
/// app-semantic only (clipboard, launches, disk writes); staged edits
/// are applied to the data internally and never surface.
pub enum SaveUiOutcome {
    /// Copy plain text to the clipboard.
    CopyText(String),
    /// Copy a raster image to the clipboard.
    CopyImage(image::RgbaImage),
    /// The embedder-defined Play button was pressed.
    Play,
    /// The embedder-defined Training button was pressed.
    Training,
    /// The edit session committed: write `sram` to the data's
    /// [`save_path`](SaveViewData::save_path) (the in-memory save is
    /// already the committed state).
    Commit { sram: Vec<u8> },
    /// The edit session was discarded — reload the on-disk original.
    Cancel,
}

/// A game's save editor, as [`crate::Game::save_ui`] carries it. The
/// contract is exactly "load, render, update": everything else about
/// the editor is the private layer's business.
pub trait SaveUi: Send + Sync {
    /// Bundle a parsed save (+ its already-patched ROM) into renderable
    /// data. `logo_games` is the cover art order — the loaded game
    /// first, then family siblings — resolved by the caller against its
    /// game registry.
    fn load(
        &'static self,
        game: crate::GameRef,
        patched_rom: Vec<u8>,
        save_path: std::path::PathBuf,
        save: crate::BoxedSave,
        patch: Option<AppliedPatch>,
        logo_games: &[crate::GameRef],
    ) -> SaveViewData;

    /// Fresh per-embed view state.
    fn new_state(&self) -> Box<dyn SaveViewState>;

    /// Render the save view. `play_button`: `None` hides the Play
    /// button, `Some(enabled)` renders it. `editable` gates the whole
    /// edit affordance (only the play tab passes true).
    fn view<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        data: &'a SaveViewData,
        state: &'a dyn SaveViewState,
        streamer_mode: bool,
        play_button: Option<bool>,
        inline_actions: bool,
        editable: bool,
    ) -> iced::Element<'a, std::sync::Arc<dyn SaveUiMessage>>;

    /// Fold a message into the state (and the data, when given — staged
    /// edits mutate it in place, including derived art). Returns a
    /// follow-up task plus whatever the app must act on.
    fn update(
        &self,
        state: &mut dyn SaveViewState,
        data: Option<&mut SaveViewData>,
        msg: &dyn SaveUiMessage,
    ) -> (iced::Task<std::sync::Arc<dyn SaveUiMessage>>, Option<SaveUiOutcome>);

    /// Serialize the current in-memory save (staged edits included) —
    /// what a netplay commitment or session launch runs on.
    fn sram(&self, data: &SaveViewData) -> Vec<u8>;
}
