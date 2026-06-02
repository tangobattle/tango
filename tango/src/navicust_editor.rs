//! iced-canvas frontends for the NaviCust grid. The actual drawing lives
//! in [`crate::navicust::paint`] (shared with the tiny-skia clipboard
//! path); this module just provides the canvas `Program`s that feed it an
//! iced-`Frame` backend:
//!
//! * [`StaticGrid`] — a non-interactive grid for the read-only Navi view.
//! * [`EditorGrid`] — the interactive editor: it ghosts the held part
//!   under the cursor and turns pointer events into placement / pickup /
//!   rotate actions.

use iced::widget::canvas::{self, Canvas, Frame, LineCap, Path, Stroke};
use iced::widget::Action;
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::navicust::{self, GridModel};
use crate::save_view::Action as Msg;

fn to_color(c: [u8; 4]) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3] as f32 / 255.0)
}

/// Outline the outer boundary of the part occupying `slot` (all its
/// cells) in white, in display coordinates — the hover highlight shared
/// by the editor and the read-only viewer.
fn draw_part_outline(
    frame: &mut Frame,
    occupancy: &[Option<usize>],
    cols: usize,
    rows: usize,
    origin_x: f32,
    origin_y: f32,
    cell: f32,
    slot: usize,
) {
    let stroke = Stroke::default()
        .with_color(Color::WHITE)
        .with_width((cell * 0.12).max(2.0))
        .with_line_cap(LineCap::Square);
    let occ = |c: isize, r: isize| -> Option<usize> {
        if c < 0 || r < 0 || c >= cols as isize || r >= rows as isize {
            None
        } else {
            occupancy.get(r as usize * cols + c as usize).copied().flatten()
        }
    };
    for row in 0..rows {
        for col in 0..cols {
            if occ(col as isize, row as isize) != Some(slot) {
                continue;
            }
            let x = origin_x + col as f32 * cell;
            let y = origin_y + row as f32 * cell;
            let same = |dc: isize, dr: isize| occ(col as isize + dc, row as isize + dr) == Some(slot);
            if !same(0, -1) {
                frame.stroke(&Path::line(Point::new(x, y), Point::new(x + cell, y)), stroke.clone());
            }
            if !same(0, 1) {
                frame.stroke(&Path::line(Point::new(x, y + cell), Point::new(x + cell, y + cell)), stroke.clone());
            }
            if !same(-1, 0) {
                frame.stroke(&Path::line(Point::new(x, y), Point::new(x, y + cell)), stroke.clone());
            }
            if !same(1, 0) {
                frame.stroke(&Path::line(Point::new(x + cell, y), Point::new(x + cell, y + cell)), stroke.clone());
            }
        }
    }
}

/// Per-cell pixel size of a palette thumbnail. Every part is drawn on the
/// same n×n grid at this size so the rows line up in the palette list.
const THUMB_PX: f32 = 8.0;

/// A small live thumbnail of one part's shape at a chosen orientation,
/// drawn the same way as the baked palette icons but as a canvas — so the
/// palette can show a rotated / uncompressed shape without re-baking an
/// image (and minting a fresh texture id) every frame. Emits no messages,
/// so it composes inside the pick-up button beneath it.
pub struct PartThumb {
    /// Set cells `(col, row)` within the n×n grid.
    cells: Vec<(usize, usize)>,
    n: usize,
    solid: [u8; 4],
    plus: [u8; 4],
    is_solid: bool,
}

impl PartThumb {
    /// Build from an already-rotated bitmap. `dim` fades it (the at-cap
    /// look, matching the baked icon's reduced opacity). `None` if empty.
    pub fn new(
        bitmap: &tango_dataview::rom::NavicustBitmap,
        color: tango_dataview::rom::NavicustPartColor,
        is_solid: bool,
        dim: bool,
    ) -> Option<Self> {
        let (h, w) = bitmap.dim();
        let mut cells = Vec::new();
        for r in 0..h {
            for c in 0..w {
                if bitmap[[r, c]] {
                    cells.push((c, r));
                }
            }
        }
        if cells.is_empty() {
            return None;
        }
        let (mut solid, mut plus) = navicust::part_colors(color);
        if dim {
            solid[3] = (solid[3] as f32 * 0.35) as u8;
            plus[3] = (plus[3] as f32 * 0.35) as u8;
        }
        Some(PartThumb { cells, n: h, solid, plus, is_solid })
    }

    pub fn view<M: 'static>(self) -> Element<'static, M> {
        let side = self.n as f32 * THUMB_PX;
        Canvas::new(self).width(Length::Fixed(side)).height(Length::Fixed(side)).into()
    }
}

