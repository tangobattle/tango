//! NaviCust grid rendering, ported from the desktop's
//! `save_view/navicust/grid.rs`. The desktop paints through an iced
//! canvas / tiny-skia; every mark is an axis-aligned rect or line, so the
//! web build emits the same passes as SVG nodes instead — crisp at any
//! scale, no image baking, and the editor's ghost is just more nodes.
//! Geometry, colors, and draw order match the desktop exactly.

use dioxus::prelude::*;
use tango_dataview::{
    navicust::MaterializedNavicust,
    rom::{Assets, NavicustLayout, NavicustPartColor},
    save::NavicustView,
};

// Native render units (the SVG viewBox coordinate space). Ratios match
// the desktop (BORDER = SQUARE_SIZE/10).
pub const BORDER_WIDTH: f32 = 12.0;
pub const SQUARE_SIZE: f32 = 120.0;
pub const PADDING_H: f32 = 40.0;
pub const PADDING_V: f32 = 40.0;

const BG_FILL_COLOR: [u8; 4] = [0x20, 0x20, 0x20, 0xff];
const BORDER_STROKE_COLOR: [u8; 4] = [0x00, 0x00, 0x00, 0xff];
const OOB_SHADE: [u8; 4] = [0x00, 0x00, 0x00, 0x80];
const GHOST_LEGAL: [u8; 4] = [0x33, 0xd9, 0x4d, 0xff];
const GHOST_ILLEGAL: [u8; 4] = [0xe6, 0x2e, 0x2e, 0xff];

/// Solid color (filled square) + plus/stroke color, matching the desktop.
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

fn css(c: [u8; 4]) -> String {
    if c[3] == 0xff {
        format!("rgb({},{},{})", c[0], c[1], c[2])
    } else {
        format!("rgba({},{},{},{:.3})", c[0], c[1], c[2], c[3] as f32 / 255.0)
    }
}

/// Resolved style for one installed part.
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

/// A held part previewed under the cursor (editor only).
pub struct Ghost {
    /// In-bounds grid cells `(col, row)` the part covers — these get filled.
    pub cells: Vec<(usize, usize)>,
    /// The part's full footprint as signed grid coords (extends off-grid
    /// when the part clips offscreen). An edge is outlined only when its
    /// neighbor isn't in this set, so clipped edges stay open.
    pub footprint: Vec<(isize, isize)>,
    pub solid: [u8; 4],
    pub plus: [u8; 4],
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
        total_w: body_w + PADDING_H * 2.0,
        total_h: body_h + PADDING_V * 3.0 + bar_h,
        bar_h,
        body_w,
        body_origin_x: PADDING_H,
        body_origin_y: PADDING_V + bar_h + PADDING_V,
    }
}

/// Reference grid width, in columns, the on-screen cell size is pinned to.
/// 7 is the widest navicust (BN6); pinning the display scale here makes
/// every grid draw its cells at the 7×7 cell size, so the image grows or
/// shrinks with the grid instead of each being squeezed to one width.
pub const REFERENCE_COLS: usize = 7;

