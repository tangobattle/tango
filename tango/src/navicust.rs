//! NaviCust grid rendering. Ported from `tango/src/gui/save_view/navi_view/navicust_view.rs`.
//! Outputs an RGBA image we can hand to iced's image widget. For BN3 the
//! color bar carries the style name on its left edge — rasterized through
//! cosmic-text (the same shaper iced uses, via iced's shared font system)
//! so script-aware font fallback picks up the bundled JP / SC / TC Noto
//! faces for non-Latin style names instead of tofu-ing out.

use iced::advanced::graphics::text::cosmic_text;
use iced::advanced::graphics::text::font_system as iced_font_system;
use std::sync::Mutex;
use std::sync::LazyLock;
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

pub const BORDER_WIDTH: f32 = 6.0;
pub const SQUARE_SIZE: f32 = 60.0;

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

pub const PADDING_H: u32 = 20;
pub const PADDING_V: u32 = 20;

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
    let mut pixmap = tiny_skia::Pixmap::new(g.total_w.round() as u32, g.total_h.round() as u32).unwrap();
    paint(&mut SkiaPainter { pixmap: &mut pixmap }, &model, None);
    let native = image::RgbaImage::from_raw(pixmap.width(), pixmap.height(), pixmap.take()).unwrap();

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

/// Resolved color-bar contents. `Bn3` carries the style's extra color
/// (the fixed White/Pink/Yellow tiles are implicit); `Bn456` carries the
/// distinct part colors in order (first ≤4 outlined, the rest "bug").
#[derive(Clone)]
pub enum ColorBar {
    Bn3 { extra: Option<[u8; 4]> },
    Bn456 { colors: Vec<[u8; 4]> },
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
    pub bar: ColorBar,
}

/// A held part previewed under the cursor (editor only).
pub struct Ghost {
    /// Absolute grid cells `(col, row)` the part would cover.
    pub cells: Vec<(usize, usize)>,
    pub solid: [u8; 4],
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

/// A backend the shared navicust drawing routine targets. One impl wraps
/// a tiny-skia pixmap (the clipboard/image path); another wraps an iced
/// canvas frame (live display). All coords are native pixels.
pub trait GridPainter {
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]);
    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 4], width: f32);
    fn stroke_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: [u8; 4], width: f32);
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
        let Some(info) = assets.navicust_part(part.id) else { continue };
        let Some(c) = info.color() else { continue };
        let (solid, plus) = part_colors(c);
        part_styles[i] = Some(PartStyle {
            solid,
            plus,
            is_solid: info.is_solid(),
        });
    }

    let bar = if let Some(sid) = view.style() {
        ColorBar::Bn3 {
            extra: assets.style(sid).and_then(|s| s.extra_ncp_color()).map(|c| part_colors(c).1),
        }
    } else {
        // Render the color bar straight from the save (its stored bytes),
        // not by recomputing it here — so it matches the in-game bar
        // exactly. The editor keeps the stored bar current as parts change.
        let colors = view
            .navicust_color_bar()
            .into_iter()
            .flatten()
            .map(|c| part_colors(c).1)
            .collect();
        ColorBar::Bn456 { colors }
    };

    GridModel {
        cols,
        rows,
        command_line: layout.command_line,
        has_out_of_bounds: layout.has_out_of_bounds,
        background: layout.background.0,
        occupancy,
        part_styles,
        bar,
    }
}

/// Draw the whole navicust (background + color bar + grid body + optional
/// ghost) through `p`. The BN3 style label is NOT drawn here — the image
/// path bakes it after resize, and the canvas overlays it as text.
pub fn paint<P: GridPainter>(p: &mut P, m: &GridModel, ghost: Option<&Ghost>) {
    let g = geometry(m.cols, m.rows);
    p.fill_rect(0.0, 0.0, g.total_w, g.total_h, m.background);
    paint_color_bar(p, m, &g);
    paint_body(p, m, &g);
    if let Some(gh) = ghost {
        paint_ghost(p, &g, gh);
    }
}

