//! One loaded save: the game it belongs to, the parsed save itself, the
//! ROM assets derived from both, and what can be edited in it.
//!
//! # What is *not* here
//!
//! Anything rendered. [`SaveModel`] used to carry baked UI-toolkit image
//! handles — chip icons, navi emblems, the NaviCust grid picture —
//! alongside the model, which is what tied the whole save view to one
//! frontend. Those are now the frontend's to derive from `assets` and to
//! re-derive when [`edit::apply`] reports an [`edit::Invalidation`].
//!
//! Everything here is a pure transformation over bytes already in
//! memory, so there is no storage seam and no async: the caller supplies
//! the ROM and the save, and gets back a model.

pub mod edit;

pub use crate::GameRef;

/// The currently committed patch (name + version + arc to the
/// per-version metadata). Held alongside the loaded ROM so refresh
/// decisions know whether the active selection still matches.
#[derive(Clone)]
pub struct AppliedPatch {
    pub name: String,
    pub version: semver::Version,
    /// The patch's `[rom_overrides]`, as scanned from its package — all
    /// the model needs (charset + name overrides). The full scanner-side
    /// version metadata stays in tango-library, which this crate must
    /// not depend on (the registry there points back at the game crates
    /// that depend on us).
    pub rom_overrides: tango_patch::Overrides,
}

/// Which sections of a loaded save can be edited in place. Each flag is a
/// pure capability probe — `view_*_mut().is_some()` — which needs `&mut save`,
/// so it's computed once and cached on the immutable [`SaveModel`] (a frontend's
/// per-frame render only holds `&SaveModel`, and the read-only `view_*()` probes
/// answer a different question: BN3 has a viewable-but-not-writable navicust,
/// BN1–4 a viewable-but-not-writable navi). Swapping the equipped navi flips
/// some of these (a link navi has no navicust / patch cards), so re-probe via
/// [`SaveModel::refresh_editability`] after any in-memory edit that can change
/// capability.
#[derive(Clone, Copy, Default)]
pub struct Editability {
    /// `view_chips_mut().is_some()` — drives the Folder tab's Edit button.
    pub folder: bool,
    /// `view_navicust_mut().is_some()` (BN4/5/6, and not a link navi).
    pub navicust: bool,
    /// `view_navi_mut().is_some()` — the equipped navi (BN5/BN6/BN4.5).
    pub navi: bool,
    /// `view_patch_card56s_mut().is_some()` — the BN5/BN6 list. BN4's
    /// slot-based Mod Cards are that game's own model; its UI crate
    /// answers for their editability itself (`SaveUi::tab_editable`).
    pub patch_cards: bool,
    /// `view_auto_battle_data_mut().is_some()` (BN4/BN5).
    pub auto_battle_data: bool,
}

impl Editability {
    /// Whether *any* section is editable — drives the single save-level Edit
    /// button (once open, the user navigates tabs to edit each section).
    pub fn any(&self) -> bool {
        self.folder || self.navicust || self.navi || self.patch_cards || self.auto_battle_data
    }
}

/// A committed game + save, with the assets derived from the pair.
pub struct SaveModel {
    pub game: GameRef,
    pub save_path: std::path::PathBuf,
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    /// Which sections of this save can be edited in place. See [`Editability`].
    pub editability: Editability,
    /// Patch+version baked into this SaveModel, if any. `None` = raw ROM.
    pub patch: Option<AppliedPatch>,
    pub assets: Box<dyn tango_dataview::rom::Assets + Send + Sync>,
}
