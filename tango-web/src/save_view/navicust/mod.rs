//! The NaviCust tab (the desktop's `save_view/navicust/mod.rs`): the
//! grid (SVG, see [`grid`]), a hover outline + name/description popover
//! over installed parts, the BN3 style label on the color bar, and the
//! installed-parts panel beside the grid.

use dioxus::prelude::*;
use tango_dataview::rom::NavicustPartColor;
use unic_langid::LanguageIdentifier;

use super::edit::{Edit, NavicustEdit as NavicustEditOp};
use super::{placeholder, stage_edit, EditUi, Loaded, SaveHandle};
use crate::t;
use crate::ui::icons;

pub mod grid;

/// Visual width the reference (7-wide) grid is drawn at, in CSS px —
/// matches the desktop editor's `DISPLAY_W`, which the viewer shares.
pub const DISPLAY_W: f32 = 360.0;

/// Maximum number of copies of one part (by id) allowed on the grid.
pub const MAX_COPIES_PER_PART: usize = 9;

/// A part picked up from the palette: its id plus the orientation +
/// compression it'll be dropped with (the desktop's `HeldPart`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldPart {
    pub id: usize,
    pub rot: u8,
    pub compressed: bool,
    /// Where on the part it was grabbed: the offset (in the *current*
    /// orientation) of the grabbed cell from the part's center anchor,
    /// as `(row, col)`. `(0, 0)` for palette pick-ups.
    pub grab_row: i8,
    pub grab_col: i8,
}

impl HeldPart {
    /// Rotate the grab point 90° clockwise to track `rot` being
    /// advanced — mirrors `rotated_offsets`' cell map: `(dy, dx) ->
    /// (dx, -dy)`.
    pub(super) fn rotate_grab_cw(&mut self) {
        let (r, c) = (self.grab_row, self.grab_col);
        self.grab_row = c;
        self.grab_col = -r;
    }
}

/// Solid + plus colors for an NCP color as CSS strings, for the
/// installed-parts badges.
fn ncp_css_colors(color: Option<NavicustPartColor>) -> (String, String) {
    match color {
        Some(c) => {
            let (solid, plus) = grid::part_colors(c);
            (
                format!("rgb({},{},{})", solid[0], solid[1], solid[2]),
                format!("rgb({},{},{})", plus[0], plus[1], plus[2]),
            )
        }
        None => ("rgb(189,189,189)".to_string(), "rgb(136,136,136)".to_string()),
    }
}

/// The hovered installed part's slot (drives the white block outline in
/// the grid; changes per cell crossing) and the cursor position (drives
/// only the follow-cursor popover, so per-pixel moves don't re-render the
/// whole save view).
pub(super) static NCP_HOVER_SLOT: GlobalSignal<Option<usize>> = Signal::global(|| None);
pub(super) static NCP_HOVER_POS: GlobalSignal<(f64, f64)> = Signal::global(|| (0.0, 0.0));

