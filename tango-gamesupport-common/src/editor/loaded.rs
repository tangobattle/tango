//! Baking a [`OpenSave`]: every image handle the save view draws from,
//! derived once per game+save so the per-frame `view()` only clones
//! handles. The `OpenSave` shape itself is public API on
//! `tango_gamesupport::loaded` (re-exported here); the baking is view
//! code and stays private.

use crate::model::GameRef;
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
    pub save_editor: &'static dyn crate::editor::GameSaveEditor,
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
    /// Logos for the Cover tab, as `(width, height, handle)` — one per
    /// variant in the loaded game's family, in the family's own order
    /// (so families with two logos — Gregar/Falzar etc. — fan both out,
    /// the same way round whichever one is loaded). Built once here so
    /// the per-frame view() just clones the handles.
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

/// The public chip table for [`tango_gamesupport::LoadedSave::chips`] —
/// what the embedder's own chip-labelled views (the match-analysis
/// chart's chip lanes) read, since they have chip ids and a loaded save
/// but no way into this crate's [`OpenSave`]. Icons come straight from
/// the bake above, so this is a name lookup plus refcount bumps.
pub fn chip_display_table(open: &OpenSave) -> Vec<tango_gamesupport::ChipDisplay> {
    (0..open.assets.num_chips())
        .map(|id| tango_gamesupport::ChipDisplay {
            name: open.assets.chip(id).and_then(|c| c.name()),
            icon: open.chip_icons.get(id).cloned().flatten(),
        })
        .collect()
}

/// Bake every image handle the save view draws from, once per
/// game+save, so the per-frame `view()` only clones handles.
pub fn from_model(
    model: crate::model::SaveModel,
    save_editor: &'static dyn crate::editor::GameSaveEditor,
) -> OpenSave {
    let assets = model.assets.as_ref();

    // Chip icons (14x14 cropped from 16x16) + full chip images for
    // hover previews. Both lazy per id; pre-pass once at load time.
    let mut chip_icons: Vec<Option<iced_image::Handle>> = Vec::with_capacity(assets.num_chips());
    let mut chip_images: Vec<Option<(u32, u32, iced_image::Handle)>> = Vec::with_capacity(assets.num_chips());
    for id in 0..assets.num_chips() {
        let chip = assets.chip(id);
        chip_icons.push(chip.as_ref().map(|c| cropped_handle(&c.icon(), 1, 1, 14, 14)));
        chip_images.push(chip.as_ref().map(|c| {
            let img = c.image();
            let (w, h) = (img.width(), img.height());
            (w, h, iced_image::Handle::from_rgba(w, h, img.into_raw()))
        }));
    }

    // Element icons: element ids are sparse; try the small id space.
    let mut element_icons = HashMap::new();
    for id in 0..32 {
        if let Some(img) = assets.element_icon(id) {
            element_icons.insert(id, cropped_handle(&img, 1, 1, 14, 14));
        }
    }

    // Navi emblems for LinkNavi games: 15x15 from (1,0). The accent
    // color (most prominent saturated pixel color) is pulled from the
    // same crop for the Link Navi card's tinting.
    let mut navi_emblems = HashMap::new();
    let mut navi_accents = HashMap::new();
    for id in 0..assets.num_navis() {
        if let Some(navi) = assets.navi(id) {
            let crop = image::imageops::crop_imm(&navi.emblem(), 1, 0, 15, 15).to_image();
            if let Some(accent) = emblem_accent(&crop) {
                navi_accents.insert(id, accent);
            }
            let (w, h) = crop.dimensions();
            navi_emblems.insert(id, iced_image::Handle::from_rgba(w, h, crop.into_raw()));
        }
    }

    // Render the NaviCust grid once per save+game.
    let navicust_render = build_navicust_render(model.save.as_ref(), assets, model.game);

    // Bake the grid-sized shape thumbnail per navicust part for the
    // editor palette (aligned blocks).
    let mut navicust_part_icons: Vec<Option<(u32, u32, iced_image::Handle)>> =
        Vec::with_capacity(assets.num_navicust_parts());
    for id in 0..assets.num_navicust_parts() {
        let img = assets.navicust_part(id).and_then(|info| {
            let color = info.color()?;
            crate::editor::view::navicust::grid::render_part_thumb(
                &info.compressed_bitmap().unwrap_or_else(|| info.uncompressed_bitmap()),
                color,
                info.is_solid(),
                false,
            )
        });
        navicust_part_icons.push(img.map(|img| {
            let (w, h) = (img.width(), img.height());
            (w, h, iced_image::Handle::from_rgba(w, h, img.into_raw()))
        }));
    }
    // Logos for the Cover tab: every variant in this game's family, in
    // the family's own order. The per-game `LazyImage` caches the PNG
    // decode; `to_rgba8` + `from_rgba` run once here so the per-frame
    // view() just clones handles.
    let logos: Vec<(u32, u32, iced_image::Handle)> = model
        .game
        .family
        .games
        .iter()
        // A game with no logo (netplay-only) simply contributes none.
        .filter_map(|gi| {
            let img = gi.logo_image?.to_rgba8();
            let (w, h) = img.dimensions();
            Some((w, h, iced_image::Handle::from_rgba(w, h, img.into_raw())))
        })
        .collect();

    OpenSave {
        model,
        save_editor,
        chip_icons,
        chip_images,
        element_icons,
        navi_emblems,
        navi_accents,
        navicust_render,
        navicust_part_icons,
        logos,
    }
}

