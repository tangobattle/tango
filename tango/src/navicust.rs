//! NaviCust grid rendering. Ported from `tango/src/gui/save_view/navi_view/navicust_view.rs`.
//! Outputs an RGBA image we can hand to iced's image widget. For BN3 the
//! color bar carries the style name on its left edge — rasterized through
//! cosmic-text (the same shaper iced uses, via iced's shared font system)
//! so script-aware font fallback picks up the bundled JP / SC / TC Noto
//! faces for non-Latin style names instead of tofu-ing out.

use iced::advanced::graphics::text::cosmic_text;
use iced::advanced::graphics::text::font_system as iced_font_system;
use iced::{Color, Point, Size};
use iced_graphics::geometry::{self, LineCap, Path, Stroke};
use std::sync::LazyLock;
use std::sync::Mutex;
use tango_dataview::{
    navicust::MaterializedNavicust,
    rom::{Assets, NavicustLayout, NavicustPartColor},
    save::NavicustView,
};

/// Glyph-pixel cache. cosmic-text rasterizes glyphs through swash; the
/// cache is just memoization keyed by (face, size, glyph) — no locale
/// or font state, so a single static is fine.
static SWASH_CACHE: LazyLock<Mutex<cosmic_text::SwashCache>> =
    LazyLock::new(|| Mutex::new(cosmic_text::SwashCache::new()));

// Native render units. The editor draws vector at a display scale, so
// these only set the baked image's resolution (viewer + clipboard); kept
// high so the viewer stays crisp at the editor's display size. Their
// ratios match the original (BORDER = SQUARE_SIZE/10).
pub const BORDER_WIDTH: f32 = 12.0;
pub const SQUARE_SIZE: f32 = 120.0;

const BG_FILL_COLOR: [u8; 4] = [0x20, 0x20, 0x20, 0xff];
const BORDER_STROKE_COLOR: [u8; 4] = [0x00, 0x00, 0x00, 0xff];

/// Solid color (filled square) + plus/stroke color, matching the egui app.
pub fn part_colors(color: NavicustPartColor) -> ([u8; 4], [u8; 4]) {
    match color {
        NavicustPartColor::Red => ([0xde, 0x10, 0x00, 0xff], [0xbd, 0x00, 0x00, 0xff]),
        NavicustPartColor::Pink => ([0xde, 0x8c, 0xc6, 0xff], [0xbd, 0x6b, 0xa5, 0xff]),
        NavicustPartColor::Yellow => ([0xde, 0xde, 0x00, 0xff], [0xbd, 0xbd, 0x00, 0xff]),
        NavicustPartColor::Green => ([0x18, 0xc6, 0x00, 0xff], [0x00, 0xa5, 0x00, 0xff]),
        NavicustPartColor::Blue => ([0x29, 0x84, 0xde, 0xff], [0x08, 0x60, 0xb8, 0xff]),
        NavicustPartColor::White => ([0xde, 0xde, 0xde, 0xff], [0xbd, 0xbd, 0xbd, 0xff]),
        NavicustPartColor::Orange => ([0xde, 0x7b, 0x00, 0xff], [0xbd, 0x5a, 0x00, 0xff]),
        NavicustPartColor::Purple => ([0x94, 0x00, 0xce, 0xff], [0x73, 0x00, 0xad, 0xff]),
        NavicustPartColor::Gray => ([0x84, 0x84, 0x84, 0xff], [0x63, 0x63, 0x63, 0xff]),
    }
}

pub const PADDING_H: u32 = 40;
pub const PADDING_V: u32 = 40;

/// Map a BCP-47 language id to the Noto font family name we want
/// cosmic-text to prefer when rasterizing the BN3 style label.
/// Latin scripts get plain "Noto Sans"; JP/SC/TC each get their
/// dedicated face so Han-unified codepoints render with the right
/// regional glyph forms.
fn family_for_locale(lang: &unic_langid::LanguageIdentifier) -> &'static str {
    use std::str::FromStr;
    let mut lang = lang.clone();
    lang.maximize();
    match lang.script {
        Some(s) if s == unic_langid::subtags::Script::from_str("Jpan").unwrap() => "Noto Sans JP",
        Some(s) if s == unic_langid::subtags::Script::from_str("Hans").unwrap() => "Noto Sans SC",
        Some(s) if s == unic_langid::subtags::Script::from_str("Hant").unwrap() => "Noto Sans TC",
        _ => "Noto Sans",
    }
}