impl<M> canvas::Program<M> for PartThumb {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let solid = to_color(self.solid);
        let plus = to_color(self.plus);
        // Solid bodies first.
        for &(c, r) in &self.cells {
            frame.fill_rectangle(
                Point::new(c as f32 * THUMB_PX, r as f32 * THUMB_PX),
                Size::new(THUMB_PX, THUMB_PX),
                solid,
            );
        }
        // Plus edges + cross via the shared shape walk — crisp 1px inset
        // lines (top/left of every cell, bottom/right only on the outer
        // boundary), matching the baked palette icons. Separators and outer
        // edges are both plus here.
        let own: std::collections::HashSet<(isize, isize)> =
            self.cells.iter().map(|&(c, r)| (c as isize, r as isize)).collect();
        navicust::for_each_part_edge(
            &self.cells,
            self.is_solid,
            |c, r| if own.contains(&(c, r)) { navicust::Adj::Own } else { navicust::Adj::Outside },
            |mark| match mark {
                navicust::PartMark::Edge { col, row, side, .. } => {
                    let ox = col as f32 * THUMB_PX;
                    let oy = row as f32 * THUMB_PX;
                    let (x, y, w, h) = match side {
                        navicust::Side::Top => (ox, oy, THUMB_PX, 1.0),
                        navicust::Side::Bottom => (ox, oy + THUMB_PX - 1.0, THUMB_PX, 1.0),
                        navicust::Side::Left => (ox, oy, 1.0, THUMB_PX),
                        navicust::Side::Right => (ox + THUMB_PX - 1.0, oy, 1.0, THUMB_PX),
                    };
                    frame.fill_rectangle(Point::new(x, y), Size::new(w, h), plus);
                }
                navicust::PartMark::Cross { col, row } => {
                    let ox = col as f32 * THUMB_PX;
                    let oy = row as f32 * THUMB_PX;
                    frame.fill_rectangle(Point::new(ox + THUMB_PX / 2.0, oy), Size::new(1.0, THUMB_PX), plus);
                    frame.fill_rectangle(Point::new(ox, oy + THUMB_PX / 2.0), Size::new(THUMB_PX, 1.0), plus);
                }
            },
        );
        vec![frame.into_geometry()]
    }
}

/// Visual width the grid is painted at, in logical pixels.
pub const DISPLAY_W: f32 = 360.0;

/// Corner-rounding of the editor grid, in display pixels — matches the
/// radius the read-only viewer image is masked to (see
/// `selection::build_navicust_render`).
const CORNER_RADIUS_DISPLAY: f32 = 4.0;

/// Maximum number of copies of one part (by id) allowed on the grid.
pub const MAX_COPIES_PER_PART: usize = 9;

/// The held part, pre-resolved for the ghost preview.
#[derive(Clone)]
pub struct Held {
    /// Set bitmap cells as `(dy, dx)` offsets from the grid center — the
    /// offsets `navicust::materialize` applies (see [`rotated_offsets`]).
    pub cells: Vec<(isize, isize)>,
    /// The grabbed cell's `(dy, dx)` offset from the part's center anchor.
    /// The part is anchored so this cell sits under the hovered cell (the
    /// center snaps there when it's `(0, 0)` — a palette pick-up).
    pub grab: (isize, isize),
    pub solid: [u8; 4],
    /// The part's "plus" (cross/border) color, drawn on non-solid parts.
    pub plus: [u8; 4],
    /// Whether the part is solid; non-solid parts get the plus/cross lines.
    pub is_solid: bool,
}

/// Interactive editor grid. Draws the full grid (via the shared routine)
/// plus a ghost of the held part, and maps pointer events to edit actions.
pub struct EditorGrid {
    model: GridModel,
    scale: f32,
    width: f32,
    height: f32,
    /// Display-space geometry for hit-testing.
    cell: f32,
    origin_x: f32,
    origin_y: f32,
    held: Option<Held>,
}

#[derive(Default)]
pub struct State {
    hovered: Option<(usize, usize)>,
}

impl EditorGrid {
    pub fn new(model: GridModel, held: Option<Held>) -> Self {
        let g = navicust::geometry(model.cols, model.rows, model.has_excode);
        let scale = navicust::display_scale(DISPLAY_W);
        EditorGrid {
            cell: navicust::SQUARE_SIZE * scale,
            origin_x: (g.body_origin_x + navicust::BORDER_WIDTH / 2.0) * scale,
            origin_y: (g.body_origin_y + navicust::BORDER_WIDTH / 2.0) * scale,
            width: g.total_w * scale,
            height: g.total_h * scale,
            scale,
            model,
            held,
        }
    }