/// The NaviCust tab: the equipped navi's customizer grid.
pub(super) fn render_navicust_tab(lang: &LanguageIdentifier, loaded: &Loaded) -> Element {
    let Some(v) = loaded.save.view_navicust() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let [cols, rows_n] = v.size();

    let Some(layout) = assets.navicust_layout() else {
        return rsx! {
            div { class: "pane ncp-pane",
                span { class: "sub", {t!(lang, "navicust-grid-size", cols = cols as i64, rows = rows_n as i64)} }
            }
        };
    };

    // Read-only view renders the WRAM-materialized cache, like the desktop.
    let materialized = v.materialized();
    let model = grid::build_model(&materialized, &layout, v.as_ref(), assets);
    let hover_slot = *NCP_HOVER_SLOT.read();
    let svg = grid::grid_svg(&model, None, hover_slot);

    // Geometry for the hover overlay + style label, in display pixels.
    let g = grid::geometry(model.cols, model.rows);
    let scale = grid::display_scale(DISPLAY_W);
    let (dw, dh) = (g.total_w * scale, g.total_h * scale);
    let origin_x = (g.body_origin_x + grid::BORDER_WIDTH / 2.0) * scale;
    let origin_y = (g.body_origin_y + grid::BORDER_WIDTH / 2.0) * scale;
    let cell = grid::SQUARE_SIZE * scale;
    let (m_cols, m_rows) = (model.cols, model.rows);
    let occupancy = model.occupancy.clone();

    // BN3: the style name rides the color bar's left edge.
    let style_label = v
        .style()
        .and_then(|sid| assets.style(sid).and_then(|s| s.name()))
        .map(|name| {
            let left = (grid::PADDING_H + grid::BORDER_WIDTH + 4.0) * scale;
            let top = grid::PADDING_V * scale;
            let h = g.bar_h * scale;
            let max_w = (g.body_w - (grid::SQUARE_SIZE + grid::BORDER_WIDTH) - (grid::BORDER_WIDTH + 4.0) * 2.0) * scale;
            let font = h * 0.72;
            rsx! {
                div {
                    class: "ncp-style-label",
                    style: "left:{left}px;top:{top}px;height:{h}px;max-width:{max_w}px;font-size:{font}px",
                    "{name}"
                }
            }
        });

    let grid_el = rsx! {
        div { class: "ncp-wrap", style: "width:{dw}px;height:{dh}px",
            {svg}
            {style_label}
            div {
                class: "ncp-overlay",
                onmousemove: move |evt| {
                    let p = evt.element_coordinates();
                    let col = ((p.x - origin_x as f64) / cell as f64).floor();
                    let row = ((p.y - origin_y as f64) / cell as f64).floor();
                    let slot = (col >= 0.0 && row >= 0.0 && (col as usize) < m_cols && (row as usize) < m_rows)
                        .then(|| occupancy.get(row as usize * m_cols + col as usize).copied().flatten())
                        .flatten();
                    let c = evt.client_coordinates();
                    *NCP_HOVER_POS.write() = (c.x, c.y);
                    if *NCP_HOVER_SLOT.peek() != slot {
                        *NCP_HOVER_SLOT.write() = slot;
                    }
                },
                onmouseleave: move |_| {
                    if NCP_HOVER_SLOT.peek().is_some() {
                        *NCP_HOVER_SLOT.write() = None;
                    }
                },
            }
        }
    };

    // The installed-parts panel beside the grid.
    let parts = navicust_parts_panel(loaded, v.as_ref());

    rsx! {
        div { class: "pane ncp-pane",
            {grid_el}
            if let Some(parts) = parts {
                div { class: "ncp-parts", {parts} }
            }
        }
    }
}

/// The follow-cursor popover for the hovered installed part: name +
/// description on the standard tooltip chrome. Mounted once by the save
/// view; renders nothing while no part is hovered.
#[component]
pub(super) fn NcpPopover(handle: SaveHandle) -> Element {
    let slot = *NCP_HOVER_SLOT.read();
    let (x, y) = *NCP_HOVER_POS.read();
    let Some(slot) = slot else { return rsx! {} };
    let l = handle.0.borrow();
    let info = l
        .save
        .view_navicust()
        .and_then(|v| v.navicust_part(slot))
        .and_then(|p| l.assets.navicust_part(p.id));
    let Some(info) = info else { return rsx! {} };
    let name = info.name().unwrap_or_else(|| "?".to_string());
    let description = info.description();
    let flip = crate::host::viewport_width().is_some_and(|w| x > w - 320.0);
    rsx! {
        div {
            class: if flip { "chip-pop flip" } else { "chip-pop" },
            style: "left:{x}px; top:{y}px",
            span { "{name}" }
            if let Some(desc) = description {
                p { "{desc}" }
            }
        }
    }
}

/// The viewer's installed-parts panel, shown beside the grid: one row per
/// part with its shape thumbnail (bounding-box crop at the part's actual
/// rotation + compression), its name badge, and its description. Solid
/// parts in the left column, plus parts in the right, keeping slot order
/// within each group. `None` when nothing is installed.
fn navicust_parts_panel(loaded: &Loaded, v: &dyn tango_dataview::save::NavicustView) -> Option<Element> {
    let assets = loaded.assets.as_ref();
    let mut solid_rows: Vec<Element> = vec![];
    let mut plus_rows: Vec<Element> = vec![];
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let part_name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        let is_solid = info.is_solid();
        let (solid_color, plus_color) = ncp_css_colors(info.color());
        let bg = if is_solid { solid_color } else { plus_color };

        // Thumb at the slot's actual orientation, cropped to its bounding
        // box, centered in a fixed 40px box so the name column lines up.
        let thumb = info.color().and_then(|color| {
            let bitmap = info
                .compressed_bitmap()
                .filter(|_| part.compressed)
                .unwrap_or_else(|| info.uncompressed_bitmap());
            let rotated = grid::rotate_bitmap(&bitmap, part.rot);
            grid::part_thumb_svg(&rotated, color, is_solid, true, false)
        });

        let row_el = rsx! {
            div { class: "ncp-part-row",
                span { class: "thumb-box", {thumb} }
                div { class: "part-info",
                    span { class: "part-badge", style: "background:{bg}", "{part_name}" }
                    if let Some(desc) = info.description() {
                        p { class: "sub", "{desc}" }
                    }
                }
            }
        };
        if is_solid {
            solid_rows.push(row_el);
        } else {
            plus_rows.push(row_el);
        }
    }
    if solid_rows.is_empty() && plus_rows.is_empty() {
        return None;
    }
    Some(rsx! {
        div { class: "ncp-col",
            for r in solid_rows {
                {r}
            }
        }
        div { class: "ncp-col",
            for r in plus_rows {
                {r}
            }
        }
    })
}