/// Background fill + color bar + body. For BN3 (style is Some)
/// the bar is widened to span the body and the style name is
/// rasterized on the left edge — same layout the legacy egui
/// app produces.
///
/// `lang` selects the Noto face used to bake the BN3 style label
/// (see `family_for_locale`).
///
/// `target_w`: if `Some(w)`, the composed image is Lanczos-resampled
/// down to width `w` BEFORE the style label is baked in, then the
/// label is rasterized at display resolution so its glyphs are
/// pixel-crisp instead of being a re-scaled high-res blur. Pass
/// `None` (clipboard / export path) to keep the full native size.
/// Rasterize a navicust to an RGBA image via the shared [`paint`] routine
/// (tiny-skia backend), then resize to `target_w` and bake the BN3 style
/// label on top. This is the clipboard / export path; live display goes
/// through the same [`paint`] routine with an iced-canvas backend.
pub fn render(
    materialized: &MaterializedNavicust,
    layout: &NavicustLayout,
    view: &dyn NavicustView,
    assets: &dyn Assets,
    lang: &unic_langid::LanguageIdentifier,
    target_w: Option<u32>,
) -> image::RgbaImage {
    let model = build_model(materialized, layout, view, assets);
    let g = geometry(model.cols, model.rows);
    let native = rasterize(&model);

    // Resize the (label-free) composite to display size if a target was
    // requested. Nearest keeps the grid borders sharp. The BN3 label is
    // baked AFTER, at display resolution, so its glyphs stay crisp.
    let (mut out, scale) = match target_w {
        Some(w) if w < native.width() => {
            let scale = w as f32 / native.width() as f32;
            let new_h = (native.height() as f32 * scale).round() as u32;
            (
                image::imageops::resize(&native, w, new_h, image::imageops::FilterType::Nearest),
                scale,
            )
        }
        _ => (native, 1.0),
    };

    if let Some(name) = view.style().and_then(|sid| assets.style(sid).and_then(|s| s.name())) {
        // The BN3 bar's right-flush tile stripe is four SQUARE_SIZE/4
        // tiles plus the border; the label fills the gap to its left.
        let tiles_w_native = SQUARE_SIZE + BORDER_WIDTH;
        let label_pad = BORDER_WIDTH + 4.0;
        let max_label_w_native = g.body_w - tiles_w_native - label_pad * 2.0;
        if max_label_w_native > 0.0 {
            let label_x0 = ((PADDING_H as f32 + label_pad) * scale).round() as i64;
            let bar_y = (PADDING_V as f32 * scale).round() as i64;
            let bar_h = (g.bar_h * scale).round() as u32;
            let max_label_w = (max_label_w_native * scale).round() as u32;
            // 0.72 × bar height leaves a hair of vertical margin.
            let font_height = bar_h as f32 * 0.72;
            rasterize_label(
                &mut out,
                &name,
                label_x0,
                bar_y,
                bar_h,
                font_height,
                max_label_w,
                family_for_locale(lang),
            );
        }
    }
    out
}

/// Blit a left-aligned white label onto `dst` inside the rect
/// (x0, bar_y, max_width, bar_h), routing through cosmic-text via
/// iced's shared FontSystem. The font system already has all the
/// bundled Noto faces (Sans + JP + SC + TC + Emoji) loaded by
/// `main.rs::main`, so cosmic-text's script-aware fallback handles
/// CJK / emoji style names natively — no per-call font setup, no
/// tofu.
fn rasterize_label(
    dst: &mut image::RgbaImage,
    text: &str,
    x0: i64,
    bar_y: i64,
    bar_h: u32,
    font_height: f32,
    max_width: u32,
    family: &str,
) {
    let mut fs_lock = iced_font_system().write().expect("font system write lock");
    let font_system = fs_lock.raw();
    let mut swash = SWASH_CACHE.lock().unwrap();

    let line_height = font_height * 1.2;
    let metrics = cosmic_text::Metrics::new(font_height, line_height);
    let mut buffer = cosmic_text::Buffer::new(font_system, metrics);
    let mut buf = buffer.borrow_with(font_system);

    // Cap layout width so cosmic-text trims overflow on its own.
    buf.set_size(Some(max_width as f32), Some(line_height));
    // Primary family is the script-appropriate Noto face for the
    // game's locale (Sans / Sans JP / SC / TC). The shared FontSystem
    // still falls back across the other loaded faces for codepoints
    // the primary family lacks.
    let attrs = cosmic_text::Attrs::new().family(cosmic_text::Family::Name(family));
    buf.set_text(text, &attrs, cosmic_text::Shaping::Advanced, None);
    buf.shape_until_scroll(true);

    // Center the line vertically inside the bar's rect.
    let y_offset = bar_y + ((bar_h as f32 - line_height) / 2.0).round() as i64;
    let dst_w = dst.width() as i32;
    let dst_h = dst.height() as i32;
    let white = cosmic_text::Color::rgb(0xff, 0xff, 0xff);

    buf.draw(&mut swash, white, |gx, gy, gw, gh, color| {
        let a = color.a();
        if a == 0 {
            return;
        }
        let coverage = a as f32 / 255.0;
        let (sr, sg, sb) = (color.r() as f32, color.g() as f32, color.b() as f32);
        for dy in 0..gh as i32 {
            for dx in 0..gw as i32 {
                let ix = x0 as i32 + gx + dx;
                let iy = y_offset as i32 + gy + dy;
                if ix < 0 || iy < 0 || ix >= dst_w || iy >= dst_h {
                    continue;
                }
                let pixel = dst.get_pixel_mut(ix as u32, iy as u32);
                // Standard "src over dst" alpha blend. The label is
                // baked AFTER the bar has been composited onto the
                // opaque background, so the destination is already
                // opaque — we blend white onto the existing color
                // weighted by glyph coverage. The previous "max
                // alpha" path silently skipped every pixel because
                // it compared against alpha=255.
                let dr = pixel.0[0] as f32;
                let dg = pixel.0[1] as f32;
                let db = pixel.0[2] as f32;
                let inv = 1.0 - coverage;
                pixel.0[0] = (sr * coverage + dr * inv).round() as u8;
                pixel.0[1] = (sg * coverage + dg * inv).round() as u8;
                pixel.0[2] = (sb * coverage + db * inv).round() as u8;
                // Keep destination alpha (already opaque); no need
                // to touch pixel.0[3].
            }
        }
    });
}