/// The constant display scale applied to every navicust: the scale a
/// `REFERENCE_COLS`-wide grid needs to fit `display_w`.
pub fn display_scale(display_w: f32) -> f32 {
    display_w / geometry(REFERENCE_COLS, REFERENCE_COLS).total_w
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

/// Which side of a cell an edge sits on.
#[derive(Clone, Copy)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

/// A neighbouring cell's relationship to a part.
pub enum Adj {
    /// Part of the same piece — the shared edge is an internal separator.
    Own,
    /// Empty / a different piece / off the board — an outer-boundary edge.
    Outside,
    /// Leave this edge open (e.g. a clipped, off-grid ghost cell).
    Skip,
}

/// One thing to draw for a part's shape.
pub enum PartMark {
    /// The `side` edge of cell `(col, row)`: `separator` is true for a line
    /// shared with another of the part's own cells.
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
/// every renderer — grid body, ghost, palette thumbnail — shares the shape
/// logic. Each internal separator is emitted once (as the lower / right
/// cell's top / left edge).
pub fn for_each_part_edge(
    cells: &[(usize, usize)],
    is_solid: bool,
    adj: impl Fn(isize, isize) -> Adj,
    mut f: impl FnMut(PartMark),
) {
    for &(col, row) in cells {
        let (c, r) = (col as isize, row as isize);
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

/// SVG marks accumulated in draw order. Thin wrapper so the paint passes
/// below read like the desktop's frame helpers.
struct Marks(Vec<Element>);

impl Marks {
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) {
        let fill = css(color);
        self.0.push(rsx! {
            rect { x: "{x}", y: "{y}", width: "{w}", height: "{h}", fill: "{fill}" }
        });
    }

    /// Stroke a rect outline, centered on the path like iced's stroke.
    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [u8; 4], width: f32) {
        let stroke = css(color);
        self.0.push(rsx! {
            rect {
                x: "{x}",
                y: "{y}",
                width: "{w}",
                height: "{h}",
                fill: "none",
                stroke: "{stroke}",
                stroke_width: "{width}",
            }
        });
    }

    /// Stroke a line with square caps (extends `width/2` past both ends),
    /// matching the desktop's `LineCap::Square`.
    fn stroke_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: [u8; 4], width: f32) {
        let stroke = css(color);
        self.0.push(rsx! {
            line {
                x1: "{x1}",
                y1: "{y1}",
                x2: "{x2}",
                y2: "{y2}",
                stroke: "{stroke}",
                stroke_width: "{width}",
                stroke_linecap: "square",
            }
        });
    }

    fn fill_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: [u8; 4]) {
        let fill = css(color);
        self.0.push(rsx! {
            rect {
                x: "{x}",
                y: "{y}",
                width: "{w}",
                height: "{h}",
                rx: "{radius}",
                fill: "{fill}",
            }
        });
    }
}

/// Draw the whole navicust (background + color bar + grid body + optional
/// ghost + optional hover outline) as an `<svg>` sized `display_w(-ish)`
/// on screen, viewBoxed in native coords. `bg_radius` is in native units.
pub fn grid_svg(m: &GridModel, ghost: Option<&Ghost>, hover_slot: Option<usize>) -> Element {
    let g = geometry(m.cols, m.rows);
    let scale = display_scale(super::DISPLAY_W);
    let (dw, dh) = (g.total_w * scale, g.total_h * scale);
    let mut marks = Marks(Vec::new());
    // Match the desktop's 4 display-px corner rounding.
    marks.fill_round_rect(0.0, 0.0, g.total_w, g.total_h, 4.0 / scale, m.background);
    paint_color_bar(&mut marks, m, &g);
    paint_body(&mut marks, m, &g);
    if let Some(gh) = ghost {
        paint_ghost(&mut marks, &g, gh);
    }
    if let Some(slot) = hover_slot {
        paint_part_outline(&mut marks, m, &g, slot);
    }
    let nodes = marks.0;
    rsx! {
        svg {
            view_box: "0 0 {g.total_w} {g.total_h}",
            width: "{dw}",
            height: "{dh}",
            {nodes.into_iter()}
        }
    }
}