    pub fn view(self) -> Element<'static, Msg> {
        let (w, h) = (self.width, self.height);
        Canvas::new(self).width(Length::Fixed(w)).height(Length::Fixed(h)).into()
    }

    fn cell_at(&self, p: Point) -> Option<(usize, usize)> {
        let x = p.x - self.origin_x;
        let y = p.y - self.origin_y;
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let col = (x / self.cell) as usize;
        let row = (y / self.cell) as usize;
        (col < self.model.cols && row < self.model.rows).then_some((col, row))
    }

    fn is_blocked_corner(&self, col: usize, row: usize) -> bool {
        self.model.has_out_of_bounds
            && (col == 0 || col == self.model.cols - 1)
            && (row == 0 || row == self.model.rows - 1)
    }

    /// Whether `(col, row)` is in the out-of-bounds outer ring (BN6).
    fn is_oob(&self, col: usize, row: usize) -> bool {
        self.model.has_out_of_bounds
            && (col == 0 || col == self.model.cols - 1 || row == 0 || row == self.model.rows - 1)
    }

    fn occ(&self, col: usize, row: usize) -> Option<usize> {
        self.model.occupancy.get(row * self.model.cols + col).copied().flatten()
    }

    /// Build the ghost for the held part with the grabbed cell under
    /// `(col, row)`. The center anchor that `materialize` stores is the
    /// hovered cell minus the grab offset; `PlaceHeld` carries that anchor.
    fn anchor(&self, col: usize, row: usize) -> Option<(isize, isize)> {
        let (gy, gx) = self.held.as_ref()?.grab;
        Some((col as isize - gx, row as isize - gy))
    }

    /// Build the ghost for the held part with its grabbed cell at `(col, row)`.
    fn ghost(&self, col: usize, row: usize) -> Option<navicust::Ghost> {
        let held = self.held.as_ref()?;
        let (acol, arow) = self.anchor(col, row)?;
        let mut cells = Vec::with_capacity(held.cells.len());
        let mut footprint = Vec::with_capacity(held.cells.len());
        // The center anchor is stored as `u8`, so it must be representable.
        let mut legal = acol >= 0 && arow >= 0;
        for &(dy, dx) in &held.cells {
            let cy = arow + dy;
            let cx = acol + dx;
            footprint.push((cx, cy));
            if cx < 0 || cy < 0 || cx >= self.model.cols as isize || cy >= self.model.rows as isize {
                legal = false;
                continue;
            }
            let (cx, cy) = (cx as usize, cy as usize);
            if self.is_blocked_corner(cx, cy) || self.occ(cx, cy).is_some() {
                legal = false;
            }
            cells.push((cx, cy));
        }
        // A part may overhang the out-of-bounds ring, but not sit entirely
        // in it — at least one cell must be inside the playable area.
        if self.model.has_out_of_bounds && !cells.iter().any(|&(c, r)| !self.is_oob(c, r)) {
            legal = false;
        }
        Some(navicust::Ghost {
            cells,
            footprint,
            solid: held.solid,
            plus: held.plus,
            is_solid: held.is_solid,
            legal,
        })
    }
}