const OOB_SHADE: [u8; 4] = [0x00, 0x00, 0x00, 0x80];
const GHOST_LEGAL: [u8; 4] = [0x33, 0xd9, 0x4d, 0xff];
const GHOST_ILLEGAL: [u8; 4] = [0xe6, 0x2e, 0x2e, 0xff];

/// Resolved style for one installed part (so drawing never reaches back
/// into the ROM assets).
#[derive(Clone, Copy)]
pub struct PartStyle {
    pub solid: [u8; 4],
    pub plus: [u8; 4],
    pub is_solid: bool,
}

/// Everything the shared [`paint`] routine needs, pre-resolved into owned
/// data so it can be held by a canvas widget or fed to the image backend.
#[derive(Clone)]
pub struct GridModel {
    pub cols: usize,
    pub rows: usize,
    pub command_line: usize,
    pub has_out_of_bounds: bool,
    pub background: [u8; 4],
    /// Row-major occupancy (`row * cols + col`): the part slot per cell.
    pub occupancy: Vec<Option<usize>>,
    /// Per-slot resolved style.
    pub part_styles: Vec<Option<PartStyle>>,
    pub is_bn3: bool,
    pub bar: Vec<Option<[u8; 4]>>,
}

/// A held part previewed under the cursor (editor only).
pub struct Ghost {
    /// In-bounds grid cells `(col, row)` the part covers — these get filled.
    pub cells: Vec<(usize, usize)>,
    /// The part's full footprint as signed grid coords (extends off-grid
    /// when the part clips offscreen). An edge is outlined only when its
    /// neighbor isn't in this set, so clipped (offscreen) edges stay open.
    pub footprint: Vec<(isize, isize)>,
    pub solid: [u8; 4],
    /// The part's "plus" (border/cross) color, used to draw the cross lines
    /// on non-solid parts — same as [`PartStyle::plus`].
    pub plus: [u8; 4],
    /// Whether the part is solid; non-solid parts get the plus/cross lines.
    pub is_solid: bool,
    pub legal: bool,
}

/// Native-coordinate layout of the composed navicust image.
pub struct Geometry {
    pub total_w: f32,
    pub total_h: f32,
    pub bar_h: f32,
    pub body_w: f32,
    pub body_origin_x: f32,
    pub body_origin_y: f32,
}

pub fn geometry(cols: usize, rows: usize) -> Geometry {
    let body_w = cols as f32 * SQUARE_SIZE + BORDER_WIDTH;
    let body_h = rows as f32 * SQUARE_SIZE + BORDER_WIDTH;
    let bar_h = SQUARE_SIZE / 2.0 + BORDER_WIDTH;
    Geometry {
        total_w: body_w + PADDING_H as f32 * 2.0,
        total_h: body_h + PADDING_V as f32 * 3.0 + bar_h,
        bar_h,
        body_w,
        body_origin_x: PADDING_H as f32,
        body_origin_y: PADDING_V as f32 + bar_h + PADDING_V as f32,
    }
}

/// Reference grid width, in columns, the on-screen cell size is pinned to.
/// 7 is the widest navicust (BN6); pinning the display scale here makes every
/// grid — viewer and editor — draw its cells at the 7×7 cell size, so the
/// image grows or shrinks with the grid instead of each grid being squeezed
/// to a single total width.
pub const REFERENCE_COLS: usize = 7;

/// The constant display scale applied to every navicust: the scale a
/// `REFERENCE_COLS`-wide grid needs to fit `display_w`. Cells therefore have a
/// fixed on-screen size across all games. Because 7 is the widest grid this
/// never upscales — narrower grids render as proportionally smaller images.
pub fn display_scale(display_w: f32) -> f32 {
    // `total_w` depends only on the column count, so the row count is moot.
    display_w / geometry(REFERENCE_COLS, REFERENCE_COLS).total_w
}

// Drawing primitives: thin wrappers over the iced canvas `Frame` so the
// shared paint routine reads in native coords + `[u8; 4]` colours. `s` is the
// display scale, applied to both coordinates and stroke widths (iced's frame
// transform wouldn't scale stroke width, so we scale here): the live editor
// passes its display factor, the baked image passes 1.0 (see `rasterize`).
// Both the window renderer and the standalone tiny-skia renderer expose the
// same `geometry::Frame`, so one set of helpers serves both.

fn fill_rect<R: geometry::Renderer>(
    frame: &mut geometry::Frame<R>,
    s: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [u8; 4],
) {
    frame.fill_rectangle(Point::new(x * s, y * s), Size::new(w * s, h * s), to_color(color));
}