/// Recompute the baked NaviCust grid image from the current in-memory
/// save. The navicust editor commits edits into the save (and rebuilds
/// the materialized WRAM cache) without triggering a full `OpenSave`
/// rebuild, so this cached image would otherwise stay stale until the
/// next reselection.
pub fn rebuild_navicust_render(loaded: &mut OpenSave) {
    loaded.navicust_render = build_navicust_render(loaded.save.as_ref(), loaded.assets.as_ref(), loaded.game);
}

fn build_navicust_render(
    save: &(dyn crate::dataview::save::Save + Send + Sync),
    assets: &(dyn crate::dataview::rom::Assets + Send + Sync),
    game: GameRef,
) -> Option<NavicustRender> {
    let view = save.view_navicust()?;
    let layout = assets.navicust_layout()?;
    let materialized = view.materialized();

    // Mirrors the constants inside navicust.rs's tiny-skia render.
    const PADDING_H: f32 = crate::editor::view::navicust::grid::PADDING_H as f32;
    const PADDING_V: f32 = crate::editor::view::navicust::grid::PADDING_V as f32;
    const SQUARE_SIZE: f32 = crate::editor::view::navicust::grid::SQUARE_SIZE;
    const BORDER_WIDTH: f32 = crate::editor::view::navicust::grid::BORDER_WIDTH;
    let (rows, cols) = materialized.dim();

    let lang = region_language(game.region());
    // Render at native resolution; the iced widget paints it at the same
    // display width as the interactive editor, so iced scales the high-res
    // source down — keeping it crisp on HiDPI.
    let mut img =
        crate::editor::view::navicust::grid::render(&materialized, &layout, view.as_ref(), assets, &lang, None);
    let (handle_w, handle_h) = (img.width(), img.height());

    // Constant cell size across all games (the 7×7 cell size), so the image
    // grows/shrinks with the grid instead of every grid being squeezed to one
    // total width. Same scale `EditorGrid` uses, so viewer and editor match.
    let display_scale =
        crate::editor::view::navicust::grid::display_scale(crate::editor::view::navicust::editor::DISPLAY_W);
    let display_w = handle_w as f32 * display_scale;
    let display_h = handle_h as f32 * display_scale;
    // Round corners to ~4 display px (the pane's radius).
    mask_rounded_corners(&mut img, (4.0 / display_scale).round().max(1.0) as u32);
    let color_bar_h = (SQUARE_SIZE / 2.0 + BORDER_WIDTH).round();
    let body_origin_x = (PADDING_H + BORDER_WIDTH / 2.0) * display_scale;
    let body_origin_y = (PADDING_V + color_bar_h + PADDING_V + BORDER_WIDTH / 2.0) * display_scale;
    let cell_size = SQUARE_SIZE * display_scale;

    let cell_part_idx: Vec<Option<usize>> = materialized.iter().copied().collect();

    Some(NavicustRender {
        // source_w/h advertise the WIDGET (logical) size, not the
        // Handle's pixel size — the iced widget will paint at
        // these dimensions and iced handles the device-pixel scale.
        source_w: display_w.round() as u32,
        source_h: display_h.round() as u32,
        handle: iced_image::Handle::from_rgba(handle_w, handle_h, img.into_raw()),
        body_origin_x,
        body_origin_y,
        cell_size,
        cols,
        rows,
        cell_part_idx,
    })
}

