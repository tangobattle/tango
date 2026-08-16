//! The save-model *operations*: building a [`SaveModel`] from a patched
//! ROM, the capability probe, and the staged-edit appliers. The data
//! shapes themselves (SaveModel, Editability, the edit enums) are
//! public API on `tango_gamesupport::model` — re-exported here so this
//! crate and the app see one namespace; the logic stays private.

pub mod edit;
pub mod rom_overrides;
pub mod rules;

pub use edit::*;
pub use tango_gamesupport::GameRef;

use rom_overrides::OverridenAssets;

pub use tango_gamesupport::AppliedPatch;

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
    /// answers for their editability itself (`SaveEditor::tab_editable`).
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
    pub save: Box<dyn crate::dataview::save::Save + Send + Sync>,
    /// Which sections of this save can be edited in place. See [`Editability`].
    pub editability: Editability,
    /// Patch+version baked into this SaveModel, if any. `None` = raw ROM.
    pub patch: Option<AppliedPatch>,
    pub assets: Box<dyn crate::dataview::rom::Assets + Send + Sync>,
}

impl SaveModel {
    /// Whether the effective ROM dataview accepts this chip id. The patched
    /// assets wrapper has already replaced the base answer when the manifest
    /// supplies explicit ranges.
    pub fn chip_is_legal(&self, chip_id: usize) -> bool {
        self.assets.chip_is_legal(chip_id)
    }
}

/// Probe every section's writable view once. Constructing a mutable view
/// has no side effects, so this is a pure capability check.
pub fn probe_editability(save: &mut (dyn crate::dataview::save::Save + Send + Sync)) -> Editability {
    // Each `is_some()` gets its own statement so the borrowed view temporary
    // is dropped before the next probe — a single struct literal would keep
    // every mutable borrow of `save` alive at once.
    let folder = save.view_chips_mut().is_some();
    let navicust = save.view_navicust_mut().is_some();
    let navi = save.view_navi_mut().is_some();
    let patch_cards = save.view_patch_card56s_mut().is_some();
    let auto_battle_data = save.view_auto_battle_data_mut().is_some();
    Editability {
        folder,
        navicust,
        navi,
        patch_cards,
        auto_battle_data,
    }
}

/// Re-probe section [`Editability`] from the current in-memory save.
/// Swapping the equipped navi flips navicust / patch-card capability, so
/// the edit path calls this after a navi change to keep the cached flags
/// in sync.
pub fn refresh_editability(save: &mut SaveModel) {
    save.editability = probe_editability(&mut *save.save);
}

/// Build a [`SaveModel`] from a ROM that's *already* had its patch
/// applied, plus the [`AppliedPatch`] that produced it (`None` for a raw
/// ROM). Never touches the BPS patch — the caller holds the patched
/// image; this just probes editability and derives the (override-
/// layered) assets.
pub fn from_patched_rom(
    game: GameRef,
    rom: Vec<u8>,
    save_path: std::path::PathBuf,
    mut save: Box<dyn crate::dataview::save::Save + Send + Sync>,
    applied_patch: Option<AppliedPatch>,
) -> SaveModel {
    let editability = probe_editability(&mut *save);

    let wram = save.as_raw_wram().into_owned();
    let charset_owned: Option<Vec<&str>> = applied_patch
        .as_ref()
        .and_then(|p| p.rom_overrides.charset.as_ref())
        .map(|c| c.iter().map(|s| s.as_str()).collect());
    // A netplay-only game has no ROM assets behind its save — bake from
    // empty ones, and the editor shell renders its empty state.
    let inner: Box<dyn crate::dataview::rom::Assets + Send + Sync> =
        match game.load_rom_assets(&rom, &wram, charset_owned.as_deref()) {
            Some(assets) => crate::dataview::unwrap_assets(assets),
            None => Box::new(crate::dataview::rom::EmptyAssets),
        };
    let overrides = applied_patch
        .as_ref()
        .map(|p| p.rom_overrides.clone())
        .unwrap_or_default();
    let assets: Box<dyn crate::dataview::rom::Assets + Send + Sync> = Box::new(OverridenAssets::new(inner, overrides));

    SaveModel {
        game,
        save_path,
        save,
        editability,
        patch: applied_patch,
        assets,
    }
}