fn stroke_rect<R: geometry::Renderer>(
    frame: &mut geometry::Frame<R>,
    s: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [u8; 4],
    width: f32,
) {
    let path = Path::rectangle(Point::new(x * s, y * s), Size::new(w * s, h * s));
    frame.stroke(
        &path,
        Stroke::default()
            .with_color(to_color(color))
            .with_width(width * s)
            .with_line_cap(LineCap::Square),
    );
}

fn stroke_line<R: geometry::Renderer>(
    frame: &mut geometry::Frame<R>,
    s: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    color: [u8; 4],
    width: f32,
) {
    let path = Path::line(Point::new(x1 * s, y1 * s), Point::new(x2 * s, y2 * s));
    frame.stroke(
        &path,
        Stroke::default()
            .with_color(to_color(color))
            .with_width(width * s)
            .with_line_cap(LineCap::Square),
    );
}

/// Fill a rectangle whose corners are rounded by `radius` (native units).
/// `radius <= 0` is a plain rectangle. Used for the outer background so the
/// editor's grid gets the same rounded corners the baked viewer image gets.
fn fill_round_rect<R: geometry::Renderer>(
    frame: &mut geometry::Frame<R>,
    s: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    color: [u8; 4],
) {
    if radius <= 0.0 {
        fill_rect(frame, s, x, y, w, h, color);
        return;
    }
    let path = Path::new(|b| {
        b.rounded_rectangle(
            Point::new(x * s, y * s),
            Size::new(w * s, h * s),
            iced::border::Radius::from(radius * s),
        );
    });
    frame.fill(&path, to_color(color));
}

/// Resolve a navicust view + ROM assets into a [`GridModel`]. `materialized`
/// is supplied by the caller so it can pass the WRAM cache (read-only view)
/// or a freshly recomputed grid (live editor).
pub fn build_model(
    materialized: &MaterializedNavicust,
    layout: &NavicustLayout,
    view: &dyn NavicustView,
    assets: &dyn Assets,
) -> GridModel {
    let (rows, cols) = materialized.dim();
    let occupancy: Vec<Option<usize>> = materialized.iter().copied().collect();

    let mut part_styles: Vec<Option<PartStyle>> = vec![None; view.count()];
    for i in 0..view.count() {
        let Some(part) = view.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let Some(c) = info.color() else { continue };
        let (solid, plus) = part_colors(c);
        part_styles[i] = Some(PartStyle {
            solid,
            plus,
            is_solid: info.is_solid(),
        });
    }

    GridModel {
        cols,
        rows,
        command_line: layout.command_line,
        has_out_of_bounds: layout.has_out_of_bounds,
        background: layout.background.0,
        occupancy,
        part_styles,
        is_bn3: view.style().is_some(),
        bar: view
            .navicust_color_bar()
            .into_iter()
            .map(|c| c.map(|c| part_colors(c).1))
            .collect(),
    }
}

/// Draw the whole navicust (background + color bar + grid body + optional
/// ghost) onto `frame`, in native coords (the caller scales the frame). The
/// BN3 style label is NOT drawn here — the image path bakes it after resize,
/// and the canvas overlays it as text.
pub fn paint<R: geometry::Renderer>(
    frame: &mut geometry::Frame<R>,
    m: &GridModel,
    ghost: Option<&Ghost>,
    bg_radius: f32,
    scale: f32,
) {
    let g = geometry(m.cols, m.rows);
    fill_round_rect(frame, scale, 0.0, 0.0, g.total_w, g.total_h, bg_radius, m.background);
    paint_color_bar(frame, m, &g, scale);
    paint_body(frame, m, &g, scale);
    if let Some(gh) = ghost {
        paint_ghost(frame, &g, gh, scale);
    }
}

fn paint_color_bar<R: geometry::Renderer>(frame: &mut geometry::Frame<R>, m: &GridModel, g: &Geometry, scale: f32) {
    let top = PADDING_V as f32 + BORDER_WIDTH / 2.0;
    if m.is_bn3 {
        const TILE: f32 = SQUARE_SIZE / 4.0;
        let bar_inner_w = TILE * 4.0 + BORDER_WIDTH;
        let left = PADDING_H as f32 + (g.body_w - bar_inner_w) + BORDER_WIDTH / 2.0;
        for (i, tile) in m.bar.iter().enumerate() {
            let x = left + i as f32 * TILE;
            fill_rect(
                frame,
                scale,
                x,
                top,
                TILE,
                SQUARE_SIZE / 2.0,
                tile.unwrap_or(BG_FILL_COLOR),
            );
            stroke_rect(
                frame,
                scale,
                x,
                top,
                TILE,
                SQUARE_SIZE / 2.0,
                BORDER_STROKE_COLOR,
                BORDER_WIDTH,
            );
        }
    } else {
        const TILE: f32 = SQUARE_SIZE * 3.0 / 4.0;
        let tile_count = std::cmp::max(4, m.bar.iter().take_while(|v| v.is_some()).count()) as f32;
        let bar_inner_w = TILE * tile_count + BORDER_WIDTH * 2.0;
        let left = PADDING_H as f32 + (g.body_w - bar_inner_w) + BORDER_WIDTH / 2.0;
        let inner_w = TILE - BORDER_WIDTH;
        let inner_h = SQUARE_SIZE / 2.0 - BORDER_WIDTH;
        // First up-to-4: filled inner + outline.
        for i in 0..4 {
            let x = left + i as f32 * TILE;
            let fill = m.bar.get(i).copied().flatten().unwrap_or(BG_FILL_COLOR);
            fill_rect(
                frame,
                scale,
                x + BORDER_WIDTH / 2.0,
                top + BORDER_WIDTH / 2.0,
                inner_w,
                inner_h,
                fill,
            );
            stroke_rect(
                frame,
                scale,
                x,
                top,
                TILE,
                SQUARE_SIZE / 2.0,
                BORDER_STROKE_COLOR,
                BORDER_WIDTH,
            );
        }
        // Remaining "bug" colors: filled inner, no outline, after a gap.
        for (j, c) in m.bar.iter().skip(4).enumerate() {
            let Some(c) = c else {
                continue;
            };
            let x = left + (j as f32 + 4.0) * TILE + BORDER_WIDTH;
            fill_rect(
                frame,
                scale,
                x + BORDER_WIDTH / 2.0,
                top + BORDER_WIDTH / 2.0,
                inner_w,
                inner_h,
                *c,
            );
        }
    }
}

