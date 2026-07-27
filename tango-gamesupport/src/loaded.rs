//! The frontend's half of a loaded save: the art baked out of it.
//!
//! Only the *shape* lives here (public, because [`crate::Game`]'s
//! save-ui trait passes it around); the baking that fills it is view
//! code and lives in `tango-gamesupport-common::loaded`.
//!
//! The model — game, parsed save, assets, editability, applied patch —
//! is [`crate::model::SaveModel`], which knows nothing about a UI
//! toolkit. What is left here is everything derived *for drawing*: chip
//! icons, navi emblems, the pre-rendered NaviCust grid. Those are all
//! iced image handles, which is precisely why they can't live with the
//! model.
//!
//! [`OpenSave`] below bundles the two and derefs to the model, so
//! `loaded.save` / `loaded.assets` / `loaded.game` read the same as they
//! always did while `loaded.chip_icons` and friends stay frontend-local.
//! Assets are derived from the ROM and the save's WRAM; image handles
//! are derived from assets. All of it is rebuilt only when game or save
//! changes, so per-frame `view()` stays cheap.

use iced::widget::image as iced_image;
use std::collections::HashMap;

/// A loaded save's model plus this frontend's baked art for it.
pub struct OpenSave {
    /// Game, parsed save, assets, editability, applied patch. Reachable
    /// directly through `Deref`, so `loaded.save` still works.
    pub model: crate::model::SaveModel,
    /// The game's own save-editor UI — every game-shaped rendering
    /// question routes through this instead of tango probing the
    /// dataview generically. Resolved by the app's per-family registry
    /// at construction (the registry owns the feature gates).
    pub save_ui: &'static dyn crate::save_ui::SaveUi,
    pub chip_icons: Vec<Option<iced_image::Handle>>,
    /// Full-size chip images (variable dimensions) for hover previews.
    pub chip_images: Vec<Option<(u32, u32, iced_image::Handle)>>,
    pub element_icons: HashMap<usize, iced_image::Handle>,
    pub navi_emblems: HashMap<usize, iced_image::Handle>,
    /// Signature color per navi, extracted from its emblem pixels —
    /// drives the Link Navi card's plate/glow tint. Missing when the
    /// emblem is entirely monochrome (then the card falls back to a
    /// neutral accent).
    pub navi_accents: HashMap<usize, iced::Color>,
    /// Precomputed NaviCust grid image, from `view_navicust()`. None for
    /// link navis (no navicust) or when no navicust_layout is published.
    pub navicust_render: Option<NavicustRender>,
    /// Per-part shape thumbnails (compressed footprint, in the part's
    /// color) for the navicust editor palette, as `(width, height,
    /// handle)`. Indexed by part id; `None` = no shape / no color. Baked
    /// once here so the per-frame palette just clones handles.
    pub navicust_part_icons: Vec<Option<(u32, u32, iced_image::Handle)>>,
    /// Cropped shape thumbnails, one per *installed navicust slot*, baked at
    /// that slot's actual rotation + compression, so the read-only Navi
    /// tab's inline parts list shows each part as it sits in the grid rather
    /// than its default footprint. Trimmed to the shape's bounding box (the
    /// grid-sized transparent margin the palette wants would just push the
    /// name text away). Indexed by navicust slot; `None` for an empty slot
    /// or a part with no color / shape. Empty for saves without a navicust.
    /// Rebuilt by [`OpenSave::rebuild_navicust_render`].
    pub navicust_installed_part_thumbs: Vec<Option<(u32, u32, iced_image::Handle)>>,
    /// Logos for the Cover tab, as `(width, height, handle)`. The
    /// loaded game's own variant comes first; any sibling variants in
    /// the family follow (so families with two logos — Gregar/Falzar
    /// etc. — can fan both out). Empty when the game has no per-game
    /// `Game` registration. Built once here so the per-frame view()
    /// just clones the handles.
    pub logos: Vec<(u32, u32, iced_image::Handle)>,
}

impl std::ops::Deref for OpenSave {
    type Target = crate::model::SaveModel;
    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

impl std::ops::DerefMut for OpenSave {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.model
    }
}

/// Cached NaviCust image plus everything needed to translate a pointer
/// position over the displayed image back to a part index (for hover
/// highlighting in the parts list).
pub struct NavicustRender {
    pub source_w: u32,
    pub source_h: u32,
    pub handle: iced_image::Handle,
    /// Top-left of the cell grid in source-image coordinates.
    pub body_origin_x: f32,
    pub body_origin_y: f32,
    /// Edge length of one cell in source-image coordinates.
    pub cell_size: f32,
    pub cols: usize,
    pub rows: usize,
    /// Flat row-major materialized grid; `None` = empty cell, `Some(i)`
    /// = `navicust_part(i)` index.
    pub cell_part_idx: Vec<Option<usize>>,
}
