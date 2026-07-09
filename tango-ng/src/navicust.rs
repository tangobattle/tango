//! NaviCust grid rendering — a port of `tango/src/save_view/navicust/grid.rs`
//! with the iced-canvas/tiny-skia backend replaced by direct pixel drawing
//! on an `image::RgbaImage`. Everything the original draws is axis-aligned
//! rects and lines, so the CPU backend is three tiny helpers; the shape
//! logic (`for_each_part_edge`, `build_model`, geometry) carries over
//! verbatim. The BN3 style label is NOT baked here — the Slint layer
//! overlays it as text (see the label fields on `loaded::NavicustRender`),
//! which gets script-aware font fallback for free.

use tango_dataview::{
    navicust::MaterializedNavicust,
    rom::{Assets, NavicustLayout, NavicustPartColor},
    save::NavicustView,
};

// Native render units, ratios matching the original (BORDER = SQUARE/10).
pub const BORDER_WIDTH: f32 = 12.0;
pub const SQUARE_SIZE: f32 = 120.0;

const BG_FILL_COLOR: [u8; 4] = [0x20, 0x20, 0x20, 0xff];
const BORDER_STROKE_COLOR: [u8; 4] = [0x00, 0x00, 0x00, 0xff];
const OOB_SHADE: [u8; 4] = [0x00, 0x00, 0x00, 0x80];

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

/// Resolved style for one installed part (so drawing never reaches back
/// into the ROM assets).
#[derive(Clone, Copy)]
pub struct PartStyle {
    pub solid: [u8; 4],
    pub plus: [u8; 4],
    pub is_solid: bool,
}

/// Everything the paint routine needs, pre-resolved into owned data.
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

/// Reference grid width, in columns, the on-screen cell size is pinned to:
/// 7 is the widest navicust (BN6), so narrower grids render proportionally
/// smaller instead of each being stretched to one total width.
pub const REFERENCE_COLS: usize = 7;