/// Which side of a cell an edge sits on.
#[derive(Clone, Copy)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

/// A neighbouring cell's relationship to a part, as seen by
/// [`for_each_part_edge`].
pub enum Adj {
    /// Part of the same piece — the shared edge is an internal separator.
    Own,
    /// Empty / a different piece / off the board — an outer-boundary edge.
    Outside,
    /// Leave this edge open (e.g. a clipped, off-grid ghost cell).
    Skip,
}

/// One thing to draw for a part's shape, yielded by [`for_each_part_edge`].
pub enum PartMark {
    /// The `side` edge of cell `(col, row)`: `separator` is true for a line
    /// shared with another of the part's own cells, false for an outer edge.
    Edge {
        col: usize,
        row: usize,
        side: Side,
        separator: bool,
    },
    /// The centre cross of non-solid cell `(col, row)` (a vertical + a
    /// horizontal line through the middle).
    Cross { col: usize, row: usize },
}

/// Walk the edges + centre crosses that make up one part's footprint, so
/// every renderer — grid body, ghost, palette thumbnail, baked icon —
/// shares the shape logic and differs only in how it strokes a line.
///
/// `f` is invoked for each [`PartMark`] to draw. Each internal separator is
/// emitted once (as the lower / right cell's top / left edge). `adj(c, r)`
/// classifies the signed neighbour cell `(c, r)` as [`Adj::Own`],
/// [`Adj::Outside`], or [`Adj::Skip`].
pub fn for_each_part_edge(
    cells: &[(usize, usize)],
    is_solid: bool,
    adj: impl Fn(isize, isize) -> Adj,
    mut f: impl FnMut(PartMark),
) {
    for &(col, row) in cells {
        let (c, r) = (col as isize, row as isize);
        // Top / left are drawn by every cell: a separator toward an own
        // neighbour, else the outer boundary.
        match adj(c, r - 1) {
            Adj::Own => f(PartMark::Edge {
                col,
                row,
                side: Side::Top,
                separator: true,
            }),
            Adj::Outside => f(PartMark::Edge {
                col,
                row,
                side: Side::Top,
                separator: false,
            }),
            Adj::Skip => {}
        }
        match adj(c - 1, r) {
            Adj::Own => f(PartMark::Edge {
                col,
                row,
                side: Side::Left,
                separator: true,
            }),
            Adj::Outside => f(PartMark::Edge {
                col,
                row,
                side: Side::Left,
                separator: false,
            }),
            Adj::Skip => {}
        }
        // Bottom / right only on the outer boundary — an own-neighbour
        // separator there is the neighbour's top / left, already emitted.
        if let Adj::Outside = adj(c, r + 1) {
            f(PartMark::Edge {
                col,
                row,
                side: Side::Bottom,
                separator: false,
            });
        }
        if let Adj::Outside = adj(c + 1, r) {
            f(PartMark::Edge {
                col,
                row,
                side: Side::Right,
                separator: false,
            });
        }
        if !is_solid {
            f(PartMark::Cross { col, row });
        }
    }
}