/// Sort order for the navicust editor's palette pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavicustSort {
    Id,
    Name,
    Color,
}

impl NavicustSort {
    pub const ALL: [NavicustSort; 3] = [NavicustSort::Id, NavicustSort::Name, NavicustSort::Color];

    fn label(self, lang: &LanguageIdentifier) -> String {
        match self {
            NavicustSort::Id => t!(lang, "navicust-sort-id"),
            NavicustSort::Name => t!(lang, "navicust-sort-name"),
            NavicustSort::Color => t!(lang, "navicust-sort-color"),
        }
    }
}

/// Stable color ordering for the palette's Color sort.
fn ncp_color_rank(color: &Option<NavicustPartColor>) -> u8 {
    use NavicustPartColor as N;
    match color {
        Some(N::White) => 0,
        Some(N::Yellow) => 1,
        Some(N::Pink) => 2,
        Some(N::Red) => 3,
        Some(N::Blue) => 4,
        Some(N::Green) => 5,
        Some(N::Orange) => 6,
        Some(N::Purple) => 7,
        Some(N::Gray) => 8,
        None => 9,
    }
}

/// Every navicust part the ROM defines, as `(id, name, description)`,
/// filtered by `filter` (case-insensitive name match) and in `sort`
/// order. Caps how many variants of one name appear so near-duplicate
/// color/junk variants don't flood the list.
fn sorted_navicust_parts(loaded: &Loaded, sort: NavicustSort, filter: &str) -> Vec<(usize, String, Option<String>)> {
    let assets = loaded.assets.as_ref();
    let filter = filter.to_lowercase();
    struct E {
        id: usize,
        name: String,
        desc: Option<String>,
        color_rank: u8,
    }
    let mut rows: Vec<E> = Vec::new();
    let mut per_type: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for id in 0..assets.num_navicust_parts() {
        let Some(info) = assets.navicust_part(id) else { continue };
        // Skip unused/padding slots: a real part has a color and a
        // non-empty shape.
        let Some(color) = info.color() else { continue };
        if !info.uncompressed_bitmap().iter().any(|&set| set) {
            continue;
        }
        let Some(name) = info.name() else { continue };
        if name.trim().is_empty() {
            continue;
        }
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        let count = per_type.entry(name.clone()).or_insert(0);
        if *count >= 9 {
            continue;
        }
        *count += 1;
        rows.push(E {
            id,
            name,
            desc: info.description(),
            color_rank: ncp_color_rank(&Some(color)),
        });
    }
    match sort {
        NavicustSort::Id => {}
        NavicustSort::Name => rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id))),
        NavicustSort::Color => rows.sort_by(|a, b| a.color_rank.cmp(&b.color_rank).then(a.id.cmp(&b.id))),
    }
    rows.into_iter().map(|e| (e.id, e.name, e.desc)).collect()
}

