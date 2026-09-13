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

/// A complete opponent-facing warning report. Implementations retain their
/// game-specific findings and display snapshot; the host only asks for the
/// localized rows when it renders the advisory.
pub trait BuildWarnings: std::fmt::Debug + Send + Sync {
    fn format(&self, lang: &LanguageIdentifier) -> Vec<String>;
}

/// Type-erased warning report retained by the game-agnostic host.
pub type OpaqueBuildWarnings = std::sync::Arc<dyn BuildWarnings>;

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

/// A message minted inside the save-editor view. Implemented (and
/// consumed) only by the private UI layer; the app routes
/// `Arc<dyn SaveEditorMessage>` through its message enums without looking
/// inside (the `Arc` keeps it cheaply `Clone`, as iced messages must
/// be).
pub trait SaveEditorMessage: std::any::Any + std::fmt::Debug + Send + Sync {}

/// Opaque per-save view state (active tab, edit session, scroll and
/// animation bookkeeping), held as [`LoadedSave::state`]. Minted by
/// [`SaveEditorFactory::load`] and dropped with the save it belongs to, so a
/// save switch or a closed view takes its view state with it and a
/// rebuilt save can never inherit a stale one — except for where the
/// reader was looking, which [`SaveEditor::carry_view_position`] hands
/// across a rebuild of the *same* save on purpose.
pub trait SaveEditorState: std::any::Any + Send + Sync {}

/// The private UI layer's loaded bundle (model + baked art), as held
/// behind [`LoadedSave::payload`]. Opaque by design; only that layer
/// implements and reads it.
pub trait LoadedSavePayload: std::any::Any + Send + Sync {}

/// A parsed save plus the effective assets derived from its ROM and patch,
/// prepared before either validation or presentation is involved.
pub struct PreparedSave {
    pub game: crate::GameRef,
    pub save_path: std::path::PathBuf,
    pub patch: Option<AppliedPatch>,
    pub save: crate::BoxedSave,
    pub assets: crate::BoxedAssets,
}

/// A loaded save, ready to render: the game-agnostic facts the app
/// needs for launching and committing (game, path, patch), the UI to
/// drive it with, the view state that UI is currently in, and the
/// private layer's loaded bundle behind an opaque payload.
pub struct LoadedSave {
    /// The save editor driving this data — render with
    /// [`SaveEditor::view`], mutate with [`SaveEditor::update`].
    pub editor: &'static dyn SaveEditor,
    /// Optional legacy identity for native launch and presentation.
    pub native_game: Option<crate::GameRef>,
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

impl PreparedSave {
    /// Let the family's editor consume the prepared data and add presentation
    /// state and baked art.
    pub fn load(self) -> LoadedSave {
        let editor = self.game.family.save_editor;
        editor.load(self)
    }
}

/// What the app must act on after an [`SaveEditor::update`] — deliberately
/// app-semantic only (clipboard, launches, disk writes); staged edits
/// are applied to the data internally and never surface.
#[derive(Debug)]
pub enum SaveEditorEvent {
    /// Copy plain text to the clipboard.
    CopyText(String),
    /// Copy HTML to the clipboard with `text` as its plain-text
    /// alternative. Rich-text paste targets receive `html`; plain-text
    /// targets keep the same text export as before.
    CopyHtml { text: String, html: String },
    /// Copy a raster image to the clipboard.
    CopyImage(image::RgbaImage),
    /// The embedder-defined Play button was pressed.
    Play,
    /// The embedder-defined Training button was pressed.
    Training,
    /// Write `sram` to the data's [`save_path`](LoadedSave::save_path), then
    /// acknowledge the result through [`SaveEditor::save_finished`]. Package
    /// editors retain their staged edits until this write succeeds.
    Commit { sram: Vec<u8> },
    /// The edit session was discarded — reload the on-disk original.
    Cancel,
}

/// Native model preparation is separate from the rendered editor. A package
/// editor can take over a loaded save without implementing native parsing.
pub trait SaveEditorFactory: SaveEditor {
    /// Validate an already-prepared save without constructing editor state or
    /// presentation assets.
    fn validate_save(&self, prepared: &PreparedSave) -> Option<crate::OpaqueBuildWarnings>;

    /// Decorate an already-prepared save model with renderable state and art.
    fn load(&'static self, prepared: PreparedSave) -> LoadedSave;
}

/// Render and update an already loaded save. Concrete model and presentation
/// shapes stay private; the same panel can host native or package-owned editors.
pub trait SaveEditor: Send + Sync {
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
    /// on. Keep the task even when events are returned. `lang` feeds localized
    /// clipboard exports; `theme` supplies the active colors for rendered images.
    fn update(
        &self,
        lang: &LanguageIdentifier,
        data: &mut LoadedSave,
        msg: &dyn SaveEditorMessage,
        theme: &iced::Theme,
    ) -> (iced::Task<std::sync::Arc<dyn SaveEditorMessage>>, Vec<SaveEditorEvent>);

    /// Serialize the current in-memory save (staged edits included) —
    /// what a netplay commitment or session launch runs on. Failure must stop
    /// the launch/commit; never substitute an older snapshot or an empty save.
    fn sram(&self, data: &LoadedSave) -> Result<Vec<u8>, String>;

    /// Complete a synchronous write requested by `Commit`. Always call this,
    /// including for missing paths and failed writes. No further editor action
    /// should be dispatched between the request and its acknowledgment.
    fn save_finished(&self, _data: &mut LoadedSave, _result: Result<(), String>) {}

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

    /// Restart the same presentation-only entrance used when this save is
    /// first loaded. Hosts call this when an already-loaded save becomes the
    /// active view again (for example, switching replay participants).
    fn restart_view_entrance(&self, _state: &mut dyn SaveEditorState) {}
}