fn paint_body<R: geometry::Renderer>(frame: &mut geometry::Frame<R>, m: &GridModel, g: &Geometry, scale: f32) {
    let bx = g.body_origin_x + BORDER_WIDTH / 2.0;
    let by = g.body_origin_y + BORDER_WIDTH / 2.0;
    let (cols, rows) = (m.cols, m.rows);
    let occ = |col: usize, row: usize| m.occupancy.get(row * cols + col).copied().flatten();
    let cell_xy = |col: usize, row: usize| (bx + col as f32 * SQUARE_SIZE, by + row as f32 * SQUARE_SIZE);
    let is_corner =
        |col: usize, row: usize| m.has_out_of_bounds && (col == 0 || col == cols - 1) && (row == 0 || row == rows - 1);

    // Pass 1: background squares.
    for row in 0..rows {
        for col in 0..cols {
            if is_corner(col, row) {
                continue;
            }
            let (x, y) = cell_xy(col, row);
            fill_rect(frame, scale, x, y, SQUARE_SIZE, SQUARE_SIZE, BG_FILL_COLOR);
            stroke_rect(
                frame,
                scale,
                x,
                y,
                SQUARE_SIZE,
                SQUARE_SIZE,
                BORDER_STROKE_COLOR,
                BORDER_WIDTH,
            );
        }
    }

    // Pass 2: fill each part's squares, then its plus borders + cross via the
    // shared edge walk. Separators and outer edges are both the part's plus
    // colour here; Pass 3 overlays the dark border between distinct parts (so
    // outer edges end up dark, internal separators stay plus — as before).
    // Grouped by part so the walk can tell own cells from neighbours.
    let mut by_slot: std::collections::HashMap<usize, Vec<(usize, usize)>> = std::collections::HashMap::new();
    for row in 0..rows {
        for col in 0..cols {
            if let Some(slot) = occ(col, row) {
                by_slot.entry(slot).or_default().push((col, row));
            }
        }
    }
    for (slot, part_cells) in &by_slot {
        let Some(style) = m.part_styles.get(*slot).and_then(|s| *s) else {
            continue;
        };
        for &(col, row) in part_cells {
            let (x, y) = cell_xy(col, row);
            fill_rect(frame, scale, x, y, SQUARE_SIZE, SQUARE_SIZE, style.solid);
        }
        let own: std::collections::HashSet<(isize, isize)> =
            part_cells.iter().map(|&(c, r)| (c as isize, r as isize)).collect();
        for_each_part_edge(
            part_cells,
            style.is_solid,
            |c, r| if own.contains(&(c, r)) { Adj::Own } else { Adj::Outside },
            |mark| match mark {
                PartMark::Edge { col, row, side, .. } => {
                    let (x, y) = cell_xy(col, row);
                    let (x1, y1, x2, y2) = match side {
                        Side::Top => (x, y, x + SQUARE_SIZE, y),
                        Side::Bottom => (x, y + SQUARE_SIZE, x + SQUARE_SIZE, y + SQUARE_SIZE),
                        Side::Left => (x, y, x, y + SQUARE_SIZE),
                        Side::Right => (x + SQUARE_SIZE, y, x + SQUARE_SIZE, y + SQUARE_SIZE),
                    };
                    stroke_line(frame, scale, x1, y1, x2, y2, style.plus, BORDER_WIDTH);
                }
                PartMark::Cross { col, row } => {
                    let (x, y) = cell_xy(col, row);
                    stroke_line(
                        frame,
                        scale,
                        x + SQUARE_SIZE / 2.0,
                        y,
                        x + SQUARE_SIZE / 2.0,
                        y + SQUARE_SIZE,
                        style.plus,
                        BORDER_WIDTH,
                    );
                    stroke_line(
                        frame,
                        scale,
                        x,
                        y + SQUARE_SIZE / 2.0,
                        x + SQUARE_SIZE,
                        y + SQUARE_SIZE / 2.0,
                        style.plus,
                        BORDER_WIDTH,
                    );
                }
            },
        );
    }

    // Pass 3: borders between distinct parts.
    for row in 0..rows {
        for col in 0..cols {
            let Some(slot) = occ(col, row) else { continue };
            let (x, y) = cell_xy(col, row);
            let edges = [
                ((0i32, -1i32), (x, y, x + SQUARE_SIZE, y)),                      // top
                ((-1, 0), (x, y, x, y + SQUARE_SIZE)),                            // left
                ((0, 1), (x, y + SQUARE_SIZE, x + SQUARE_SIZE, y + SQUARE_SIZE)), // bottom
                ((1, 0), (x + SQUARE_SIZE, y, x + SQUARE_SIZE, y + SQUARE_SIZE)), // right
            ];
            for ((dx, dy), (x1, y1, x2, y2)) in edges {
                let ncol = col as i32 + dx;
                let nrow = row as i32 + dy;
                let different = ncol < 0
                    || nrow < 0
                    || ncol >= cols as i32
                    || nrow >= rows as i32
                    || occ(ncol as usize, nrow as usize) != Some(slot);
                if different {
                    stroke_line(frame, scale, x1, y1, x2, y2, BORDER_STROKE_COLOR, BORDER_WIDTH);
                }
            }
        }
    }

    // Pass 4: command-line markers.
    let cl = by + m.command_line as f32 * SQUARE_SIZE;
    for frac in [0.25_f32, 0.75] {
        let ly = cl + SQUARE_SIZE * frac;
        stroke_line(
            frame,
            scale,
            bx,
            ly,
            bx + cols as f32 * SQUARE_SIZE,
            ly,
            BORDER_STROKE_COLOR,
            BORDER_WIDTH,
        );
    }

    // Pass 5: out-of-bounds shading (the outer band, half-alpha black).
    if m.has_out_of_bounds {
        let band_w = SQUARE_SIZE + BORDER_WIDTH;
        let band_h = (rows as f32 - 2.0) * SQUARE_SIZE + BORDER_WIDTH;
        fill_rect(
            frame,
            scale,
            bx - BORDER_WIDTH / 2.0,
            by + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_w,
            band_h,
            OOB_SHADE,
        );
        fill_rect(
            frame,
            scale,
            bx + (cols as f32 - 1.0) * SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_w,
            band_h,
            OOB_SHADE,
        );
        fill_rect(
            frame,
            scale,
            bx + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by - BORDER_WIDTH / 2.0,
            band_h,
            band_w,
            OOB_SHADE,
        );
        fill_rect(
            frame,
            scale,
            bx + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by + (rows as f32 - 1.0) * SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_h,
            band_w,
            OOB_SHADE,
        );
    }
}

