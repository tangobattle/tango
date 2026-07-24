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
pub mod rom_overrides;
pub mod rules;

use rom_overrides::OverridenAssets;
use std::sync::Arc;

pub use tango_gamesupport::GameRef;

/// The currently committed patch (name + version + arc to the
/// per-version metadata). Held alongside the loaded ROM so refresh
/// decisions know whether the active selection still matches.
#[derive(Clone)]
pub struct AppliedPatch {
    pub name: String,
    pub version: semver::Version,
    pub version_meta: Arc<tango_library::patch::Version>,
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
    /// `view_patch_cards_mut().is_some()` — BN4 (PatchCard4s, slot-based) and
    /// BN5/BN6 (PatchCard56s, list-based); each gets its own editor.
    pub patch_cards: bool,
    /// `view_auto_battle_data_mut().is_some()` (BN4/BN5).
    pub auto_battle_data: bool,
}

impl Editability {
    /// Probe every section's writable view once. Constructing a mutable view
    /// has no side effects, so this is a pure capability check.
    fn probe(save: &mut (dyn tango_dataview::save::Save + Send + Sync)) -> Self {
        // Each `is_some()` gets its own statement so the borrowed view temporary
        // is dropped before the next probe — a single struct literal would keep
        // every mutable borrow of `save` alive at once.
        let folder = save.view_chips_mut().is_some();
        let navicust = save.view_navicust_mut().is_some();
        let navi = save.view_navi_mut().is_some();
        let patch_cards = save.view_patch_cards_mut().is_some();
        let auto_battle_data = save.view_auto_battle_data_mut().is_some();
        Self {
            folder,
            navicust,
            navi,
            patch_cards,
            auto_battle_data,
        }
    }

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

impl SaveModel {
    /// Build from a *raw* (unpatched) ROM, applying the selected patch
    /// first. On apply failure we fall back to the unpatched ROM (and
    /// log) so the save view still renders. Callers that already hold the
    /// patched image should use [`from_patched_rom`] instead, to avoid
    /// applying the patch a second time.
    ///
    /// [`from_patched_rom`]: Self::from_patched_rom
    pub fn build(
        storage: &dyn tango_library::Storage,
        game: GameRef,
        rom: Vec<u8>,
        save_path: std::path::PathBuf,
        save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        patches_path: &std::path::Path,
        patch: Option<(String, semver::Version, Arc<tango_library::patch::Version>)>,
    ) -> Self {
        let (rom, applied_patch) = match patch {
            Some((name, version, meta)) => {
                match tango_library::patch::apply_patch(storage, &rom, game, patches_path, &name, &version) {
                    Ok(patched) => (
                        patched,
                        Some(AppliedPatch {
                            name,
                            version,
                            version_meta: meta,
                        }),
                    ),
                    Err(e) => {
                        log::error!(
                            "failed to apply patch {name} v{version} to {:?}: {e}",
                            game.family_and_variant()
                        );
                        (rom, None)
                    }
                }
            }
            None => (rom, None),
        };
        Self::from_patched_rom(game, rom, save_path, save, applied_patch)
    }

    /// Build from a ROM that's *already* had its patch applied, plus the
    /// [`AppliedPatch`] that produced it (`None` for a raw ROM). Unlike
    /// [`build`], this never touches the BPS patch — use it when the
    /// caller already holds the patched image (e.g. a live session that
    /// patched the ROM for the emulator) so the patch isn't re-applied
    /// just to read the asset overrides + charset off `applied_patch`.
    ///
    /// [`build`]: Self::build
    pub fn from_patched_rom(
        game: GameRef,
        rom: Vec<u8>,
        save_path: std::path::PathBuf,
        mut save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        applied_patch: Option<AppliedPatch>,
    ) -> Self {
        // Probe section editability once (each needs `&mut save`, but a
        // per-frame render only holds `&SaveModel`). Constructing a mutable view
        // has no side effects, so this is a pure capability check we can cache.
        let editability = Editability::probe(&mut *save);

        let wram = save.as_raw_wram().into_owned();
        let charset_owned: Option<Vec<&str>> = applied_patch
            .as_ref()
            .and_then(|p| p.version_meta.rom_overrides.charset.as_ref())
            .map(|c| c.iter().map(|s| s.as_str()).collect());
        let inner = game.load_rom_assets(&rom, &wram, charset_owned.as_deref());
        let overrides = applied_patch
            .as_ref()
            .map(|p| p.version_meta.rom_overrides.clone())
            .unwrap_or_default();
        let assets: Box<dyn tango_dataview::rom::Assets + Send + Sync> =
            Box::new(OverridenAssets::new(inner, overrides));

        Self {
            game,
            save_path,
            save,
            editability,
            patch: applied_patch,
            assets,
        }
    }

    /// Re-probe section [`Editability`] from the current in-memory save.
    /// Swapping the equipped navi flips navicust / patch-card capability, so
    /// the edit path calls this after a navi change to keep the cached flags
    /// in sync.
    pub fn refresh_editability(&mut self) {
        self.editability = Editability::probe(&mut *self.save);
    }
}