fn paint_color_bar<P: GridPainter>(p: &mut P, m: &GridModel, g: &Geometry) {
    let top = PADDING_V as f32 + BORDER_WIDTH / 2.0;
    match &m.bar {
        ColorBar::Bn3 { extra } => {
            const TILE: f32 = SQUARE_SIZE / 4.0;
            let bar_inner_w = TILE * 4.0 + BORDER_WIDTH;
            let left = PADDING_H as f32 + (g.body_w - bar_inner_w) + BORDER_WIDTH / 2.0;
            let tiles = [
                Some(part_colors(NavicustPartColor::White).1),
                Some(part_colors(NavicustPartColor::Pink).1),
                Some(part_colors(NavicustPartColor::Yellow).1),
                *extra,
            ];
            for (i, tile) in tiles.iter().enumerate() {
                let x = left + i as f32 * TILE;
                p.fill_rect(x, top, TILE, SQUARE_SIZE / 2.0, tile.unwrap_or(BG_FILL_COLOR));
                p.stroke_rect(x, top, TILE, SQUARE_SIZE / 2.0, BORDER_STROKE_COLOR, BORDER_WIDTH);
            }
        }
        ColorBar::Bn456 { colors } => {
            const TILE: f32 = SQUARE_SIZE * 3.0 / 4.0;
            let tile_count = std::cmp::max(4, colors.len()) as f32;
            let bar_inner_w = TILE * tile_count + BORDER_WIDTH * 2.0;
            let left = PADDING_H as f32 + (g.body_w - bar_inner_w) + BORDER_WIDTH / 2.0;
            let inner_w = TILE - BORDER_WIDTH;
            let inner_h = SQUARE_SIZE / 2.0 - BORDER_WIDTH;
            // First up-to-4: filled inner + outline.
            for i in 0..4 {
                let x = left + i as f32 * TILE;
                let fill = colors.get(i).copied().unwrap_or(BG_FILL_COLOR);
                p.fill_rect(x + BORDER_WIDTH / 2.0, top + BORDER_WIDTH / 2.0, inner_w, inner_h, fill);
                p.stroke_rect(x, top, TILE, SQUARE_SIZE / 2.0, BORDER_STROKE_COLOR, BORDER_WIDTH);
            }
            // Remaining "bug" colors: filled inner, no outline, after a gap.
            for (j, c) in colors.iter().skip(4).enumerate() {
                let x = left + (j as f32 + 4.0) * TILE + BORDER_WIDTH;
                p.fill_rect(x + BORDER_WIDTH / 2.0, top + BORDER_WIDTH / 2.0, inner_w, inner_h, *c);
            }
        }
    }
}

fn paint_body<P: GridPainter>(p: &mut P, m: &GridModel, g: &Geometry) {
    let bx = g.body_origin_x + BORDER_WIDTH / 2.0;
    let by = g.body_origin_y + BORDER_WIDTH / 2.0;
    let (cols, rows) = (m.cols, m.rows);
    let occ = |col: usize, row: usize| m.occupancy.get(row * cols + col).copied().flatten();
    let cell_xy = |col: usize, row: usize| (bx + col as f32 * SQUARE_SIZE, by + row as f32 * SQUARE_SIZE);
    let is_corner = |col: usize, row: usize| {
        m.has_out_of_bounds && (col == 0 || col == cols - 1) && (row == 0 || row == rows - 1)
    };

    // Pass 1: background squares.
    for row in 0..rows {
        for col in 0..cols {
            if is_corner(col, row) {
                continue;
            }
            let (x, y) = cell_xy(col, row);
            p.fill_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, BG_FILL_COLOR);
            p.stroke_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, BORDER_STROKE_COLOR, BORDER_WIDTH);
        }
    }

    // Pass 2: filled part squares.
    for row in 0..rows {
        for col in 0..cols {
            let Some(slot) = occ(col, row) else { continue };
            let Some(style) = m.part_styles.get(slot).and_then(|s| *s) else {
                continue;
            };
            let (x, y) = cell_xy(col, row);
            p.fill_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, style.solid);
            p.stroke_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, style.plus, BORDER_WIDTH);
            if !style.is_solid {
                p.stroke_line(x + SQUARE_SIZE / 2.0, y, x + SQUARE_SIZE / 2.0, y + SQUARE_SIZE, style.plus, BORDER_WIDTH);
                p.stroke_line(x, y + SQUARE_SIZE / 2.0, x + SQUARE_SIZE, y + SQUARE_SIZE / 2.0, style.plus, BORDER_WIDTH);
            }
        }
    }

    // Pass 3: borders between distinct parts.
    for row in 0..rows {
        for col in 0..cols {
            let Some(slot) = occ(col, row) else { continue };
            let (x, y) = cell_xy(col, row);
            let edges = [
                ((0i32, -1i32), (x, y, x + SQUARE_SIZE, y)),                          // top
                ((-1, 0), (x, y, x, y + SQUARE_SIZE)),                                // left
                ((0, 1), (x, y + SQUARE_SIZE, x + SQUARE_SIZE, y + SQUARE_SIZE)),      // bottom
                ((1, 0), (x + SQUARE_SIZE, y, x + SQUARE_SIZE, y + SQUARE_SIZE)),      // right
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
                    p.stroke_line(x1, y1, x2, y2, BORDER_STROKE_COLOR, BORDER_WIDTH);
                }
            }
        }
    }

    // Pass 4: command-line markers.
    let cl = by + m.command_line as f32 * SQUARE_SIZE;
    for frac in [0.25_f32, 0.75] {
        let ly = cl + SQUARE_SIZE * frac;
        p.stroke_line(bx, ly, bx + cols as f32 * SQUARE_SIZE, ly, BORDER_STROKE_COLOR, BORDER_WIDTH);
    }

    // Pass 5: out-of-bounds shading (the outer band, half-alpha black).
    if m.has_out_of_bounds {
        let band_w = SQUARE_SIZE + BORDER_WIDTH;
        let band_h = (rows as f32 - 2.0) * SQUARE_SIZE + BORDER_WIDTH;
        p.fill_rect(bx - BORDER_WIDTH / 2.0, by + SQUARE_SIZE - BORDER_WIDTH / 2.0, band_w, band_h, OOB_SHADE);
        p.fill_rect(
            bx + (cols as f32 - 1.0) * SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_w,
            band_h,
            OOB_SHADE,
        );
        p.fill_rect(bx + SQUARE_SIZE - BORDER_WIDTH / 2.0, by - BORDER_WIDTH / 2.0, band_h, band_w, OOB_SHADE);
        p.fill_rect(
            bx + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by + (rows as f32 - 1.0) * SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_h,
            band_w,
            OOB_SHADE,
        );
    }
}