fn paint_ghost<R: geometry::Renderer>(frame: &mut geometry::Frame<R>, g: &Geometry, gh: &Ghost, scale: f32) {
    let bx = g.body_origin_x + BORDER_WIDTH / 2.0;
    let by = g.body_origin_y + BORDER_WIDTH / 2.0;
    let tint = [gh.solid[0], gh.solid[1], gh.solid[2], 0x80];
    let outline = if gh.legal { GHOST_LEGAL } else { GHOST_ILLEGAL };
    // The plus lines are drawn opaque, but at the colour they'd composite to
    // if drawn at the fill's alpha over the cell background — i.e. the same
    // "50% over background" the translucent fill lands on. Stroking opaque
    // plus over the translucent fill would be full-intensity (too bright);
    // stroking translucent plus over it would double-composite (muddy). This
    // pre-blend gives the right colour with neither artefact.
    let a = 0x80 as f32 / 255.0;
    let blend = |c: u8, b: u8| (c as f32 * a + b as f32 * (1.0 - a)).round() as u8;
    let plus = [
        blend(gh.plus[0], BG_FILL_COLOR[0]),
        blend(gh.plus[1], BG_FILL_COLOR[1]),
        blend(gh.plus[2], BG_FILL_COLOR[2]),
        0xff,
    ];
    let cell_xy = |col: usize, row: usize| (bx + col as f32 * SQUARE_SIZE, by + row as f32 * SQUARE_SIZE);
    let cells: std::collections::HashSet<(isize, isize)> =
        gh.cells.iter().map(|&(c, r)| (c as isize, r as isize)).collect();
    let footprint: std::collections::HashSet<(isize, isize)> = gh.footprint.iter().copied().collect();

    // Translucent fill first.
    for &(col, row) in &gh.cells {
        let (x, y) = cell_xy(col, row);
        fill_rect(frame, scale, x, y, SQUARE_SIZE, SQUARE_SIZE, tint);
    }

    // Walk the part's edges + crosses (same shape logic as a placed part).
    // Separators (between the part's own cells) and the centre cross go down
    // now in the pre-blended `plus` (see above). Outer-boundary edges are
    // buffered and stroked last so the legality outline sits on top of the
    // plus lines at shared corners. A neighbour that's in the footprint but
    // off-grid is a clipped edge — Skip, left open so a piece running
    // offscreen isn't boxed in.
    let adj = |c: isize, r: isize| {
        if cells.contains(&(c, r)) {
            Adj::Own
        } else if footprint.contains(&(c, r)) {
            Adj::Skip
        } else {
            Adj::Outside
        }
    };
    let mut boundary: Vec<(f32, f32, f32, f32)> = Vec::new();
    for_each_part_edge(&gh.cells, gh.is_solid, adj, |mark| match mark {
        PartMark::Edge {
            col,
            row,
            side,
            separator,
        } => {
            let (x, y) = cell_xy(col, row);
            let line = match side {
                Side::Top => (x, y, x + SQUARE_SIZE, y),
                Side::Bottom => (x, y + SQUARE_SIZE, x + SQUARE_SIZE, y + SQUARE_SIZE),
                Side::Left => (x, y, x, y + SQUARE_SIZE),
                Side::Right => (x + SQUARE_SIZE, y, x + SQUARE_SIZE, y + SQUARE_SIZE),
            };
            if separator {
                stroke_line(frame, scale, line.0, line.1, line.2, line.3, plus, BORDER_WIDTH);
            } else {
                boundary.push(line);
            }
        }
        PartMark::Cross { col, row } => {
            let (x, y) = cell_xy(col, row);
            stroke_line(
                frame,
                scale,
                x + SQUARE_SIZE / 2.0,
                y,
                x + SQUARE_SIZE / 2.0,
                y + SQUARE_SIZE,
                plus,
                BORDER_WIDTH,
            );
            stroke_line(
                frame,
                scale,
                x,
                y + SQUARE_SIZE / 2.0,
                x + SQUARE_SIZE,
                y + SQUARE_SIZE / 2.0,
                plus,
                BORDER_WIDTH,
            );
        }
    });
    for (x1, y1, x2, y2) in boundary {
        stroke_line(frame, scale, x1, y1, x2, y2, outline, BORDER_WIDTH);
    }
}

fn to_color(c: [u8; 4]) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3] as f32 / 255.0)
}