/// Resolve a navicust view + ROM assets into a [`GridModel`].
pub fn build_model(
    materialized: &MaterializedNavicust,
    layout: &NavicustLayout,
    view: &dyn NavicustView,
    assets: &dyn Assets,
) -> GridModel {
    let (rows, cols) = materialized.dim();
    let occupancy: Vec<Option<usize>> = materialized.iter().copied().collect();

    let mut part_styles: Vec<Option<PartStyle>> = vec![None; view.count()];
    for (i, style) in part_styles.iter_mut().enumerate() {
        let Some(part) = view.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let Some(c) = info.color() else { continue };
        let (solid, plus) = part_colors(c);
        *style = Some(PartStyle {
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

// ----- pixel backend -----
// The original strokes through iced's geometry Frame; every stroke it makes
// is axis-aligned with LineCap::Square, so the whole backend reduces to a
// blended rect fill and two line-as-rect helpers.

/// src-over blend `color` onto the rect `[x, y, w, h]` (native coords,
/// rounded to pixels). Opaque colors overwrite; the OOB shade blends.
fn fill_rect(img: &mut image::RgbaImage, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) {
    let x0 = x.round().max(0.0) as i64;
    let y0 = y.round().max(0.0) as i64;
    let x1 = ((x + w).round() as i64).min(img.width() as i64);
    let y1 = ((y + h).round() as i64).min(img.height() as i64);
    let a = color[3] as u32;
    for py in y0..y1 {
        for px in x0..x1 {
            let p = img.get_pixel_mut(px as u32, py as u32);
            if a == 0xff {
                p.0 = color;
            } else {
                for c in 0..3 {
                    p.0[c] = ((color[c] as u32 * a + p.0[c] as u32 * (255 - a)) / 255) as u8;
                }
            }
        }
    }
}

/// An axis-aligned stroked line of `width`, centered on the segment with
/// square caps (each end extends by `width / 2`) — the shape iced's
/// `LineCap::Square` stroke produces.
fn stroke_line(img: &mut image::RgbaImage, x1: f32, y1: f32, x2: f32, y2: f32, color: [u8; 4], width: f32) {
    let hw = width / 2.0;
    if (y1 - y2).abs() < f32::EPSILON {
        let (xa, xb) = if x1 <= x2 { (x1, x2) } else { (x2, x1) };
        fill_rect(img, xa - hw, y1 - hw, (xb - xa) + width, width, color);
    } else {
        let (ya, yb) = if y1 <= y2 { (y1, y2) } else { (y2, y1) };
        fill_rect(img, x1 - hw, ya - hw, width, (yb - ya) + width, color);
    }
}

/// Outline a rect with a stroke of `width` centered on its edges.
fn stroke_rect(img: &mut image::RgbaImage, x: f32, y: f32, w: f32, h: f32, color: [u8; 4], width: f32) {
    stroke_line(img, x, y, x + w, y, color, width);
    stroke_line(img, x, y + h, x + w, y + h, color, width);
    stroke_line(img, x, y, x, y + h, color, width);
    stroke_line(img, x + w, y, x + w, y + h, color, width);
}

/// Rasterize the whole navicust (background + color bar + grid body) at
/// native resolution, then nearest-downscale to `target_w` if smaller —
/// the same two-step the original uses so grid borders stay sharp.
pub fn render(model: &GridModel, target_w: Option<u32>) -> image::RgbaImage {
    let g = geometry(model.cols, model.rows);
    let w = g.total_w.round().max(1.0) as u32;
    let h = g.total_h.round().max(1.0) as u32;
    let mut img = image::RgbaImage::new(w, h);

    fill_rect(&mut img, 0.0, 0.0, g.total_w, g.total_h, model.background);
    paint_color_bar(&mut img, model, &g);
    paint_body(&mut img, model, &g);

    match target_w {
        Some(tw) if tw < w => {
            let th = (h as f32 * tw as f32 / w as f32).round() as u32;
            image::imageops::resize(&img, tw, th, image::imageops::FilterType::Nearest)
        }
        _ => img,
    }
}

fn paint_color_bar(img: &mut image::RgbaImage, m: &GridModel, g: &Geometry) {
    let top = PADDING_V as f32 + BORDER_WIDTH / 2.0;
    if m.is_bn3 {
        const TILE: f32 = SQUARE_SIZE / 4.0;
        let bar_inner_w = TILE * 4.0 + BORDER_WIDTH;
        let left = PADDING_H as f32 + (g.body_w - bar_inner_w) + BORDER_WIDTH / 2.0;
        for (i, tile) in m.bar.iter().enumerate() {
            let x = left + i as f32 * TILE;
            fill_rect(img, x, top, TILE, SQUARE_SIZE / 2.0, tile.unwrap_or(BG_FILL_COLOR));
            stroke_rect(img, x, top, TILE, SQUARE_SIZE / 2.0, BORDER_STROKE_COLOR, BORDER_WIDTH);
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
                img,
                x + BORDER_WIDTH / 2.0,
                top + BORDER_WIDTH / 2.0,
                inner_w,
                inner_h,
                fill,
            );
            stroke_rect(img, x, top, TILE, SQUARE_SIZE / 2.0, BORDER_STROKE_COLOR, BORDER_WIDTH);
        }
        // Remaining "bug" colors: filled inner, no outline, after a gap.
        for (j, c) in m.bar.iter().skip(4).enumerate() {
            let Some(c) = c else {
                continue;
            };
            let x = left + (j as f32 + 4.0) * TILE + BORDER_WIDTH;
            fill_rect(
                img,
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
    /// Leave this edge open (unused here; kept for the editor port).
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
    /// The centre cross of non-solid cell `(col, row)`.
    Cross { col: usize, row: usize },
}

/// Walk the edges + centre crosses that make up one part's footprint, so
/// every renderer — grid body, palette thumbnail, baked icon — shares the
/// shape logic and differs only in how it strokes a line.
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

fn paint_body(img: &mut image::RgbaImage, m: &GridModel, g: &Geometry) {
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
            fill_rect(img, x, y, SQUARE_SIZE, SQUARE_SIZE, BG_FILL_COLOR);
            stroke_rect(img, x, y, SQUARE_SIZE, SQUARE_SIZE, BORDER_STROKE_COLOR, BORDER_WIDTH);
        }
    }

    // Pass 2: fill each part's squares, then its plus borders + cross via
    // the shared edge walk. Pass 3 overlays the dark border between
    // distinct parts (so outer edges end up dark, separators stay plus).
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
            fill_rect(img, x, y, SQUARE_SIZE, SQUARE_SIZE, style.solid);
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
                    stroke_line(img, x1, y1, x2, y2, style.plus, BORDER_WIDTH);
                }
                PartMark::Cross { col, row } => {
                    let (x, y) = cell_xy(col, row);
                    stroke_line(
                        img,
                        x + SQUARE_SIZE / 2.0,
                        y,
                        x + SQUARE_SIZE / 2.0,
                        y + SQUARE_SIZE,
                        style.plus,
                        BORDER_WIDTH,
                    );
                    stroke_line(
                        img,
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
                    stroke_line(img, x1, y1, x2, y2, BORDER_STROKE_COLOR, BORDER_WIDTH);
                }
            }
        }
    }

    // Pass 4: command-line markers.
    let cl = by + m.command_line as f32 * SQUARE_SIZE;
    for frac in [0.25_f32, 0.75] {
        let ly = cl + SQUARE_SIZE * frac;
        stroke_line(
            img,
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
            img,
            bx - BORDER_WIDTH / 2.0,
            by + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_w,
            band_h,
            OOB_SHADE,
        );
        fill_rect(
            img,
            bx + (cols as f32 - 1.0) * SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_w,
            band_h,
            OOB_SHADE,
        );
        fill_rect(
            img,
            bx + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by - BORDER_WIDTH / 2.0,
            band_h,
            band_w,
            OOB_SHADE,
        );
        fill_rect(
            img,
            bx + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by + (rows as f32 - 1.0) * SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_h,
            band_w,
            OOB_SHADE,
        );
    }
}

/// A small standalone thumbnail of one part's shape: filled cells in the
/// part's solid color with a plus-color outline + separators, on a
/// transparent background, sized straight to the shape's bounding box —
/// for the read-only parts list where it sits inline beside the name.
/// Returns `None` for an empty bitmap.
pub fn render_part_thumb(
    bitmap: &tango_dataview::rom::NavicustBitmap,
    color: NavicustPartColor,
    is_solid: bool,
) -> Option<image::RgbaImage> {
    const PX: u32 = 8;
    let (h, w) = bitmap.dim();
    let mut cells: Vec<(usize, usize)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| bitmap[[y, x]])
        .collect();
    if cells.is_empty() {
        return None;
    }
    // Shift the shape to the origin and size the image to its bounding box.
    let min_x = cells.iter().map(|&(x, _)| x).min().unwrap();
    let min_y = cells.iter().map(|&(_, y)| y).min().unwrap();
    let max_x = cells.iter().map(|&(x, _)| x).max().unwrap();
    let max_y = cells.iter().map(|&(_, y)| y).max().unwrap();
    for c in &mut cells {
        c.0 -= min_x;
        c.1 -= min_y;
    }
    let (grid_w, grid_h) = ((max_x - min_x + 1) as u32, (max_y - min_y + 1) as u32);
    let (solid, plus) = part_colors(color);
    let mut img = image::RgbaImage::new(grid_w * PX, grid_h * PX);
    // Solid bodies first.
    for &(cx, cy) in &cells {
        for dy in 0..PX {
            for dx in 0..PX {
                img.put_pixel(cx as u32 * PX + dx, cy as u32 * PX + dy, image::Rgba(solid));
            }
        }
    }
    // Plus edges + cross via the shared shape walk — uniform 1px lines.
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
/// (5×5 / 7×7), so a quarter turn is an in-shape permutation.
pub fn rotate_bitmap(bitmap: &tango_dataview::rom::NavicustBitmap, rot: u8) -> tango_dataview::rom::NavicustBitmap {
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