fn paint_color_bar(marks: &mut Marks, m: &GridModel, g: &Geometry) {
    let top = PADDING_V + BORDER_WIDTH / 2.0;
    if m.is_bn3 {
        const TILE: f32 = SQUARE_SIZE / 4.0;
        let bar_inner_w = TILE * 4.0 + BORDER_WIDTH;
        let left = PADDING_H + (g.body_w - bar_inner_w) + BORDER_WIDTH / 2.0;
        for (i, tile) in m.bar.iter().enumerate() {
            let x = left + i as f32 * TILE;
            marks.fill_rect(x, top, TILE, SQUARE_SIZE / 2.0, tile.unwrap_or(BG_FILL_COLOR));
            marks.stroke_rect(x, top, TILE, SQUARE_SIZE / 2.0, BORDER_STROKE_COLOR, BORDER_WIDTH);
        }
    } else {
        const TILE: f32 = SQUARE_SIZE * 3.0 / 4.0;
        let tile_count = std::cmp::max(4, m.bar.iter().take_while(|v| v.is_some()).count()) as f32;
        let bar_inner_w = TILE * tile_count + BORDER_WIDTH * 2.0;
        let left = PADDING_H + (g.body_w - bar_inner_w) + BORDER_WIDTH / 2.0;
        let inner_w = TILE - BORDER_WIDTH;
        let inner_h = SQUARE_SIZE / 2.0 - BORDER_WIDTH;
        // First up-to-4: filled inner + outline.
        for i in 0..4 {
            let x = left + i as f32 * TILE;
            let fill = m.bar.get(i).copied().flatten().unwrap_or(BG_FILL_COLOR);
            marks.fill_rect(x + BORDER_WIDTH / 2.0, top + BORDER_WIDTH / 2.0, inner_w, inner_h, fill);
            marks.stroke_rect(x, top, TILE, SQUARE_SIZE / 2.0, BORDER_STROKE_COLOR, BORDER_WIDTH);
        }
        // Remaining "bug" colors: filled inner, no outline, after a gap.
        for (j, c) in m.bar.iter().skip(4).enumerate() {
            let Some(c) = c else {
                continue;
            };
            let x = left + (j as f32 + 4.0) * TILE + BORDER_WIDTH;
            marks.fill_rect(x + BORDER_WIDTH / 2.0, top + BORDER_WIDTH / 2.0, inner_w, inner_h, *c);
        }
    }
}

fn paint_body(marks: &mut Marks, m: &GridModel, g: &Geometry) {
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
            marks.fill_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, BG_FILL_COLOR);
            marks.stroke_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, BORDER_STROKE_COLOR, BORDER_WIDTH);
        }
    }

    // Pass 2: fill each part's squares, then its plus borders + cross via
    // the shared edge walk (Pass 3 overlays the dark border between
    // distinct parts, so outer edges end up dark, separators stay plus).
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
            marks.fill_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, style.solid);
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
                    marks.stroke_line(x1, y1, x2, y2, style.plus, BORDER_WIDTH);
                }
                PartMark::Cross { col, row } => {
                    let (x, y) = cell_xy(col, row);
                    marks.stroke_line(
                        x + SQUARE_SIZE / 2.0,
                        y,
                        x + SQUARE_SIZE / 2.0,
                        y + SQUARE_SIZE,
                        style.plus,
                        BORDER_WIDTH,
                    );
                    marks.stroke_line(
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
                ((0i32, -1i32), (x, y, x + SQUARE_SIZE, y)),
                ((-1, 0), (x, y, x, y + SQUARE_SIZE)),
                ((0, 1), (x, y + SQUARE_SIZE, x + SQUARE_SIZE, y + SQUARE_SIZE)),
                ((1, 0), (x + SQUARE_SIZE, y, x + SQUARE_SIZE, y + SQUARE_SIZE)),
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
                    marks.stroke_line(x1, y1, x2, y2, BORDER_STROKE_COLOR, BORDER_WIDTH);
                }
            }
        }
    }

    // Pass 4: command-line markers.
    let cl = by + m.command_line as f32 * SQUARE_SIZE;
    for frac in [0.25_f32, 0.75] {
        let ly = cl + SQUARE_SIZE * frac;
        marks.stroke_line(bx, ly, bx + cols as f32 * SQUARE_SIZE, ly, BORDER_STROKE_COLOR, BORDER_WIDTH);
    }

    // Pass 5: out-of-bounds shading (the outer band, half-alpha black).
    if m.has_out_of_bounds {
        let band_w = SQUARE_SIZE + BORDER_WIDTH;
        let band_h = (rows as f32 - 2.0) * SQUARE_SIZE + BORDER_WIDTH;
        marks.fill_rect(
            bx - BORDER_WIDTH / 2.0,
            by + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_w,
            band_h,
            OOB_SHADE,
        );
        marks.fill_rect(
            bx + (cols as f32 - 1.0) * SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_w,
            band_h,
            OOB_SHADE,
        );
        marks.fill_rect(
            bx + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by - BORDER_WIDTH / 2.0,
            band_h,
            band_w,
            OOB_SHADE,
        );
        marks.fill_rect(
            bx + SQUARE_SIZE - BORDER_WIDTH / 2.0,
            by + (rows as f32 - 1.0) * SQUARE_SIZE - BORDER_WIDTH / 2.0,
            band_h,
            band_w,
            OOB_SHADE,
        );
    }
}