/// Hard-clip the four corners of `img` to transparency so the iced
/// Image widget renders with rounded corners. The pane plate behind it
/// shows through where alpha drops to 0. Iced's container clip(true)
/// only clips to a rectangle, so the rounding has to live in the
/// pixels.
fn mask_rounded_corners(img: &mut image::RgbaImage, radius: u32) {
    let (w, h) = (img.width(), img.height());
    let r = radius.min(w / 2).min(h / 2);
    if r == 0 {
        return;
    }
    let r_sq = (r as f32) * (r as f32);
    // Iterate just the bounding boxes of the four corner squares.
    let corners = [
        // (x_start, y_start, cx_anchor, cy_anchor)
        (0, 0, r as f32, r as f32),
        (w - r, 0, w as f32 - r as f32, r as f32),
        (0, h - r, r as f32, h as f32 - r as f32),
        (w - r, h - r, w as f32 - r as f32, h as f32 - r as f32),
    ];
    for (x0, y0, cx_anchor, cy_anchor) in corners {
        for y in y0..(y0 + r) {
            for x in x0..(x0 + r) {
                let dx = (x as f32 + 0.5) - cx_anchor;
                let dy = (y as f32 + 0.5) - cy_anchor;
                if dx * dx + dy * dy > r_sq {
                    img.get_pixel_mut(x, y).0[3] = 0;
                }
            }
        }
    }
}

fn cropped_handle(src: &image::RgbaImage, x: u32, y: u32, w: u32, h: u32) -> iced_image::Handle {
    let sub = image::imageops::crop_imm(src, x, y, w, h).to_image();
    iced_image::Handle::from_rgba(w, h, sub.into_raw())
}

/// The ROM region's display language, for region-sensitive baking (the
/// BN3 style label picks its script by this). Fixed mapping — this
/// crate has no game registry to consult, deliberately.
pub fn region_language(region: tango_gamesupport::Region) -> unic_langid::LanguageIdentifier {
    match region {
        tango_gamesupport::Region::US => unic_langid::langid!("en-US"),
        tango_gamesupport::Region::JP => unic_langid::langid!("ja-JP"),
    }
}

/// The emblem's signature color: every distinct opaque pixel color is
/// scored by frequency, weighted toward saturated mid-to-bright tones so
/// black outlines and white highlights (numerous but characterless) lose
/// to the emblem's actual identity color. `None` when nothing scores —
/// a fully transparent or pure black/white emblem.
fn emblem_accent(img: &image::RgbaImage) -> Option<iced::Color> {
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for p in img.pixels() {
        if p.0[3] >= 0x80 {
            *counts.entry([p.0[0], p.0[1], p.0[2]]).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter_map(|(rgb, n)| {
            let max = rgb.iter().copied().max().unwrap() as f32 / 255.0;
            let min = rgb.iter().copied().min().unwrap() as f32 / 255.0;
            let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
            // Saturation dominates; the small constant keeps a vivid-but-
            // rare color from losing to a huge near-gray field, and the
            // value factor zeroes out the black outline entirely.
            let score = n as f32 * (saturation + 0.05) * max;
            (score > 0.0).then_some((score, rgb))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, rgb)| iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]))
}