/// Rasterize `model` to an RGBA image by drawing it through the shared
/// [`paint`] routine onto a standalone [`iced_tiny_skia::Renderer`] — the
/// exact same canvas pipeline the live editor uses, just rendered to a
/// pixmap instead of the window. No text is drawn here (the BN3 style
/// label is baked separately, after), so the default font is irrelevant.
fn rasterize(model: &GridModel) -> image::RgbaImage {
    let g = geometry(model.cols, model.rows);
    let w = g.total_w.round().max(1.0) as u32;
    let h = g.total_h.round().max(1.0) as u32;

    let mut renderer = iced_tiny_skia::Renderer::new(iced::Font::DEFAULT, iced::Pixels(16.0));
    {
        // 1:1 (native coords == pixels), so the draw scale is 1.0.
        let mut frame = geometry::Frame::new(&renderer, Size::new(w as f32, h as f32));
        paint(&mut frame, model, None, 0.0, 1.0);
        let geom = frame.into_geometry();
        <iced_tiny_skia::Renderer as geometry::Renderer>::draw_geometry(&mut renderer, geom);
    }

    let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("navicust pixmap alloc");
    let mut mask = tiny_skia::Mask::new(w, h).expect("navicust mask alloc");
    let viewport = iced::advanced::graphics::Viewport::with_physical_size(Size::new(w, h), 1.0);
    let full = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: w as f32,
        height: h as f32,
    };
    renderer.draw(&mut pixmap.as_mut(), &mut mask, &viewport, &[full], Color::TRANSPARENT);

    // iced_tiny_skia writes its surface as BGRA (`into_color` swaps R/B),
    // so swap them back to get the RGBA `image` expects — the same fix-up
    // iced's own offscreen `screenshot` does.
    let mut rgba = pixmap.data().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    image::RgbaImage::from_raw(w, h, rgba).expect("navicust rgba")
}

/// A small standalone thumbnail of one part's shape — the whole grid-sized
/// bitmap (uncropped, so every part's thumbnail is the same n×n block size
/// and lines up in the palette), with filled cells in the part's solid
/// color and a plus-color outline + separators, on a transparent
/// background. Rendered at an integer block size and shown 1:1, so the 1px
/// lines never warp. Returns `None` for an empty bitmap.
pub fn render_part_thumb(
    bitmap: &tango_dataview::rom::NavicustBitmap,
    color: NavicustPartColor,
    is_solid: bool,
) -> Option<image::RgbaImage> {
    const PX: u32 = 8;
    let (h, w) = bitmap.dim();
    let cells: Vec<(usize, usize)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| bitmap[[y, x]])
        .collect();
    if cells.is_empty() {
        return None;
    }
    let (solid, plus) = part_colors(color);
    let mut img = image::RgbaImage::new(w as u32 * PX, h as u32 * PX);
    // Solid bodies first.
    for &(cx, cy) in &cells {
        for dy in 0..PX {
            for dx in 0..PX {
                img.put_pixel(cx as u32 * PX + dx, cy as u32 * PX + dy, image::Rgba(solid));
            }
        }
    }
    // Plus edges + cross via the shared shape walk — uniform 1px lines:
    // top/left of every cell (the separators between blocks) and bottom/right
    // only on the outer boundary, with no doubled-up internal borders. Same
    // model the live `PartThumb` canvas draws.
    let own: std::collections::HashSet<(isize, isize)> = cells.iter().map(|&(c, r)| (c as isize, r as isize)).collect();
    for_each_part_edge(
        &cells,
        is_solid,
        |c, r| if own.contains(&(c, r)) { Adj::Own } else { Adj::Outside },
        |mark| match mark {
            PartMark::Edge { col, row, side, .. } => {
                let (ox, oy) = (col as u32 * PX, row as u32 * PX);
                match side {
                    Side::Top => (0..PX).for_each(|dx| img.put_pixel(ox + dx, oy, image::Rgba(plus))),
                    Side::Bottom => (0..PX).for_each(|dx| img.put_pixel(ox + dx, oy + PX - 1, image::Rgba(plus))),
                    Side::Left => (0..PX).for_each(|dy| img.put_pixel(ox, oy + dy, image::Rgba(plus))),
                    Side::Right => (0..PX).for_each(|dy| img.put_pixel(ox + PX - 1, oy + dy, image::Rgba(plus))),
                }
            }
            PartMark::Cross { col, row } => {
                let (ox, oy) = (col as u32 * PX, row as u32 * PX);
                (0..PX).for_each(|dy| img.put_pixel(ox + PX / 2, oy + dy, image::Rgba(plus)));
                (0..PX).for_each(|dx| img.put_pixel(ox + dx, oy + PX / 2, image::Rgba(plus)));
            }
        },
    );
    Some(img)
}

/// `bitmap` rotated clockwise `rot` quarter turns. Grids are square
/// (5×5 / 7×7), so a quarter turn preserves the dimensions. Matches the
/// `(by, bx) -> (bx, n-1-by)` mapping used by
/// [`crate::navicust_editor::rotated_offsets`] and `navicust::rotate90`.
pub fn rotate_bitmap(bitmap: &tango_dataview::rom::NavicustBitmap, rot: u8) -> tango_dataview::rom::NavicustBitmap {
    // Square grids only, so a quarter turn is an in-shape permutation —
    // clone the source each step and reassign cells (avoids naming the
    // ndarray crate, which isn't a direct dependency of this crate).
    let mut out = bitmap.clone();
    for _ in 0..(rot % 4) {
        let src = out.clone();
        let (h, w) = src.dim();
        debug_assert_eq!(h, w);
        let n = h;
        for y in 0..n {
            for x in 0..n {
                out[[x, n - 1 - y]] = src[[y, x]];
            }
        }
    }
    out
}
