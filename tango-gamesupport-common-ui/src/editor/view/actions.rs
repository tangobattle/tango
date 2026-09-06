//! Save-editor inputs and effects returned to the host.

use super::{AutoBattleDataSort, LibrarySort, NavicustSort, PatchCard56Sort, Tab};

/// What the host must do in response to an applied [`Action`] —
/// everything the save view can't do itself because it needs App-level
/// collaborators (the in-memory loaded save, the clipboard, the
/// session host).
pub enum Outcome {
    /// Stage one edit into the loaded save in memory (the UI reads it
    /// live; nothing hits disk until [`Outcome::Commit`]).
    Edit(crate::model::edit::Edit),
    /// Copy plain text to the clipboard.
    CopyText(String),
    /// Copy HTML to the clipboard with a plain-text alternative.
    CopyHtml { text: String, html: String },
    /// Copy a raster image to the clipboard.
    CopyImage(image::RgbaImage),
    /// The embedder-defined Play button was pressed.
    Play,
    /// The embedder-defined Training button was pressed.
    Training,
    /// Write every staged edit (folder + navicust + patch cards + auto
    /// battle data) to the .sav on disk in one shot.
    Commit,
    /// Discard all staged edits, reloading the on-disk original.
    Cancel,
}

/// User-driven changes the embedded save view wants to surface. The
/// caller `.map`s its top-level Message onto this and dispatches:
/// most variants just need `state.apply(&action)`; the Copy
/// variants need the caller's `tab_as_text` / `tab_as_image` plus
/// a clipboard write.
#[derive(Debug, Clone)]
pub enum Action {
    /// Leave the streamer-mode cover and reveal the regular save viewer.
    Review,
    SelectTab(Tab),
    /// The sub-tab strip was scrolled; carries the new relative x offset
    /// (0..=1), used only to drive the strip's edge fades.
    TabScrolled(f32),
    ToggleFolderGrouped(bool),
    CopyTab(Tab),
    CopyTabImage(Tab),
    /// Embedder-defined "start single-player here" action.
    /// Emitted by the Play button rendered in the editor tab
    /// strip when [`super::view`] is called with `play_button = Some(_)`.
    /// The play tab routes this to `Effect::StartSinglePlayer`;
    /// other embedders (replay, opponent panel) pass `None` and
    /// the button isn't rendered.
    PlayClicked,
    /// Embedder-defined "start training here" action, routed by the
    /// play tab to `Effect::StartTraining`. Nothing raises it while the
    /// Training button is hidden (see the actions row in [`super::view`]); the
    /// route stays wired so restoring the button is a local change.
    #[allow(dead_code)]
    TrainingClicked,
    // ----- Folder editor (only emitted when `view`'s `editable` is set) -----
    /// Enter folder edit mode. The play tab seeds tag state via
    /// [`super::State::enter_edit`]; the rest is handled in [`super::State::apply`].
    /// Edits are staged live in the loaded save but not written to disk
    /// until [`Action::SaveEdit`].
    EnterEdit,
    /// Focus the navi picker as the edit body — fired by clicking the navi
    /// strip's card, which is only a button while the global edit session is
    /// open. The navi has no tab of its own, so this points the body at the
    /// picker (handled in [`super::State::apply`]); the host opens the session if it
    /// somehow isn't already (it needs `&OpenSave`, like [`Action::EnterEdit`]).
    EnterEditNavi,
    /// Finish editing: commit the staged folder to the save file on
    /// disk, then leave edit mode.
    SaveEdit,
    /// Discard all staged edits (reverts the loaded save to the
    /// on-disk original) and leave edit mode.
    CancelEdit,
    /// Library pane: add this chip+code to the first empty folder slot.
    AddChip {
        chip_id: usize,
        code: crate::dataview::save::ChipCode,
    },
    /// Folder pane: empty `slot`.
    RemoveChip {
        slot: usize,
    },
    /// Folder pane: a drag-reorder gesture from the draggable folder list
    /// (carries sweeten's raw [`sweeten::widget::drag::DragEvent`]; only a completed drop between two
    /// filled slots actually moves a chip — see the play tab's handler).
    ReorderChips(sweeten::widget::drag::DragEvent),
    /// Folder pane: empty every slot (and clear REG/TAG).
    ClearFolder,
    /// Toggle `slot` as the folder's Regular chip — set it, or clear it
    /// if it's already the regular chip.
    ToggleRegular {
        slot: usize,
    },
    /// Toggle `slot`'s membership in the Tag chip pair.
    ToggleTag {
        slot: usize,
    },
    // ----- Slot-targeted deck editor (BCC program deck; only emitted
    // when `editable` is set) -----
    /// Board pane: aim the library at this deck slot (`None` clears the
    /// selection).
    SelectDeckSlot(Option<usize>),
    /// Library pane: install this chip into deck slot `slot`, replacing
    /// whatever occupied it.
    SetDeckChip {
        slot: usize,
        chip_id: usize,
        code: crate::dataview::save::ChipCode,
    },
    /// Board pane: empty deck slot `slot`.
    ClearDeckChip {
        slot: usize,
    },
    /// Library pane: the filter text changed.
    LibraryFilterChanged(String),
    /// Library pane: the sort order changed.
    LibrarySortChanged(LibrarySort),
    // ----- Navicust editor (only emitted when `editable` is set) -----
    /// Palette: pick up part `id` in the orientation shown in the picker.
    PickUpPalettePart {
        id: usize,
    },
    /// Rotate the held part 90° clockwise (grid scroll-wheel).
    RotateHeld,
    /// Palette: rotate this part's picker entry 90° clockwise.
    RotatePart {
        id: usize,
    },
    /// Palette: toggle this part's picker entry between its compressed
    /// and uncompressed shape.
    ToggleCompressPart {
        id: usize,
    },
    /// Drop the held part without placing it.
    ClearHeld,
    /// Place the held part with its center on grid cell `(col, row)`.
    PlaceHeld {
        col: u8,
        row: u8,
    },
    /// Pick an installed part back up — it's removed and becomes held.
    /// `(col, row)` is the cell that was clicked, so the part can be
    /// grabbed at that point rather than re-centered on the cursor.
    PickUpInstalledPart {
        slot: usize,
        col: u8,
        row: u8,
    },
    /// Remove every installed part.
    ClearNavicust,
    // ----- Navi editor (only emitted when `editable` is set) -----
    /// Set the equipped navi to this index.
    SetNavi(usize),
    /// Palette: the filter text changed.
    NavicustFilterChanged(String),
    /// Palette: the sort order changed.
    NavicustSortChanged(NavicustSort),
    // ----- BN5/BN6 patch-card editor (only emitted when `editable` is set) -----
    /// Library pane: register patch card `id` (appended to the list,
    /// enabled).
    AddPatchCard56 {
        id: usize,
    },
    /// List pane: unregister the patch card in `slot`.
    RemovePatchCard56 {
        slot: usize,
    },
    /// List pane: a drag-reorder gesture (carries sweeten's raw [`sweeten::widget::drag::DragEvent`];
    /// only a completed drop reorders — see the play tab's handler).
    ReorderPatchCard56s(sweeten::widget::drag::DragEvent),
    /// List pane: unregister every patch card.
    ClearPatchCard56s,
    /// Library pane: the filter text changed.
    PatchCard56FilterChanged(String),
    /// Library pane: the sort order changed.
    PatchCard56SortChanged(PatchCard56Sort),
    // ----- Game-specific editors (only emitted when `editable` is set) -----
    /// A staged edit whose model belongs to one game (BN4's Mod Cards):
    /// the game's UI crate builds a [`crate::model::edit::GameEdit`]
    /// and this carries it to the host unchanged. `Arc` keeps the
    /// message `Clone`.
    Game(std::sync::Arc<dyn crate::model::edit::GameEdit>),
    // ----- Auto Battle Data editor (only emitted when `editable` is set) -----
    /// Library pane: set chip `id`'s primary use count (the count that
    /// drives the materialized deck for every section).
    SetChipUseCount {
        id: usize,
        count: usize,
    },
    /// Library pane: set chip `id`'s secondary use count (drives the
    /// secondary-standard section — only meaningful for Standard chips).
    SetSecondaryChipUseCount {
        id: usize,
        count: usize,
    },
    /// Deck pane: zero every chip's use counts, emptying the deck.
    ClearAutoBattleData,
    /// Library pane: the filter text changed.
    AutoBattleDataFilterChanged(String),
    /// Library pane: the sort order changed.
    AutoBattleDataSortChanged(AutoBattleDataSort),
}