impl canvas::Program<Msg> for EditorGrid {
    type State = State;

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let ghost = state.hovered.and_then(|(col, row)| self.ghost(col, row));
        // `paint` works in native units and scales each draw by `scale`;
        // pass the radius in native units so it lands on
        // `CORNER_RADIUS_DISPLAY`. The hover outline below is in display
        // coords, drawn unscaled.
        let scale = self.scale;
        let bg_radius = CORNER_RADIUS_DISPLAY / scale;
        navicust::paint(&mut frame, &self.model, ghost.as_ref(), bg_radius, scale);
        // When not holding a part, outline the block under the cursor.
        if self.held.is_none() {
            if let Some((col, row)) = state.hovered {
                if let Some(slot) = self.occ(col, row) {
                    draw_part_outline(
                        &mut frame,
                        &self.model.occupancy,
                        self.model.cols,
                        self.model.rows,
                        self.origin_x,
                        self.origin_y,
                        self.cell,
                        slot,
                    );
                }
            }
        }
        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Msg>> {
        let inside = cursor.position_in(bounds);
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let cell = inside.and_then(|p| self.cell_at(p));
                if cell != state.hovered {
                    state.hovered = cell;
                    return Some(Action::request_redraw());
                }
            }
            iced::Event::Mouse(mouse::Event::CursorLeft) => {
                if state.hovered.take().is_some() {
                    return Some(Action::request_redraw());
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some((col, row)) = inside.and_then(|p| self.cell_at(p)) {
                    if self.held.is_some() {
                        // `PlaceHeld` stores the part's center anchor, which
                        // the grab offset shifts off the hovered cell. The
                        // legal ghost guarantees the anchor is non-negative.
                        if self.ghost(col, row).map(|g| g.legal).unwrap_or(false) {
                            if let Some((acol, arow)) = self.anchor(col, row) {
                                return Some(
                                    Action::publish(Msg::PlaceHeld {
                                        col: acol as u8,
                                        row: arow as u8,
                                    })
                                    .and_capture(),
                                );
                            }
                        }
                    } else if let Some(slot) = self.occ(col, row) {
                        return Some(
                            Action::publish(Msg::PickUpInstalledPart {
                                slot,
                                col: col as u8,
                                row: row as u8,
                            })
                            .and_capture(),
                        );
                    }
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if self.held.is_some() && cursor.is_over(bounds) {
                    return Some(Action::publish(Msg::ClearHeld).and_capture());
                }
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { .. }) => {
                if self.held.is_some() && cursor.is_over(bounds) {
                    return Some(Action::publish(Msg::RotateHeld).and_capture());
                }
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(&self, _state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        let Some(p) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        if self.held.is_some() {
            // Carrying a part to drop it — closed "grabbing" hand.
            mouse::Interaction::Grabbing
        } else if self.cell_at(p).and_then(|(c, r)| self.occ(c, r)).is_some() {
            // Hovering a placed part you can pick up — open "grab" hand.
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

/// A transparent overlay for the read-only viewer that outlines the whole
/// part (block) under the cursor. Generic over the host message type and
/// never captures events, so the tooltip layer beneath it still works.
pub struct HoverOutline {
    pub cols: usize,
    pub rows: usize,
    pub origin_x: f32,
    pub origin_y: f32,
    pub cell: f32,
    pub width: f32,
    pub height: f32,
    /// Row-major occupancy (`row * cols + col`): the part slot per cell.
    pub occupancy: Vec<Option<usize>>,
}

#[derive(Default)]
pub struct HoverState {
    hovered: Option<(usize, usize)>,
}

impl HoverOutline {
    pub fn view<M: 'static>(self) -> Element<'static, M> {
        let (w, h) = (self.width, self.height);
        Canvas::new(self).width(Length::Fixed(w)).height(Length::Fixed(h)).into()
    }

    fn cell_at(&self, p: Point) -> Option<(usize, usize)> {
        let x = p.x - self.origin_x;
        let y = p.y - self.origin_y;
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let col = (x / self.cell) as usize;
        let row = (y / self.cell) as usize;
        (col < self.cols && row < self.rows).then_some((col, row))
    }

    fn occ(&self, col: isize, row: isize) -> Option<usize> {
        if col < 0 || row < 0 || col >= self.cols as isize || row >= self.rows as isize {
            return None;
        }
        self.occupancy.get(row as usize * self.cols + col as usize).copied().flatten()
    }
}

impl<M> canvas::Program<M> for HoverOutline {
    type State = HoverState;

    fn draw(
        &self,
        state: &HoverState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if let Some((hc, hr)) = state.hovered {
            if let Some(slot) = self.occ(hc as isize, hr as isize) {
                draw_part_outline(
                    &mut frame,
                    &self.occupancy,
                    self.cols,
                    self.rows,
                    self.origin_x,
                    self.origin_y,
                    self.cell,
                    slot,
                );
            }
        }
        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut HoverState,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<M>> {
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let cell = cursor.position_in(bounds).and_then(|p| self.cell_at(p));
                if cell != state.hovered {
                    state.hovered = cell;
                    // Redraw to move the outline; don't capture — the
                    // tooltip layer beneath still needs the event.
                    return Some(Action::request_redraw());
                }
            }
            iced::Event::Mouse(mouse::Event::CursorLeft) => {
                if state.hovered.take().is_some() {
                    return Some(Action::request_redraw());
                }
            }
            _ => {}
        }
        None
    }
}

/// The set bitmap cells of `bitmap`, rotated clockwise `rot` quarter
/// turns, expressed as `(dy, dx)` offsets from the (rotated) grid center
/// — exactly the offsets `navicust::materialize` applies when it stamps a
/// part at `(row, col)`. Grids are square (5×5 / 7×7), so a quarter turn
/// preserves the dimensions. Inverse of dropping the stamp center on the
/// hovered cell.
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
    // Matches `navicust::rotate90` (transpose + reverse rows).
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