fn paint_ghost(marks: &mut Marks, g: &Geometry, gh: &Ghost) {
    let bx = g.body_origin_x + BORDER_WIDTH / 2.0;
    let by = g.body_origin_y + BORDER_WIDTH / 2.0;
    let tint = [gh.solid[0], gh.solid[1], gh.solid[2], 0x80];
    let outline = if gh.legal { GHOST_LEGAL } else { GHOST_ILLEGAL };
    // Plus lines pre-blended to "50% over the cell background" — see the
    // desktop's paint_ghost for why (opaque = too bright, translucent =
    // double-composited).
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

    for &(col, row) in &gh.cells {
        let (x, y) = cell_xy(col, row);
        marks.fill_rect(x, y, SQUARE_SIZE, SQUARE_SIZE, tint);
    }

    let adj = |c: isize, r: isize| {
        if cells.contains(&(c, r)) {
            Adj::Own
        } else if footprint.contains(&(c, r)) {
            Adj::Skip
        } else {
            Adj::Outside
        }
    };
    // Boundary edges stroked last so the legality outline sits on top of
    // the plus lines at shared corners.
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
                marks.stroke_line(line.0, line.1, line.2, line.3, plus, BORDER_WIDTH);
            } else {
                boundary.push(line);
            }
        }
        PartMark::Cross { col, row } => {
            let (x, y) = cell_xy(col, row);
            marks.stroke_line(
                x + SQUARE_SIZE / 2.0,
                y,
                x + SQUARE_SIZE / 2.0,
                y + SQUARE_SIZE,
                plus,
                BORDER_WIDTH,
            );
            marks.stroke_line(
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
        marks.stroke_line(x1, y1, x2, y2, outline, BORDER_WIDTH);
    }
}

/// Outline the outer boundary of the part occupying `slot` in white — the
/// hover highlight shared by the viewer and the editor. Stroke width
/// matches the desktop's display-space `(cell * 0.12).max(2.0)`.
fn paint_part_outline(marks: &mut Marks, m: &GridModel, g: &Geometry, slot: usize) {
    let bx = g.body_origin_x + BORDER_WIDTH / 2.0;
    let by = g.body_origin_y + BORDER_WIDTH / 2.0;
    let (cols, rows) = (m.cols, m.rows);
    let occ = |c: isize, r: isize| -> Option<usize> {
        if c < 0 || r < 0 || c >= cols as isize || r >= rows as isize {
            None
        } else {
            m.occupancy.get(r as usize * cols + c as usize).copied().flatten()
        }
    };
    let width = SQUARE_SIZE * 0.12;
    const WHITE: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
    for row in 0..rows {
        for col in 0..cols {
            if occ(col as isize, row as isize) != Some(slot) {
                continue;
            }
            let x = bx + col as f32 * SQUARE_SIZE;
            let y = by + row as f32 * SQUARE_SIZE;
            let same = |dc: isize, dr: isize| occ(col as isize + dc, row as isize + dr) == Some(slot);
            if !same(0, -1) {
                marks.stroke_line(x, y, x + SQUARE_SIZE, y, WHITE, width);
            }
            if !same(0, 1) {
                marks.stroke_line(x, y + SQUARE_SIZE, x + SQUARE_SIZE, y + SQUARE_SIZE, WHITE, width);
            }
            if !same(-1, 0) {
                marks.stroke_line(x, y, x, y + SQUARE_SIZE, WHITE, width);
            }
            if !same(1, 0) {
                marks.stroke_line(x + SQUARE_SIZE, y, x + SQUARE_SIZE, y + SQUARE_SIZE, WHITE, width);
            }
        }
    }
}