fn paint_ghost<P: GridPainter>(p: &mut P, g: &Geometry, gh: &Ghost) {
    let bx = g.body_origin_x + BORDER_WIDTH / 2.0;
    let by = g.body_origin_y + BORDER_WIDTH / 2.0;
    let tint = [gh.solid[0], gh.solid[1], gh.solid[2], 0x80];
    let outline = if gh.legal { GHOST_LEGAL } else { GHOST_ILLEGAL };
    for &(col, row) in &gh.cells {
        let (x, y) = (bx + col as f32 * SQUARE_SIZE, by + row as f32 * SQUARE_SIZE);
        p.fill_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, tint);
        p.stroke_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, outline, BORDER_WIDTH);
        if !gh.is_solid {
            p.stroke_line(x + SQUARE_SIZE / 2.0, y, x + SQUARE_SIZE / 2.0, y + SQUARE_SIZE, outline, BORDER_WIDTH);
            p.stroke_line(x, y + SQUARE_SIZE / 2.0, x + SQUARE_SIZE, y + SQUARE_SIZE / 2.0, outline, BORDER_WIDTH);
        }
    }
}

/// tiny-skia backend (clipboard / export path).
struct SkiaPainter<'a> {
    pixmap: &'a mut tiny_skia::Pixmap,
}

fn grid_stroke(width: f32) -> tiny_skia::Stroke {
    tiny_skia::Stroke {
        line_cap: tiny_skia::LineCap::Square,
        width,
        ..Default::default()
    }
}

impl GridPainter for SkiaPainter<'_> {
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) {
        let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) else { return };
        let path = tiny_skia::PathBuilder::from_rect(rect);
        self.pixmap.fill_path(
            &path,
            &solid_paint(color),
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 4], width: f32) {
        let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) else { return };
        let path = tiny_skia::PathBuilder::from_rect(rect);
        self.pixmap
            .stroke_path(&path, &solid_paint(color), &grid_stroke(width), tiny_skia::Transform::identity(), None);
    }

    fn stroke_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: [u8; 4], width: f32) {
        let path = line_path(x1, y1, x2, y2);
        self.pixmap
            .stroke_path(&path, &solid_paint(color), &grid_stroke(width), tiny_skia::Transform::identity(), None);
    }
}

fn solid_paint(rgba: [u8; 4]) -> tiny_skia::Paint<'static> {
    let mut p = tiny_skia::Paint::default();
    p.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    p
}

fn line_path(x1: f32, y1: f32, x2: f32, y2: f32) -> tiny_skia::Path {
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);
    pb.finish().unwrap()
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
    if !(0..h).any(|y| (0..w).any(|x| bitmap[[y, x]])) {
        return None;
    }
    let (solid, plus) = part_colors(color);
    let mut img = image::RgbaImage::new(w as u32 * PX, h as u32 * PX);
    for y in 0..h {
        for x in 0..w {
            if !bitmap[[y, x]] {
                continue;
            }
            let ox = x as u32 * PX;
            let oy = y as u32 * PX;
            // Draw each block's top + left edge unconditionally — those
            // become the 1px lines *between* blocks — and the bottom/right
            // edges only on the part's outer boundary. This keeps every
            // line (separators, outline, and cross) a uniform 1px with no
            // doubled-up internal borders.
            let down = y + 1 < h && bitmap[[y + 1, x]];
            let right = x + 1 < w && bitmap[[y, x + 1]];
            for dy in 0..PX {
                for dx in 0..PX {
                    let edge = dy == 0 || dx == 0 || (dy == PX - 1 && !down) || (dx == PX - 1 && !right);
                    // Non-solid (plus) parts get the center cross so they
                    // read distinctly; 1px, matching the borders.
                    let cross = !is_solid && (dx == PX / 2 || dy == PX / 2);
                    let c = if edge || cross { plus } else { solid };
                    img.put_pixel(ox + dx, oy + dy, image::Rgba(c));
                }
            }
        }
    }
    Some(img)
}