/// The installed-parts badge strip shown under the editor grid: two
/// columns (solid | plus), each badge colored by its NCP color. Reads
/// the live view, so it reflects staged edits. `None` when empty.
fn installed_badges(loaded: &Loaded, v: &dyn tango_dataview::save::NavicustView) -> Option<Element> {
    let assets = loaded.assets.as_ref();
    let mut solid_col: Vec<Element> = vec![];
    let mut plus_col: Vec<Element> = vec![];
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let part_name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        let is_solid = info.is_solid();
        let (solid_color, plus_color) = ncp_css_colors(info.color());
        let bg = if is_solid { solid_color } else { plus_color };
        let badge = rsx! {
            span { class: "part-badge", style: "background:{bg}", "{part_name}" }
        };
        if is_solid {
            solid_col.push(badge);
        } else {
            plus_col.push(badge);
        }
    }
    if solid_col.is_empty() && plus_col.is_empty() {
        return None;
    }
    Some(rsx! {
        div { class: "ncp-badges",
            div { class: "ncp-col", {solid_col.into_iter()} }
            div { class: "ncp-col", {plus_col.into_iter()} }
        }
    })
}

/// The navicust editor: the interactive grid (left) + the part palette
/// (right). Click a palette row to pick a part up (in the orientation
/// its rotate / compress buttons show), click a legal cell to place it,
/// click an installed part to pick it back up, right-click to drop the
/// held part, scroll to rotate it.
#[component]
pub(super) fn NavicustEdit(handle: SaveHandle, editing: Signal<Option<EditUi>>, sort: Signal<NavicustSort>) -> Element {
    let mut editing = editing;
    let mut sort = sort;
    let lang = crate::i18n::LANG.read().clone();
    let edit_ui = editing.read().clone().unwrap_or_default();
    let mut hovered = use_signal(|| Option::<(usize, usize)>::None);

    let loaded_rc = handle.0.clone();
    let loaded = loaded_rc.borrow();
    let Some(v) = loaded.save.view_navicust() else {
        return placeholder(t!(&lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let size = v.size();
    let (cols, rows) = (size[0], size[1]);
    let Some(layout) = assets.navicust_layout() else {
        return placeholder(t!(&lang, "save-empty"));
    };

    // Live grid recomputed from the part slots (NOT the WRAM cache), so
    // staged edits show immediately. `materialize` takes `[rows, cols]`.
    let materialized = tango_dataview::navicust::materialize(v.as_ref(), [rows, cols], assets);
    let model = grid::build_model(&materialized, &layout, v.as_ref(), assets);
    let installed = (0..v.count()).filter(|&i| v.navicust_part(i).is_some()).count();
    let occupancy = model.occupancy.clone();
    let has_oob = model.has_out_of_bounds;
    let (m_cols, m_rows) = (model.cols, model.rows);

    // Held-part ghost data, resolved from the ROM.
    let held = edit_ui.held_part.and_then(|hp| {
        let info = assets.navicust_part(hp.id)?;
        let color = info.color()?;
        let bitmap = info
            .compressed_bitmap()
            .filter(|_| hp.compressed)
            .unwrap_or_else(|| info.uncompressed_bitmap());
        let (solid, plus) = grid::part_colors(color);
        Some((
            grid::rotated_offsets(&bitmap, hp.rot),
            (hp.grab_row as isize, hp.grab_col as isize),
            solid,
            plus,
            info.is_solid(),
        ))
    });
    let holding = held.is_some();

    // Ghost legality (the desktop `EditorGrid::ghost`): anchor from the
    // grab offset, every cell in-bounds / unoccupied / not a blocked
    // corner, and — on OOB grids — at least one cell in the playable area.
    let is_blocked_corner =
        move |col: usize, row: usize| has_oob && (col == 0 || col == m_cols - 1) && (row == 0 || row == m_rows - 1);
    let is_oob = move |col: usize, row: usize| has_oob && (col == 0 || col == m_cols - 1 || row == 0 || row == m_rows - 1);
    let occ = {
        let occupancy = occupancy.clone();
        move |col: usize, row: usize| occupancy.get(row * m_cols + col).copied().flatten()
    };
    let ghost_at = {
        let occ = occ.clone();
        move |col: usize, row: usize| -> Option<(grid::Ghost, (isize, isize))> {
            let (cells_off, (gy, gx), solid, plus, is_solid) = held.as_ref()?;
            let (acol, arow) = (col as isize - gx, row as isize - gy);
            let mut cells = Vec::with_capacity(cells_off.len());
            let mut footprint = Vec::with_capacity(cells_off.len());
            let mut legal = acol >= 0 && arow >= 0;
            for &(dy, dx) in cells_off {
                let cy = arow + dy;
                let cx = acol + dx;
                footprint.push((cx, cy));
                if cx < 0 || cy < 0 || cx >= m_cols as isize || cy >= m_rows as isize {
                    legal = false;
                    continue;
                }
                let (cx, cy) = (cx as usize, cy as usize);
                if is_blocked_corner(cx, cy) || occ(cx, cy).is_some() {
                    legal = false;
                }
                cells.push((cx, cy));
            }
            // A part may overhang the OOB ring, but not sit entirely in it.
            if has_oob && !cells.iter().any(|&(c, r)| !is_oob(c, r)) {
                legal = false;
            }
            Some((
                grid::Ghost {
                    cells,
                    footprint,
                    solid: *solid,
                    plus: *plus,
                    is_solid: *is_solid,
                    legal,
                },
                (acol, arow),
            ))
        }
    };

    let ghost = hovered().and_then(|(c, r)| ghost_at(c, r).map(|(g, _)| g));
    // When not holding, outline the block under the cursor.
    let hover_slot = if holding {
        None
    } else {
        hovered().and_then(|(c, r)| occ(c, r))
    };
    let svg = grid::grid_svg(&model, ghost.as_ref(), hover_slot);

    // Geometry for the pointer overlay, in display pixels.
    let g = grid::geometry(m_cols, m_rows);
    let scale = grid::display_scale(DISPLAY_W);
    let (dw, dh) = (g.total_w * scale, g.total_h * scale);
    let origin_x = (g.body_origin_x + grid::BORDER_WIDTH / 2.0) * scale;
    let origin_y = (g.body_origin_y + grid::BORDER_WIDTH / 2.0) * scale;
    let cell_px = grid::SQUARE_SIZE * scale;

    let cell_at = move |x: f64, y: f64| -> Option<(usize, usize)> {
        let cx = ((x - origin_x as f64) / cell_px as f64).floor();
        let cy = ((y - origin_y as f64) / cell_px as f64).floor();
        (cx >= 0.0 && cy >= 0.0 && (cx as usize) < m_cols && (cy as usize) < m_rows).then_some((cx as usize, cy as usize))
    };

    let on_click = {
        let handle = handle.clone();
        let ghost_at = ghost_at.clone();
        let occ = occ.clone();
        move |evt: MouseEvent| {
            let p = evt.element_coordinates();
            let Some((col, row)) = cell_at(p.x, p.y) else { return };
            if holding {
                // The legal ghost guarantees the anchor is non-negative.
                if let Some((g, (acol, arow))) = ghost_at(col, row) {
                    if g.legal {
                        editing.with_mut(|e| {
                            if let Some(e) = e.as_mut() {
                                if let Some(hp) = e.held_part.take() {
                                    stage_edit(
                                        &handle,
                                        Edit::Navicust(NavicustEditOp::AddPart(tango_dataview::save::NavicustPart {
                                            id: hp.id,
                                            col: acol as u8,
                                            row: arow as u8,
                                            rot: hp.rot,
                                            compressed: hp.compressed,
                                        })),
                                    );
                                }
                            }
                        });
                    }
                }
            } else if let Some(slot) = occ(col, row) {
                // Pick the installed part back up, grabbed at the clicked
                // cell, and sync the palette entry's orientation.
                let part = {
                    let l = handle.0.borrow();
                    l.save.view_navicust().and_then(|v| v.navicust_part(slot))
                };
                if let Some(p) = part {
                    // Grab the part at the clicked cell: store that cell's
                    // offset from the part's center anchor so it stays
                    // under the cursor while dragging.
                    editing.with_mut(|e| {
                        if let Some(e) = e.as_mut() {
                            e.held_part = Some(HeldPart {
                                id: p.id,
                                rot: p.rot,
                                compressed: p.compressed,
                                grab_row: row as i8 - p.row as i8,
                                grab_col: col as i8 - p.col as i8,
                            });
                            e.part_orient.insert(p.id, (p.rot, p.compressed));
                        }
                    });
                    stage_edit(&handle, Edit::Navicust(NavicustEditOp::RemovePart { slot }));
                }
            }
        }
    };

    let mut clear_held = move || {
        editing.with_mut(|e| {
            if let Some(e) = e.as_mut() {
                e.held_part = None;
            }
        });
    };

    let mut rotate_held = move || {
        editing.with_mut(|e| {
            if let Some(e) = e.as_mut() {
                if let Some(mut h) = e.held_part {
                    h.rot = (h.rot + 1) % 4;
                    h.rotate_grab_cw();
                    e.held_part = Some(h);
                    e.part_orient.insert(h.id, (h.rot, h.compressed));
                }
            }
        });
    };

    // Installed copies per part id — palette entries at the per-part cap
    // are shown disabled.
    let mut installed_counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for i in 0..v.count() {
        if let Some(p) = v.navicust_part(i) {
            *installed_counts.entry(p.id).or_insert(0) += 1;
        }
    }

    // ----- Right pane: the part palette -----
    let mut palette_rows: Vec<Element> = Vec::new();
    for (id, name, description) in sorted_navicust_parts(&loaded, sort(), &edit_ui.navicust_filter) {
        let at_cap = installed_counts.get(&id).copied().unwrap_or(0) >= MAX_COPIES_PER_PART;
        let (rot, compressed) = edit_ui.orient_of(id);
        let selected = edit_ui.held_part.is_some_and(|h| h.id == id);
        let info = loaded.assets.navicust_part(id);
        let thumb = info.as_ref().and_then(|info| {
            let color = info.color()?;
            let bitmap = info
                .compressed_bitmap()
                .filter(|_| compressed)
                .unwrap_or_else(|| info.uncompressed_bitmap());
            let rotated = grid::rotate_bitmap(&bitmap, rot);
            grid::part_thumb_svg(&rotated, color, info.is_solid(), false, at_cap)
        });
        // A part whose compressed and uncompressed shapes are identical
        // can't be (de)compressed.
        let compressible = info
            .as_ref()
            .and_then(|info| info.compressed_bitmap().map(|bmp| bmp != info.uncompressed_bitmap()))
            .unwrap_or(false);
        let pick = {
            move |_| {
                if at_cap {
                    return;
                }
                editing.with_mut(|e| {
                    if let Some(e) = e.as_mut() {
                        // Toggle: clicking the held part deselects it.
                        if e.held_part.is_some_and(|h| h.id == id) {
                            e.held_part = None;
                        } else {
                            let (rot, compressed) = e.orient_of(id);
                            e.held_part = Some(HeldPart {
                                id,
                                rot,
                                compressed,
                                grab_row: 0,
                                grab_col: 0,
                            });
                        }
                    }
                });
            }
        };
        let rotate_part = move |evt: MouseEvent| {
            evt.stop_propagation();
            editing.with_mut(|e| {
                if let Some(e) = e.as_mut() {
                    let (rot, compressed) = e.orient_of(id);
                    let rot = (rot + 1) % 4;
                    e.part_orient.insert(id, (rot, compressed));
                    if let Some(h) = e.held_part.as_mut() {
                        if h.id == id {
                            h.rot = rot;
                            h.rotate_grab_cw();
                        }
                    }
                }
            });
        };
        let compress_part = move |evt: MouseEvent| {
            evt.stop_propagation();
            editing.with_mut(|e| {
                if let Some(e) = e.as_mut() {
                    let (rot, compressed) = e.orient_of(id);
                    let compressed = !compressed;
                    e.part_orient.insert(id, (rot, compressed));
                    if let Some(h) = e.held_part.as_mut() {
                        if h.id == id {
                            h.compressed = compressed;
                            // The shape changed entirely — re-center the grab.
                            h.grab_row = 0;
                            h.grab_col = 0;
                        }
                    }
                }
            });
        };
        let compress_label = if compressed {
            t!(&lang, "navicust-edit-uncompress")
        } else {
            t!(&lang, "navicust-edit-compress")
        };
        let rotate_label = t!(&lang, "navicust-edit-rotate");
        palette_rows.push(rsx! {
            div {
                class: if selected { "palette-row selected" } else if at_cap { "palette-row disabled" } else { "palette-row" },
                onclick: pick,
                span { class: "thumb-box", {thumb} }
                div { class: "part-info",
                    span { class: if at_cap { "muted" } else { "" }, "{name}" }
                    if let Some(desc) = description.filter(|d| !d.trim().is_empty()) {
                        p { class: "sub", "{desc}" }
                    }
                }
                div { class: "grow" }
                div { class: "part-controls",
                    button { class: "btn compact", title: "{rotate_label}", onclick: rotate_part,
                        icons::RotateCw {}
                    }
                    button {
                        class: "btn compact",
                        title: "{compress_label}",
                        disabled: !compressible,
                        onclick: compress_part,
                        if compressed {
                            icons::Expand {}
                        } else {
                            icons::Shrink {}
                        }
                    }
                }
            }
        });
    }

    let count_label = t!(&lang, "navicust-edit-count", count = installed as i64);
    let clear_all = {
        let handle = handle.clone();
        move |()| {
            editing.with_mut(|e| {
                if let Some(e) = e.as_mut() {
                    e.held_part = None;
                }
            });
            stage_edit(&handle, Edit::Navicust(NavicustEditOp::ClearAll));
        }
    };

    let sort_options: Vec<String> = NavicustSort::ALL.iter().map(|s| s.label(&lang)).collect();
    let sort_selected = NavicustSort::ALL.iter().position(|s| *s == sort()).unwrap_or(0);
    let badges = installed_badges(&loaded, v.as_ref());
    let cursor_class = if holding {
        "ncp-overlay grabbing"
    } else if hover_slot.is_some() {
        "ncp-overlay grab"
    } else {
        "ncp-overlay"
    };

    rsx! {
        div { class: "editor-panes",
            div { class: "pane editor-pane",
                div { class: "editor-header",
                    div { class: "line",
                        span { {t!(&lang, "navicust-edit-grid")} }
                        span { class: "sub", "{count_label}" }
                        div { class: "grow" }
                        {super::clear_all_button(&lang, EventHandler::new(clear_all))}
                    }
                }
                div { class: "editor-scroll center",
                    div { class: "ncp-wrap", style: "width:{dw}px;height:{dh}px",
                        {svg}
                        div {
                            class: "{cursor_class}",
                            onmousemove: move |evt| {
                                let p = evt.element_coordinates();
                                let cell = cell_at(p.x, p.y);
                                if hovered() != cell {
                                    hovered.set(cell);
                                }
                                // Feed the name popover only while idle-hovering
                                // an installed part.
                                let c = evt.client_coordinates();
                                *NCP_HOVER_POS.write() = (c.x, c.y);
                                let slot = if holding {
                                    None
                                } else {
                                    cell.and_then(|(c, r)| occ(c, r))
                                };
                                if *NCP_HOVER_SLOT.peek() != slot {
                                    *NCP_HOVER_SLOT.write() = slot;
                                }
                            },
                            onmouseleave: move |_| {
                                hovered.set(None);
                                if NCP_HOVER_SLOT.peek().is_some() {
                                    *NCP_HOVER_SLOT.write() = None;
                                }
                            },
                            onclick: on_click,
                            oncontextmenu: move |evt: MouseEvent| {
                                evt.prevent_default();
                                if holding {
                                    clear_held();
                                }
                            },
                            onwheel: move |evt: WheelEvent| {
                                if holding {
                                    evt.prevent_default();
                                    rotate_held();
                                }
                            },
                        }
                    }
                    {badges}
                }
            }
            div { class: "pane editor-pane",
                {super::library_header(
                    t!(&lang, "navicust-edit-search"),
                    edit_ui.navicust_filter.clone(),
                    EventHandler::new(move |v: String| {
                        editing.with_mut(|e| {
                            if let Some(e) = e.as_mut() {
                                e.navicust_filter = v;
                            }
                        });
                    }),
                    t!(&lang, "save-edit-sort"),
                    sort_options,
                    sort_selected,
                    EventHandler::new(move |i: usize| {
                        if let Some(s) = NavicustSort::ALL.get(i) {
                            sort.set(*s);
                        }
                    }),
                )}
                div { class: "editor-scroll",
                    {palette_rows.into_iter()}
                }
            }
        }
    }
}

/// The NaviCust tab as text: style name first (BN3 only), then two TSV
/// columns — solid parts left, plus parts right, lined up row-by-row.
pub(crate) fn navicust_as_text(loaded: &Loaded) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let v = loaded.save.view_navicust()?;
    let mut out = String::new();
    if let Some(style_id) = v.style() {
        if let Some(name) = assets.style(style_id).and_then(|s| s.name()) {
            out.push_str(&name);
            out.push('\n');
        }
    }
    let mut solid = Vec::new();
    let mut plus = Vec::new();
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        if info.is_solid() {
            solid.push(name);
        } else {
            plus.push(name);
        }
    }
    for i in 0..solid.len().max(plus.len()) {
        let s = solid.get(i).map(String::as_str).unwrap_or("");
        let p = plus.get(i).map(String::as_str).unwrap_or("");
        out.push_str(s);
        out.push('\t');
        out.push_str(p);
        out.push('\n');
    }
    Some(out)
}