/// A small standalone thumbnail of one part's shape as an SVG: filled
/// cells in the part's solid color with a plus-color outline + separators,
/// on a transparent background, 8 CSS px per cell with 1px lines (the
/// desktop's `render_part_thumb`). `crop` trims to the shape's bounding
/// box; `dim` fades it for at-cap palette rows. `None` for an empty shape.
pub fn part_thumb_svg(
    bitmap: &tango_dataview::rom::NavicustBitmap,
    color: NavicustPartColor,
    is_solid: bool,
    crop: bool,
    dim: bool,
) -> Option<Element> {
    const PX: f32 = 8.0;
    let (h, w) = bitmap.dim();
    let mut cells: Vec<(usize, usize)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| bitmap[[y, x]])
        .collect();
    if cells.is_empty() {
        return None;
    }
    let (grid_w, grid_h) = if crop {
        let min_x = cells.iter().map(|&(x, _)| x).min().unwrap();
        let min_y = cells.iter().map(|&(_, y)| y).min().unwrap();
        let max_x = cells.iter().map(|&(x, _)| x).max().unwrap();
        let max_y = cells.iter().map(|&(_, y)| y).max().unwrap();
        for c in &mut cells {
            c.0 -= min_x;
            c.1 -= min_y;
        }
        (max_x - min_x + 1, max_y - min_y + 1)
    } else {
        (w, h)
    };
    let (solid, plus) = part_colors(color);
    let mut marks = Marks(Vec::new());
    for &(cx, cy) in &cells {
        marks.fill_rect(cx as f32 * PX, cy as f32 * PX, PX, PX, solid);
    }
    // 1px inset lines, drawn as thin fills like the desktop's pixel put.
    let own: std::collections::HashSet<(isize, isize)> = cells.iter().map(|&(c, r)| (c as isize, r as isize)).collect();
    for_each_part_edge(
        &cells,
        is_solid,
        |c, r| if own.contains(&(c, r)) { Adj::Own } else { Adj::Outside },
        |mark| match mark {
            PartMark::Edge { col, row, side, .. } => {
                let (ox, oy) = (col as f32 * PX, row as f32 * PX);
                let (x, y, w, h) = match side {
                    Side::Top => (ox, oy, PX, 1.0),
                    Side::Bottom => (ox, oy + PX - 1.0, PX, 1.0),
                    Side::Left => (ox, oy, 1.0, PX),
                    Side::Right => (ox + PX - 1.0, oy, 1.0, PX),
                };
                marks.fill_rect(x, y, w, h, plus);
            }
            PartMark::Cross { col, row } => {
                let (ox, oy) = (col as f32 * PX, row as f32 * PX);
                marks.fill_rect(ox + PX / 2.0, oy, 1.0, PX, plus);
                marks.fill_rect(ox, oy + PX / 2.0, PX, 1.0, plus);
            }
        },
    );
    let (vw, vh) = (grid_w as f32 * PX, grid_h as f32 * PX);
    let opacity = if dim { "0.35" } else { "1" };
    let nodes = marks.0;
    Some(rsx! {
        svg {
            view_box: "0 0 {vw} {vh}",
            width: "{vw}",
            height: "{vh}",
            opacity: "{opacity}",
            {nodes.into_iter()}
        }
    })
}

/// `bitmap` rotated clockwise `rot` quarter turns. Grids are square
/// (5×5 / 7×7), so a quarter turn preserves the dimensions.
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

/// The set bitmap cells of `bitmap`, rotated clockwise `rot` quarter
/// turns, expressed as `(dy, dx)` offsets from the (rotated) grid center
/// — exactly the offsets `navicust::materialize` applies when it stamps a
/// part at `(row, col)`.
#[allow(dead_code)] // the navicust editor's ghost (next phase)
pub fn rotated_offsets(bitmap: &tango_dataview::rom::NavicustBitmap, rot: u8) -> Vec<(isize, isize)> {
    let (h, w) = bitmap.dim();
    let n = h; // square grids only
    let mut cells: Vec<(usize, usize)> = Vec::new();
    for by in 0..h {
        for bx in 0..w {
            if bitmap[[by, bx]] {
                cells.push((by, bx));
            }
        }
    }
    // Clockwise quarter turn on a square grid: (by, bx) -> (bx, n-1-by).
    for _ in 0..(rot % 4) {
        for c in cells.iter_mut() {
            let (by, bx) = *c;
            *c = (bx, n - 1 - by);
        }
    }
    let center = (n / 2) as isize;
    cells
        .into_iter()
        .map(|(by, bx)| (by as isize - center, bx as isize - center))
        .collect()
}
